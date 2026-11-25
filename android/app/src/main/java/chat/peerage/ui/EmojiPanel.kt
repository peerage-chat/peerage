package chat.peerage.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Backspace
import androidx.compose.material.icons.filled.DirectionsCar
import androidx.compose.material.icons.filled.EmojiPeople
import androidx.compose.material.icons.filled.EmojiSymbols
import androidx.compose.material.icons.filled.Fastfood
import androidx.compose.material.icons.filled.Flag
import androidx.compose.material.icons.filled.Lightbulb
import androidx.compose.material.icons.filled.Mood
import androidx.compose.material.icons.filled.Pets
import androidx.compose.material.icons.filled.SportsSoccer
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * Панель эмодзи для строки ввода (макет `docs/design/mockups/emoji-panel.html`).
 *
 * Ведёт себя как лист вложений: выезжает ИЗ строки ввода, сама строка остаётся на
 * месте. Эмодзи здесь — обычные системные символы, они едут в тексте сообщения и
 * ничего не добавляют к протоколу. Не путать с эмодзи-fingerprint: там свой набор
 * ровно потому, что системные рендерятся по-разному на разных устройствах.
 */

/** Группа набора (порядок групп Unicode): иконка для нижнего ряда и подпись. */
private class EmojiGroup(val icon: ImageVector, val labelRes: Int)

private val Groups = listOf(
    EmojiGroup(Icons.Filled.Mood, R.string.emoji_cat_smileys),
    EmojiGroup(Icons.Filled.EmojiPeople, R.string.emoji_cat_people),
    EmojiGroup(Icons.Filled.Pets, R.string.emoji_cat_animals),
    EmojiGroup(Icons.Filled.Fastfood, R.string.emoji_cat_food),
    EmojiGroup(Icons.Filled.SportsSoccer, R.string.emoji_cat_activity),
    EmojiGroup(Icons.Filled.DirectionsCar, R.string.emoji_cat_travel),
    EmojiGroup(Icons.Filled.Lightbulb, R.string.emoji_cat_objects),
    EmojiGroup(Icons.Filled.EmojiSymbols, R.string.emoji_cat_symbols),
    EmojiGroup(Icons.Filled.Flag, R.string.emoji_cat_flags),
)

/**
 * Полный набор Unicode ([EmojiIndex], `assets/emoji.tsv`) без того, что шрифт телефона не
 * рисует: на Android 11 нет эмодзи последних лет, и квадратики в панели хуже, чем их отсутствие.
 * Читается и проверяется один раз за процесс, не на главном потоке.
 */
private object EmojiStore {
    @Volatile var entries: List<EmojiIndex.Entry>? = null

    fun load(ctx: android.content.Context): List<EmojiIndex.Entry> = entries ?: synchronized(this) {
        entries ?: run {
            val paint = android.graphics.Paint()
            val all = runCatching { ctx.assets.open("emoji.tsv").bufferedReader().use { it.readText() } }.getOrDefault("")
            EmojiIndex.parse(all).filter { paint.hasGlyph(it.emoji) }.also { entries = it }
        }
    }
}

/**
 * Панель: поиск, «Недавние» и сетка группы, снизу — ряд групп и забой. Группы листаются и
 * свайпом вбок, и нажатием на ряд; ряд подсвечивает текущую. Пока идёт поиск, сетка
 * сворачивается в строку результатов: над панелью поднимается клавиатура, и вместе с сеткой
 * они не поместились бы.
 *
 * @param recent последние использованные символы (первым — самый свежий)
 * @param onPick вставить символ в текущую позицию курсора
 * @param onBackspace удалить один символ перед курсором; null — забоя нет (выбор реакции)
 */
@Composable
internal fun EmojiPanel(
    recent: List<String>,
    onPick: (String) -> Unit,
    onBackspace: (() -> Unit)?,
    onClose: () -> Unit = {},
) {
    val ctx = LocalContext.current
    val entries by produceState(EmojiStore.entries) {
        if (value == null) value = withContext(Dispatchers.Default) { EmojiStore.load(ctx.applicationContext) }
    }
    // Группа — страница: листаются вбок, как вкладки (п. 33); внутри страницы — прокрутка вниз.
    val pager = rememberPagerState { Groups.size }
    val category = pager.currentPage
    var query by remember { mutableStateOf("") }
    var searchFocused by remember { mutableStateOf(false) }
    val scope = rememberCoroutineScope()
    val searching = searchFocused || query.isNotBlank()
    val found = remember(entries, query) { entries?.let { EmojiIndex.search(it, query) }.orEmpty() }
    // Клавиатура поднята (п. 53): тапнули в поле сообщения при открытой панели — над
    // клавиатурой помещаются и панель, и поле, только если сетка ужмётся до двух рядов без
    // подписей. Спрятали клавиатуру «назад» посреди поиска — поиск закончен: иначе панель
    // оставалась свёрнутой в одну пустую строку.
    val ime = rememberImeVisible()
    val focus = LocalFocusManager.current
    LaunchedEffect(ime) { if (!ime && searchFocused) focus.clearFocus() }

    Column(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(topStart = 18.dp, topEnd = 18.dp))
            .background(HoloSurface),
    ) {
        // Ручка — тот же приём, что у листа вложений: панель «выехала» из строки ввода, и
        // потянуть её вниз — закрыть.
        PanelGrip(onClose)
        EmojiSearchField(query, { query = it }, { searchFocused = it })

        if (searching) {
            // Строка результатов над клавиатурой; не нашлось — словами, а не пустой полосой.
            if (query.isNotBlank() && found.isEmpty()) {
                Box(Modifier.fillMaxWidth().height(44.dp).padding(start = Tokens.Space.l), contentAlignment = Alignment.CenterStart) {
                    Text(stringResource(R.string.emoji_search_empty), color = HoloSecondary, fontSize = Tokens.Font.body)
                }
            } else {
                LazyRow(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.s)) {
                    items(if (query.isBlank()) recent else found) { e -> EmojiCell(e, Modifier.width(44.dp)) { onPick(e) } }
                }
            }
            Spacer(Modifier.height(Tokens.Space.xs))
            return@Column
        }

        // Недавние: реально в ходу десяток символов, они должны быть сразу под пальцем.
        if (recent.isNotEmpty() && !ime) {
            SectionLabel(stringResource(R.string.emoji_recent))
            Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.s)) {
                recent.take(8).forEach { e ->
                    EmojiCell(e, Modifier.weight(1f)) { onPick(e) }
                }
                repeat(8 - recent.take(8).size) { Spacer(Modifier.weight(1f)) }
            }
        }

        if (!ime) SectionLabel(stringResource(Groups[category].labelRes))
        val byGroup = remember(entries) { entries?.groupBy({ it.group }, { it.emoji }).orEmpty() }
        HorizontalPager(pager, Modifier.fillMaxWidth().height(if (ime) 44.dp * GRID_ROWS_COMPACT else 220.dp)) { page ->
            LazyVerticalGrid(
                columns = GridCells.Fixed(8),
                modifier = Modifier.fillMaxSize().padding(horizontal = Tokens.Space.s),
            ) {
                items(byGroup[page].orEmpty()) { e -> EmojiCell(e) { onPick(e) } }
            }
        }

        // Группы снизу: до них дотягивается палец, и они не уезжают при прокрутке.
        Row(
            Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.s, vertical = Tokens.Space.xs),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Groups.forEachIndexed { i, c ->
                val on = i == category
                Box(
                    Modifier.weight(1f).height(38.dp).clip(RoundedCornerShape(Tokens.Radius.chip))
                        .background(if (on) FolderSelected else Color.Transparent)
                        .clickable { scope.launch { pager.animateScrollToPage(i) } },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        c.icon,
                        stringResource(c.labelRes),
                        tint = if (on) HoloAccent else HoloSecondary,
                        modifier = Modifier.size(Tokens.Size.iconM),
                    )
                }
            }
            // Забой: системная клавиатура сейчас скрыта, а эмодзи ставят подряд и промахиваются.
            if (onBackspace != null) BackspaceKey(onBackspace)
        }
    }
}

/**
 * Забой с автоповтором, как на клавиатуре (п. 55): нажатие — один символ, удержание —
 * после паузы стирает подряд, пока палец на кнопке.
 */
@Composable
private fun BackspaceKey(onBackspace: () -> Unit) {
    val del by rememberUpdatedState(onBackspace)
    val scope = rememberCoroutineScope()
    val label = stringResource(R.string.emoji_backspace)
    Box(
        Modifier.size(44.dp).clip(RoundedCornerShape(Tokens.Radius.chip))
            .semantics {
                role = Role.Button
                onClick(label) {
                    del()
                    true
                }
            }
            .pointerInput(Unit) {
                awaitEachGesture {
                    awaitFirstDown()
                    del()
                    val repeat = scope.launch {
                        delay(REPEAT_START_MS)
                        while (true) {
                            del()
                            delay(REPEAT_EVERY_MS)
                        }
                    }
                    waitForUpOrCancellation()
                    repeat.cancel()
                }
            },
        contentAlignment = Alignment.Center,
    ) {
        Icon(Icons.AutoMirrored.Filled.Backspace, label, tint = HoloSecondary, modifier = Modifier.size(Tokens.Size.iconM))
    }
}

/** Рядов сетки, пока поднята клавиатура: панель, поле и клавиатура — на одном экране. */
private const val GRID_ROWS_COMPACT = 2

/** Пауза до автоповтора и шаг — как у системной клавиатуры. */
private const val REPEAT_START_MS = 400L
private const val REPEAT_EVERY_MS = 50L

/** Поиск по словам (ru, en). Крестик очищает и снимает фокус — панель возвращается к сетке. */
@Composable
private fun EmojiSearchField(query: String, onQuery: (String) -> Unit, onFocus: (Boolean) -> Unit) {
    val focus = LocalFocusManager.current
    Row(
        Modifier.padding(horizontal = Tokens.Space.m).fillMaxWidth().height(Tokens.Size.touch)
            .clip(RoundedCornerShape(Tokens.Radius.field)).background(Tokens.Colors.raised)
            .padding(start = Tokens.Space.m),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(TablerIcons.Search, null, tint = Tokens.Colors.tertiary, modifier = Modifier.size(Tokens.Size.iconS))
        Spacer(Modifier.width(Tokens.Space.s))
        BasicTextField(
            value = query,
            onValueChange = onQuery,
            singleLine = true,
            textStyle = Tokens.Text.base.copy(color = Color.White, fontSize = Tokens.Font.body),
            cursorBrush = SolidColor(HoloAccent),
            modifier = Modifier.weight(1f).onFocusChanged { onFocus(it.isFocused) },
            decorationBox = { inner ->
                Box(contentAlignment = Alignment.CenterStart) {
                    if (query.isEmpty()) Text(stringResource(R.string.emoji_search), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.body)
                    inner()
                }
            },
        )
        if (query.isNotEmpty()) {
            Box(
                Modifier.size(Tokens.Size.touch).clickable {
                    onQuery("")
                    focus.clearFocus()
                },
                contentAlignment = Alignment.Center,
            ) {
                Icon(TablerIcons.X, stringResource(R.string.emoji_search_clear), tint = Tokens.Colors.tertiary, modifier = Modifier.size(Tokens.Size.iconS))
            }
        }
    }
}

@Composable
private fun SectionLabel(text: String) {
    Text(
        text.uppercase(),
        color = HoloSecondary,
        fontSize = Tokens.Font.caption,
        fontWeight = FontWeight.Medium,
        modifier = Modifier.padding(start = Tokens.Space.l, top = Tokens.Space.s, bottom = Tokens.Space.xs),
    )
}

/** Ячейка 44dp — цель нажатия задаёт она сама, видимых зазоров нет. */
@Composable
private fun EmojiCell(emoji: String, modifier: Modifier = Modifier, onClick: () -> Unit) {
    Box(
        modifier.height(44.dp).clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) {
        Text(emoji, fontSize = Tokens.Font.big)
    }
}
