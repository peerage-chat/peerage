@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.HoloSurface
import dev.p2p.messenger.R
import kotlinx.coroutines.launch
import kotlin.math.abs

// ── Общие элементы ──────────────────────────────────────────────────────────

/** Иконка-кнопка топ-бара: единый размер (48dp касание / 24dp иконка). */
@Composable
internal fun BarIcon(icon: ImageVector, desc: String, tint: Color = HoloAccent, onClick: () -> Unit) {
    IconButton(onClick = onClick) {
        Icon(icon, contentDescription = desc, tint = tint, modifier = Modifier.size(24.dp))
    }
}

@Composable
internal fun TopBar(
    title: String,
    onBack: (() -> Unit)? = null,
    onMenu: (() -> Unit)? = null,
    actions: @Composable RowScope.() -> Unit = {},
) {
    Row(
        Modifier.fillMaxWidth().background(HoloAccentDark).height(56.dp).padding(horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        when {
            onMenu != null -> BarIcon(TablerIcons.Menu2, stringResource(R.string.common_menu), onClick = onMenu)
            onBack != null -> BarIcon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.common_back), onClick = onBack)
            else -> Spacer(Modifier.width(12.dp))
        }
        Text(
            title,
            color = Color.White,
            fontSize = 19.sp,
            fontWeight = FontWeight.Medium,
            modifier = Modifier.weight(1f),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        actions()
    }
}

/** Индикатор присутствия: маленький кружок (онлайн/офлайн). */
@Composable
internal fun StatusDot(online: Boolean) {
    Box(Modifier.size(10.dp).clip(RoundedCornerShape(percent = 30)).background(if (online) HoloOnline else OfflineDot))
}

/** Индикатор пиров меша в шапке Главной (⊹ N). Тап → «Сеть»,
 *  долгий тап — диагностический бенч приёма. */
@Composable
internal fun PeerIndicator(peers: Int, onTap: () -> Unit = {}, onLongPress: () -> Unit = {}) {
    Row(
        Modifier.clip(RoundedCornerShape(8.dp))
            .pointerInput(Unit) {
                detectTapGestures(
                    onTap = { onTap() },
                    onLongPress = { onLongPress() },
                )
            }
            .padding(horizontal = 8.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        Icon(
            TablerIcons.Affiliate,
            stringResource(R.string.menu_network),
            tint = if (peers > 0) HoloOnline else HoloSecondary,
            modifier = Modifier.size(18.dp),
        )
        Text("$peers", color = Color.White, fontSize = 13.sp, fontWeight = FontWeight.Medium)
    }
}

/** Квадратная аватарка со скруглением — единый визуальный язык приложения
 *  (НЕ круг). Буква имени на детерминированном цвете; опционально presence-точка
 *  в правом-нижнем углу; [bg]/[content] переопределяют фон и содержимое (комнаты). */
@Composable
internal fun SquareAvatar(
    name: String,
    size: Dp = 44.dp,
    online: Boolean? = null,
    bg: Color? = null,
    avatarBytes: ByteArray? = null,
    content: (@Composable () -> Unit)? = null,
) {
    // Декод байт аватара (M2) — один раз на набор байт, не на каждую рекомпозицию.
    val bmp = avatarBytes?.let {
        remember(it) { runCatching { android.graphics.BitmapFactory.decodeByteArray(it, 0, it.size)?.asImageBitmap() }.getOrNull() }
    }
    Box(Modifier.size(size), contentAlignment = Alignment.BottomEnd) {
        Box(
            // «квадрат со скруглением» (app-ui.md ~14-15px): пропорция size×0.3, но с кэпом
            // на Tokens.Radius.avatar — иначе крупные аватарки «балонят» до почти-круга.
            Modifier.size(size).clip(RoundedCornerShape(minOf(size * 0.3f, Tokens.Radius.avatar)))
                .background(bg ?: avatarColor(name)),
            contentAlignment = Alignment.Center,
        ) {
            when {
                bmp != null -> Image(bmp, null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
                content != null -> content()
                else -> Text(
                    name.take(1).uppercase(),
                    color = Color.White,
                    fontSize = (size.value * 0.4f).sp,
                    fontWeight = FontWeight.Medium,
                )
            }
        }
        if (online != null) {
            Box(
                Modifier.size(size * 0.28f).clip(RoundedCornerShape(percent = 30))
                    .background(if (online) HoloOnline else OfflineDot)
                    .border(2.dp, HoloBg, RoundedCornerShape(percent = 30)),
            )
        }
    }
}

@Composable
internal fun ListRow(onClick: () -> Unit, content: @Composable RowScope.() -> Unit) {
    // Без разделителей — мягкий «телеграмный» список (по макету).
    Row(
        Modifier.fillMaxWidth().clickable { onClick() }.padding(horizontal = ScreenPad, vertical = RowPadV),
        verticalAlignment = Alignment.CenterVertically,
        content = content,
    )
}

@Composable
internal fun FieldLabel(text: String) {
    Text(text, color = HoloSecondary, fontSize = 11.sp, fontWeight = FontWeight.Medium)
}

@Composable
internal fun AccentButton(text: String, modifier: Modifier = Modifier, onClick: () -> Unit) {
    Button(
        onClick = onClick,
        modifier = modifier,
        colors = ButtonDefaults.buttonColors(containerColor = HoloAccent),
    ) {
        Text(text, color = HoloAccentDark, fontWeight = FontWeight.Medium)
    }
}

@Composable
internal fun SendButton(onClick: () -> Unit) {
    Box(
        Modifier.clip(RoundedCornerShape(percent = 30)).background(HoloAccent).size(44.dp).clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) { Icon(Icons.AutoMirrored.Filled.Send, stringResource(R.string.common_send), tint = HoloAccentDark, modifier = Modifier.size(20.dp)) }
}

// ── Экраны ──────────────────────────────────────────────────────────────────

/** Время сообщения «ЧЧ:ММ» (time — unix-секунды). */
internal fun fmtMsgTime(ts: Long): String =
    java.text.SimpleDateFormat("HH:mm", java.util.Locale.getDefault()).format(java.util.Date(ts * 1000))

/** Ключ дня для сравнения «сменился ли день» между соседними сообщениями
 *  (не-Composable: используется в условии группировки). */
internal fun dayKey(ts: Long): Long {
    val c = java.util.Calendar.getInstance().apply { timeInMillis = ts * 1000 }
    return c.get(java.util.Calendar.YEAR) * 1000L + c.get(java.util.Calendar.DAY_OF_YEAR)
}

// ── Склейка сообщений в блоки (docs/design/mockups/chat-blocks.html) ────────
// Блок — подряд идущие сообщения ОДНОГО отправителя внутри окна времени, без
// разделителей между ними. Внутри блока сообщения стоят вплотную и «сшиваются»
// уменьшенным радиусом со стороны отправителя.

/** Окно склейки, секунды. Часы устройств не синхронизированы (время ставит
 *  отправитель), поэтому окно нарочно широкое: узкое рвало бы блок на расхождении. */
internal const val BLOCK_WINDOW_SEC = 300L

/** Положение сообщения в блоке. */
internal enum class BlockPos { Solo, First, Mid, Last }

/** Продолжает ли сообщение блок предыдущего: тот же отправитель, тот же день,
 *  разрыв меньше окна. Системные строки блок разрывают (передавайте `null`). */
internal fun continuesBlock(prevSender: String?, prevTime: Long, sender: String, time: Long): Boolean =
    prevSender == sender && time - prevTime < BLOCK_WINDOW_SEC && dayKey(prevTime) == dayKey(time)

internal fun blockPos(withPrev: Boolean, withNext: Boolean): BlockPos = when {
    !withPrev && !withNext -> BlockPos.Solo
    !withPrev -> BlockPos.First
    withNext -> BlockPos.Mid
    else -> BlockPos.Last
}

/** Отступ ПОД сообщением: внутри блока — хайрлайн, между блоками — обычный шаг. */
internal fun blockGap(pos: BlockPos) =
    if (pos == BlockPos.First || pos == BlockPos.Mid) Tokens.Space.xxs else Tokens.Space.s

/** Резерв места под время в правом-нижнем углу пузыря: время не участвует в потоке
 *  текста, поэтому текст обязан оставить ему ширину, иначе они наложатся.
 *  У своих сообщений шире — там ещё галочка статуса. */
internal fun bubbleTimeReserve(mine: Boolean, edited: Boolean = false) =
    (if (mine) 52.dp else 34.dp) + if (edited) 34.dp else 0.dp

/** Форма пузыря: «шов» внутри блока — со стороны отправителя. */
internal fun bubbleShape(mine: Boolean, pos: BlockPos): RoundedCornerShape {
    val r = Tokens.Radius.preview // 14 — свободные углы
    val seam = 5.dp // сшитые углы внутри блока
    val topSeam = pos == BlockPos.Mid || pos == BlockPos.Last
    val bottomSeam = pos == BlockPos.Mid || pos == BlockPos.First
    return if (mine) {
        RoundedCornerShape(r, if (topSeam) seam else r, if (bottomSeam) seam else r, r)
    } else {
        RoundedCornerShape(if (topSeam) seam else r, r, r, if (bottomSeam) seam else r)
    }
}

/** Локализованная метка дня для чипа-разделителя: Сегодня / Вчера / «d MMMM». */
@Composable
internal fun dayLabel(ts: Long): String {
    val nowSec = System.currentTimeMillis() / 1000
    return when (dayKey(ts)) {
        dayKey(nowSec) -> stringResource(R.string.date_today)
        dayKey(nowSec - 86_400) -> stringResource(R.string.date_yesterday)
        else -> java.text.SimpleDateFormat("d MMMM", java.util.Locale.getDefault()).format(java.util.Date(ts * 1000))
    }
}

/** Детерминированный цвет аватара по имени контакта. */
internal fun avatarColor(name: String): Color {
    val palette = listOf(
        Color(0xFF1E3A5A),
        Color(0xFF2A1A3A),
        Color(0xFF1A2A1A),
        Color(0xFF2A2A1A),
        Color(0xFF3A1A1A),
        Color(0xFF1A3A3A),
    )
    return palette[abs(name.hashCode()) % palette.size]
}

/** Генерирует QR-код для строки [data] размером 512×512. Возвращает null при ошибке. */
internal fun generateQrBitmap(data: String): android.graphics.Bitmap? = runCatching {
    val size = 512
    val bits = com.google.zxing.qrcode.QRCodeWriter()
        .encode(data, com.google.zxing.BarcodeFormat.QR_CODE, size, size)
    val pixels = IntArray(size * size) { i ->
        if (bits[i % size, i / size]) android.graphics.Color.BLACK else android.graphics.Color.WHITE
    }
    android.graphics.Bitmap.createBitmap(size, size, android.graphics.Bitmap.Config.ARGB_8888)
        .also { it.setPixels(pixels, 0, size, 0, 0, size, size) }
}.getOrNull()

@Composable
internal fun inputColors() = TextFieldDefaults.colors(
    focusedContainerColor = HoloBg,
    unfocusedContainerColor = HoloBg,
    focusedTextColor = Color.White,
    unfocusedTextColor = Color.White,
    cursorColor = HoloAccent,
    focusedIndicatorColor = HoloAccent,
    unfocusedIndicatorColor = HoloBorder,
)

/**
 * Догрузка истории ВВЕРХ по мере прокрутки: чат открывается хвостом (движок не
 * поднимает переписку целиком), остальное приезжает страницами.
 *
 * Две тонкости, обе видны только на живом экране:
 * - после вставки сверху возвращаем взгляд на то же сообщение, иначе лента дёргается;
 * - если история короче экрана, прокрутки нет вовсе и по одному лишь индексу вторая
 *   страница не запросилась бы никогда — поэтому тянем, пока лента не станет
 *   прокручиваемой или страницы не кончатся.
 */
@Composable
internal fun PagingUp(listState: LazyListState, key: Any?, load: (done: (Int) -> Unit) -> Unit) {
    val scope = rememberCoroutineScope()
    fun pull() {
        val idx = listState.firstVisibleItemIndex
        val offset = listState.firstVisibleItemScrollOffset
        load { added ->
            if (added > 0) {
                scope.launch {
                    listState.scrollToItem(idx + added, offset)
                    if (!listState.canScrollBackward) pull()
                }
            }
        }
    }
    LaunchedEffect(key) {
        // Следим и за числом элементов: история приезжает асинхронно, и в момент
        // подписки список ещё пуст — по одному лишь индексу (он всё это время 0)
        // первая страница не запросилась бы никогда.
        snapshotFlow { listState.firstVisibleItemIndex to listState.layoutInfo.totalItemsCount }
            .collect { (idx, count) -> if (idx <= 3 && count > 0) pull() }
    }
}

/**
 * Плашка закреплённого сообщения над лентой. Показывает ВЕРХНЕЕ (самое свежее) и
 * счётчик остальных: закреп — свойство чата, поэтому одинаков у всех участников.
 *
 * Кнопка «открепить» видна тогда же, когда доступно само действие: в личном чате —
 * всегда, в группе — только админу (иначе она обещала бы то, что движок отклонит).
 */
@Composable
internal fun PinnedBar(model: AppModel, chat: String, group: Boolean, canUnpin: Boolean) {
    val list = model.pinned[chat].orEmpty()
    val top = list.firstOrNull() ?: return
    Row(
        Modifier.fillMaxWidth().background(HoloSurface).padding(horizontal = 12.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(TablerIcons.Pin, null, tint = HoloAccent, modifier = Modifier.size(18.dp))
        Spacer(Modifier.width(10.dp))
        Column(Modifier.weight(1f)) {
            Text(
                if (list.size > 1) {
                    stringResource(R.string.pin_title_many, list.size)
                } else {
                    stringResource(R.string.pin_title)
                },
                color = HoloAccent,
                fontSize = 11.sp,
            )
            Text(
                top.text.ifEmpty { stringResource(R.string.pin_attachment) },
                color = Color.White,
                fontSize = 13.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        if (canUnpin) {
            BarIcon(Icons.Filled.Close, stringResource(R.string.pin_remove)) {
                model.setPinned(chat, group, top, false)
            }
        }
    }
}

/**
 * Полоска «Редактирование» над полем ввода: показывает, что уходит правка, а не
 * новое сообщение, и даёт из неё выйти. Занимает то же место, что цитата ответа —
 * два режима ввода одновременно не бывают.
 */
@Composable
internal fun EditStrip(text: String, onCancel: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(Icons.Filled.Edit, null, tint = HoloAccent, modifier = Modifier.size(16.dp))
        Spacer(Modifier.width(8.dp))
        Column(Modifier.weight(1f)) {
            Text(
                stringResource(R.string.chat_editing),
                color = HoloAccent,
                fontSize = 12.sp,
                fontWeight = FontWeight.Medium,
            )
            Text(
                text,
                color = HoloSecondary,
                fontSize = 12.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        BarIcon(Icons.Filled.Close, stringResource(R.string.common_cancel), onClick = onCancel)
    }
}
