package dev.p2p.messenger

import androidx.compose.material3.MaterialTheme
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

@Composable
fun P2PTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = Scheme, content = content)
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
