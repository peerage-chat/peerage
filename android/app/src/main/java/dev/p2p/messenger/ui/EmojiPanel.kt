package dev.p2p.messenger.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectVerticalDragGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.grid.rememberLazyGridState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Backspace
import androidx.compose.material.icons.filled.DirectionsCar
import androidx.compose.material.icons.filled.EmojiSymbols
import androidx.compose.material.icons.filled.Fastfood
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.R
import kotlinx.coroutines.launch

/**
 * Панель эмодзи для строки ввода (макет `docs/design/mockups/emoji-panel.html`).
 *
 * Ведёт себя как лист вложений: выезжает ИЗ строки ввода, сама строка остаётся на
 * месте. Эмодзи здесь — обычные системные символы, они едут в тексте сообщения и
 * ничего не добавляют к протоколу. Не путать с эмодзи-fingerprint: там свой набор
 * ровно потому, что системные рендерятся по-разному на разных устройствах.
 */

/** Категория набора: иконка для нижнего ряда + сами символы. */
internal data class EmojiCategory(val icon: ImageVector, val labelRes: Int, val items: List<String>)

private fun chars(s: String): List<String> = s.split(" ").filter { it.isNotBlank() }

/** Курируемый набор: по паре десятков ходовых символов на категорию. Полный Unicode
 *  здесь не нужен — панель для быстрой вставки, редкое ищут системной клавиатурой. */
internal val EmojiSet: List<EmojiCategory> = listOf(
    EmojiCategory(
        Icons.Filled.Mood,
        R.string.emoji_cat_smileys,
        chars(
            "😀 😃 😄 😁 😆 😅 🤣 😂 🙂 🙃 😉 😊 😇 🥰 😍 🤩 😘 😗 😚 😙 " +
                "😋 😛 😜 🤪 🤨 🧐 🤓 😎 🥸 🤯 😴 🤔 🤗 🤭 🤫 😶 😐 😑 😬 🙄 " +
                "😮 😯 😲 😳 🥺 😢 😭 😤 😠 😡 🤬 😱 😨 😰 😥 😓 🤢 🤮 🥵 🥶 " +
                "👍 👎 👌 🤝 🙏 👏 🙌 💪 🤞 ✌️ 👋 🖐️ ✊ 👊 🫶 🤟 ☝️ 👇 👈 👉",
        ),
    ),
    EmojiCategory(
        Icons.Filled.Pets,
        R.string.emoji_cat_animals,
        chars(
            "🐶 🐱 🐭 🐹 🐰 🦊 🐻 🐼 🐨 🐯 🦁 🐮 🐷 🐸 🐵 🐔 🐧 🐦 🐤 🦆 " +
                "🦅 🦉 🦇 🐺 🐗 🐴 🦄 🐝 🐛 🦋 🐌 🐞 🐜 🕷️ 🦂 🐢 🐍 🦎 🐙 🦑 " +
                "🌵 🌲 🌳 🌴 🌱 🌿 ☘️ 🍀 🎋 🍁 🍂 🍃 🌺 🌸 🌼 🌻 🌹 🌷 🌍 🌙",
        ),
    ),
    EmojiCategory(
        Icons.Filled.Fastfood,
        R.string.emoji_cat_food,
        chars(
            "🍏 🍎 🍐 🍊 🍋 🍌 🍉 🍇 🍓 🫐 🍈 🍒 🍑 🥭 🍍 🥥 🥝 🍅 🥑 🍆 " +
                "🥔 🥕 🌽 🌶️ 🥒 🥬 🥦 🧄 🧅 🍄 🥜 🍞 🥐 🥖 🧀 🥚 🍳 🥞 🧇 🥓 " +
                "🍔 🍟 🍕 🌭 🥪 🌮 🌯 🥗 🍝 🍜 🍲 🍣 🍱 🍤 🍚 🍦 🍰 🧁 🍫 🍿 " +
                "☕ 🍵 🥤 🍺 🍻 🥂 🍷 🥃 🍸 🧊",
        ),
    ),
    EmojiCategory(
        Icons.Filled.SportsSoccer,
        R.string.emoji_cat_activity,
        chars(
            "⚽ 🏀 🏈 ⚾ 🎾 🏐 🏉 🎱 🏓 🏸 🥅 ⛳ 🏹 🎣 🥊 🥋 🎽 🛹 🛼 ⛸️ " +
                "🎿 🏂 🏋️ 🤸 ⛹️ 🤺 🤾 🏌️ 🏇 🧘 🏄 🏊 🚴 🚵 🎯 🎮 🕹️ 🎲 🧩 🎭 " +
                "🎨 🎬 🎤 🎧 🎼 🎹 🥁 🎷 🎺 🎸 🪕 🎻 🏆 🥇 🥈 🥉 🎉 🎊 🎈 🎁",
        ),
    ),
    EmojiCategory(
        Icons.Filled.DirectionsCar,
        R.string.emoji_cat_travel,
        chars(
            "🚗 🚕 🚙 🚌 🚎 🏎️ 🚓 🚑 🚒 🚐 🚚 🚛 🚜 🛵 🏍️ 🚲 🛴 🚨 🚔 🚍 " +
                "🚄 🚅 🚈 🚂 🚆 🚇 🚊 🚉 ✈️ 🛫 🛬 🛩️ 💺 🚀 🛸 🚁 🛶 ⛵ 🚤 🛥️ " +
                "🗺️ 🧭 🏔️ ⛰️ 🌋 🏕️ 🏖️ 🏝️ 🏠 🏢 🏰 🗼 🗽 ⛩️ 🌃 🌅 🌇 🌉 🎠 🎡",
        ),
    ),
    EmojiCategory(
        Icons.Filled.Lightbulb,
        R.string.emoji_cat_objects,
        chars(
            "⌚ 📱 💻 ⌨️ 🖥️ 🖨️ 🖱️ 💽 💾 💿 📷 📸 📹 🎥 📞 ☎️ 📟 📠 📺 📻 " +
                "🔋 🔌 💡 🔦 🕯️ 🧯 🛢️ 💸 💵 💳 🧾 💎 ⚖️ 🔧 🔨 ⚙️ 🧱 🔗 ⛓️ 🧲 " +
                "🔒 🔓 🔑 🗝️ 🚪 🛏️ 🛋️ 🚽 🚿 🛁 🧹 🧺 🧻 🪣 🧼 🪥 🔬 🔭 📡 💉 " +
                "📌 📎 ✂️ 📏 📐 🖊️ 🖍️ 📝 📖 📚 🗂️ 📅 📈 📉 🔍 🔎",
        ),
    ),
    EmojiCategory(
        Icons.Filled.EmojiSymbols,
        R.string.emoji_cat_symbols,
        chars(
            "❤️ 🧡 💛 💚 💙 💜 🖤 🤍 🤎 💔 ❣️ 💕 💞 💓 💗 💖 💘 💝 ✨ ⭐ " +
                "🌟 💫 ⚡ 🔥 💥 💢 💦 💨 🕐 ⏰ ⏳ ✅ ❌ ⭕ ❗ ❓ 💤 💬 💭 🔔 " +
                "🔕 🚫 ⚠️ ♻️ 🔱 ⚜️ 🔰 ✔️ ➕ ➖ ➗ ✖️ 🟰 💲 🔠 🔡 🔢 🔣 🔤 🆗 " +
                "🆕 🆒 🆓 🔝 🔜 🔙 🔚 🔛 ▶️ ⏸️ ⏹️ ⏺️ ⏭️ ⏮️ 🔀 🔁",
        ),
    ),
)

/**
 * Панель: «Недавние» + сетка текущей категории, снизу — ряд категорий и забой.
 *
 * @param recent последние использованные символы (первым — самый свежий)
 * @param onPick вставить символ в текущую позицию курсора
 * @param onBackspace удалить один символ перед курсором
 */
@Composable
internal fun EmojiPanel(
    recent: List<String>,
    onPick: (String) -> Unit,
    onBackspace: () -> Unit,
    onClose: () -> Unit = {},
) {
    var category by remember { mutableStateOf(0) }
    val gridState = rememberLazyGridState()
    val scope = rememberCoroutineScope()

    Column(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(topStart = 18.dp, topEnd = 18.dp))
            .background(HoloSurface),
    ) {
        // Ручка — тот же приём, что у листа вложений: панель «выехала» из строки ввода.
        //
        // И ручка ОБЯЗАНА работать: она обещает «потяни вниз — закроется». Раньше
        // обещание было пустым — жеста не было вовсе, и панель закрывалась только
        // повторным тапом по кнопке эмодзи. Аффорданс, который ничего не делает,
        // хуже отсутствующего: он учит не доверять интерфейсу.
        var dragged by remember { mutableStateOf(0f) }
        val closeThreshold = with(LocalDensity.current) { 48.dp.toPx() }
        Box(
            Modifier.fillMaxWidth()
                .pointerInput(Unit) {
                    detectVerticalDragGestures(
                        onDragEnd = {
                            if (dragged > closeThreshold) onClose()
                            dragged = 0f
                        },
                        onDragCancel = { dragged = 0f },
                    ) { _, dy -> dragged += dy }
                }
                // Тап по ручке закрывает тоже: до неё уже дотянулся палец, а
                // «потянуть» ради закрытия — лишняя работа.
                .clickable { onClose() }
                .padding(vertical = Tokens.Space.s),
            contentAlignment = Alignment.Center,
        ) {
            Box(Modifier.size(width = 36.dp, height = 4.dp).clip(RoundedCornerShape(2.dp)).background(HoloSecondary))
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

        SectionLabel(stringResource(EmojiSet[category].labelRes))
        LazyVerticalGrid(
            columns = GridCells.Fixed(8),
            state = gridState,
            modifier = Modifier.fillMaxWidth().height(220.dp).padding(horizontal = Tokens.Space.s),
        ) {
            items(EmojiSet[category].items) { e -> EmojiCell(e) { onPick(e) } }
        }

        // Категории снизу: до них дотягивается палец, и они не уезжают при прокрутке.
        Row(
            Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.s, vertical = Tokens.Space.xs),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            EmojiSet.forEachIndexed { i, c ->
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
                        modifier = Modifier.size(20.dp),
                    )
                }
            }
            // Забой: системная клавиатура сейчас скрыта, а эмодзи ставят подряд и промахиваются.
            Box(
                Modifier.size(44.dp).clip(RoundedCornerShape(Tokens.Radius.chip)).clickable { onBackspace() },
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    Icons.AutoMirrored.Filled.Backspace,
                    stringResource(R.string.emoji_backspace),
                    tint = HoloSecondary,
                    modifier = Modifier.size(21.dp),
                )
            }
        }
    }
}

@Composable
private fun SectionLabel(text: String) {
    Text(
        text.uppercase(),
        color = HoloSecondary,
        fontSize = 11.sp,
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
        Text(emoji, fontSize = 25.sp)
    }
}
