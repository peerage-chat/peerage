package chat.peerage

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

// Holo (Android 4.x): тёмный фон, синий акцент.
// Значения — в Tokens (единый источник истины); тут семантические псевдонимы.
val HoloBg = Tokens.Colors.bg
val HoloSurface = Tokens.Colors.surface
val HoloBorder = Tokens.Colors.border
val HoloAccent = Tokens.Colors.accent
val HoloAccentDark = Tokens.Colors.accentDark
val HoloSecondary = Tokens.Colors.secondary
val HoloOnline = Tokens.Colors.online
val HoloVerified = Tokens.Colors.verified
val HoloError = Tokens.Colors.error
val HoloWarn = Tokens.Colors.warn
val HoloIconMuted = Tokens.Colors.iconMuted

private val Scheme = darkColorScheme(
    primary = HoloAccent,
    onPrimary = HoloAccentDark,
    background = HoloBg,
    onBackground = Color.White,
    surface = HoloSurface,
    onSurface = Color.White,
    surfaceVariant = HoloSurface,
    error = HoloError,
)

// Все слоты Material (Text по умолчанию, кнопки, диалоги, меню) — на нашей основе:
// пропорциональный интервал и без разрядки. Размер у слотов остаётся свой, но в наших
// экранах его всё равно задаёт Tokens.Font.
private val AppTypography = Typography().run {
    val b = Tokens.Text.base
    Typography(
        displayLarge = displayLarge.merge(b.copy(fontSize = displayLarge.fontSize)),
        displayMedium = displayMedium.merge(b.copy(fontSize = displayMedium.fontSize)),
        displaySmall = displaySmall.merge(b.copy(fontSize = displaySmall.fontSize)),
        headlineLarge = headlineLarge.merge(b.copy(fontSize = headlineLarge.fontSize)),
        headlineMedium = headlineMedium.merge(b.copy(fontSize = headlineMedium.fontSize)),
        headlineSmall = headlineSmall.merge(b.copy(fontSize = Tokens.Font.title)),
        titleLarge = titleLarge.merge(b.copy(fontSize = Tokens.Font.title)),
        titleMedium = titleMedium.merge(b.copy(fontSize = Tokens.Font.label)),
        titleSmall = titleSmall.merge(b.copy(fontSize = Tokens.Font.body)),
        bodyLarge = bodyLarge.merge(b.copy(fontSize = Tokens.Font.label)),
        bodyMedium = bodyMedium.merge(b.copy(fontSize = Tokens.Font.label)),
        bodySmall = bodySmall.merge(b.copy(fontSize = Tokens.Font.small)),
        labelLarge = labelLarge.merge(b.copy(fontSize = Tokens.Font.label)),
        labelMedium = labelMedium.merge(b.copy(fontSize = Tokens.Font.small)),
        labelSmall = labelSmall.merge(b.copy(fontSize = Tokens.Font.caption)),
    )
}

@Composable
fun AppTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = Scheme, typography = AppTypography, content = content)
}

// ── Сетка размеров/отступов и семантические цвета — псевдонимы из Tokens ─────
internal val ScreenPad = Tokens.Space.screen
internal val FormGap = Tokens.Space.formGap
internal val RowPadV = Tokens.Space.rowV
internal val OfflineDot = Tokens.Colors.offlineDot
internal val WarnColor = Tokens.Colors.warn
internal val BubbleIn = Tokens.Colors.bubbleIn // входящий пузырь (нейтральный)
internal val BubbleOut = Tokens.Colors.bubbleOut // исходящий пузырь (приглушённый акцент)
internal val FolderSelected = Tokens.Colors.folderSelected // фон активной папки (приглушённый)
