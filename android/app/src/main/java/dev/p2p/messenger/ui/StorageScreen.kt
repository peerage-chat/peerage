package dev.p2p.messenger.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.R

/** Что занимает место: вложения и очередь недоставленного.
 *
 *  СОЗНАТЕЛЬНО простой (нормальный дизайн — позже): цифры + одно действие. Важное
 *  правило экрана — ничего не удаляется само; очистка только по кнопке, и рядом
 *  честно написано, что вложения потом можно скачать заново. */
@Composable
internal fun StorageScreen(model: AppModel) {
    var confirm by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { model.refreshStorage() }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(ScreenPad),
        verticalArrangement = Arrangement.spacedBy(4.dp),
    ) {
        FieldLabel(stringResource(R.string.storage_attachments))
        StorageRow(
            stringResource(R.string.storage_files, model.storageBlobs),
            formatBytes(model.storageBlobBytes),
        )
        Spacer(Modifier.height(8.dp))
        Text(
            stringResource(R.string.storage_attachments_hint),
            color = HoloSecondary,
            fontSize = 11.sp,
        )

        Spacer(Modifier.height(20.dp))
        FieldLabel(stringResource(R.string.storage_queue))
        StorageRow(
            stringResource(R.string.storage_queue_pending, model.storageOutboxPending),
            "",
        )
        Spacer(Modifier.height(8.dp))
        Text(
            stringResource(R.string.storage_queue_hint),
            color = HoloSecondary,
            fontSize = 11.sp,
        )

        Spacer(Modifier.height(24.dp))
        if (model.storageBlobs > 0) {
            AccentButton(stringResource(R.string.storage_clear_attachments)) { confirm = true }
        }
    }

    if (confirm) {
        AlertDialog(
            onDismissRequest = { confirm = false },
            title = { Text(stringResource(R.string.storage_clear_attachments)) },
            text = { Text(stringResource(R.string.storage_clear_confirm)) },
            confirmButton = {
                TextButton(onClick = {
                    model.clearAttachments()
                    confirm = false
                }) { Text(stringResource(R.string.storage_clear_ok), color = HoloError) }
            },
            dismissButton = {
                TextButton(onClick = { confirm = false }) {
                    Text(stringResource(R.string.common_cancel), color = HoloSecondary)
                }
            },
        )
    }
}

@Composable
private fun StorageRow(label: String, value: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = Color.White, fontSize = 14.sp, modifier = Modifier.weight(1f))
        if (value.isNotEmpty()) Text(value, color = HoloAccent, fontSize = 14.sp)
    }
}

/** Байты в человеческий вид (КБ/МБ) — без внешних зависимостей. */
private fun formatBytes(b: Long): String = when {
    b >= 1024L * 1024 -> "%.1f МБ".format(b / (1024.0 * 1024))
    b >= 1024 -> "%.0f КБ".format(b / 1024.0)
    else -> "$b Б"
}
