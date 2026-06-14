package dev.p2p.messenger

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color

// Holo (Android 4.x): тёмный фон, синий акцент.
val HoloBg = Color(0xFF1A1A1A)
val HoloSurface = Color(0xFF222222)
val HoloBorder = Color(0xFF333333)
val HoloAccent = Color(0xFF33B5E5)
val HoloAccentDark = Color(0xFF04222E)
val HoloSecondary = Color(0xFF888888)
val HoloOnline = Color(0xFF8BC34A)
val HoloVerified = Color(0xFF4CAF50)
val HoloError = Color(0xFFE53935)

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
