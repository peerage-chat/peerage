@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.R
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

/** Индикатор пиров меша в шапке Главной (⊹ N). Тап → «Сеть» (TODO),
 *  долгий тап — диагностический бенч приёма. */
@Composable
internal fun PeerIndicator(peers: Int, onLongPress: () -> Unit = {}) {
    Row(
        Modifier.clip(RoundedCornerShape(8.dp))
            .pointerInput(Unit) {
                detectTapGestures(
                    onTap = { /* TODO: экран Сеть */ },
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
    content: (@Composable () -> Unit)? = null,
) {
    Box(Modifier.size(size), contentAlignment = Alignment.BottomEnd) {
        Box(
            // «квадрат со скруглением» (app-ui.md ~14-15px): пропорция size×0.3, но с кэпом
            // на Tokens.Radius.avatar — иначе крупные аватарки «балонят» до почти-круга.
            Modifier.size(size).clip(RoundedCornerShape(minOf(size * 0.3f, Tokens.Radius.avatar)))
                .background(bg ?: avatarColor(name)),
            contentAlignment = Alignment.Center,
        ) {
            if (content != null) {
                content()
            } else {
                Text(
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
