@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Bookmark
import androidx.compose.material.icons.filled.Call
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.HoloBg
import dev.p2p.messenger.HoloBorder
import dev.p2p.messenger.HoloSecondary

/** Аккуратное пустое состояние: приглушённая иконка + заголовок + подсказка. */
@Composable
internal fun EmptyState(icon: ImageVector, title: String, hint: String) {
    Box(Modifier.fillMaxSize().background(HoloBg), contentAlignment = Alignment.Center) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            modifier = Modifier.padding(32.dp),
        ) {
            Icon(icon, null, tint = HoloBorder, modifier = Modifier.size(56.dp))
            Spacer(Modifier.height(14.dp))
            Text(title, color = Color.White, fontSize = 16.sp, fontWeight = FontWeight.Medium)
            Spacer(Modifier.height(6.dp))
            Text(hint, color = HoloSecondary, fontSize = 13.sp, textAlign = TextAlign.Center)
        }
    }
}

/** История звонков. Журнал заведём с движковой поддержкой (домейн-данные — в движке). */
@Composable
internal fun CallsScreen() {
    EmptyState(Icons.Filled.Call, "Пока нет звонков", "Журнал звонков появится здесь")
}

/** Сохранённые сообщения («заметки себе»). Появятся с движковым хранилищем. */
@Composable
internal fun SavedScreen() {
    EmptyState(Icons.Filled.Bookmark, "Сохранённое пусто", "Сообщения, которые вы сохраните, появятся здесь")
}
