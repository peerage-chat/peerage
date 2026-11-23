package chat.peerage.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.lazy.items
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R
import kotlinx.coroutines.Dispatchers
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
 * Панель: поиск, «Недавние» и сетка группы, снизу — ряд групп и забой. Пока идёт поиск, сетка
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
    var category by remember { mutableStateOf(0) }
    var query by remember { mutableStateOf("") }
    var searchFocused by remember { mutableStateOf(false) }
    val gridState = rememberLazyGridState()
    val scope = rememberCoroutineScope()
    val searching = searchFocused || query.isNotBlank()
    val found = remember(entries, query) { entries?.let { EmojiIndex.search(it, query) }.orEmpty() }

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
        if (recent.isNotEmpty()) {
            SectionLabel(stringResource(R.string.emoji_recent))
            Row(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.s)) {
                recent.take(8).forEach { e ->
                    EmojiCell(e, Modifier.weight(1f)) { onPick(e) }
                }
                repeat(8 - recent.take(8).size) { Spacer(Modifier.weight(1f)) }
            }
        }

        SectionLabel(stringResource(Groups[category].labelRes))
        val items = remember(entries, category) { entries?.filter { it.group == category }?.map { it.emoji }.orEmpty() }
        LazyVerticalGrid(
            columns = GridCells.Fixed(8),
            state = gridState,
            modifier = Modifier.fillMaxWidth().height(220.dp).padding(horizontal = Tokens.Space.s),
        ) {
            items(items) { e -> EmojiCell(e) { onPick(e) } }
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
                        .clickable {
                            category = i
                            scope.launch { gridState.scrollToItem(0) }
                        },
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
            if (onBackspace != null) {
                Box(
                    Modifier.size(44.dp).clip(RoundedCornerShape(Tokens.Radius.chip)).clickable { onBackspace() },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(
                        Icons.AutoMirrored.Filled.Backspace,
                        stringResource(R.string.emoji_backspace),
                        tint = HoloSecondary,
                        modifier = Modifier.size(Tokens.Size.iconM),
                    )
                }
            }
        }
    }
}

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
