@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package chat.peerage.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import chat.peerage.HoloBorder
import chat.peerage.HoloSecondary
import chat.peerage.Tokens

/** Аккуратное пустое состояние: приглушённая иконка + заголовок + подсказка. */
@Composable
internal fun EmptyState(icon: ImageVector, title: String, hint: String) {
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            modifier = Modifier.padding(Tokens.Space.xxxl),
        ) {
            Icon(icon, null, tint = HoloBorder, modifier = Modifier.size(Tokens.Size.iconHero))
            Spacer(Modifier.height(Tokens.Space.m))
            Text(title, color = Color.White, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium)
            Spacer(Modifier.height(Tokens.Space.s))
            Text(hint, color = HoloSecondary, fontSize = Tokens.Font.body, textAlign = TextAlign.Center)
        }
    }
}
