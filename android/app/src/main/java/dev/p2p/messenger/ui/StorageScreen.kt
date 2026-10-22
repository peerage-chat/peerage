package dev.p2p.messenger.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.HideImage
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.R

/**
 * Хранилище (макет `docs/design/mockups/storage.html`).
 *
 * Вопрос пользователя — «что съело место», поэтому первым идёт состав, а не список.
 * Ничего не удаляется само: очистка только по кнопке, и рядом честно сказано, что
 * скачать заново получится лишь пока файл есть у отправителя.
 */
@Composable
internal fun StorageScreen(model: AppModel, onBackup: () -> Unit = {}) {
    var confirm by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { model.refreshStorage() }

    val total = model.storageBlobBytes
    val kinds = model.storageByKind

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        // Итог крупно: сколько занято приложением.
        Column(
            Modifier.fillMaxWidth().padding(top = Tokens.Space.xl, bottom = Tokens.Space.m),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Text(fmtSizeLong(total), color = Color.White, fontSize = 28.sp, fontWeight = FontWeight.Medium)
            Text(
                stringResource(R.string.storage_total_sub, model.storageBlobs),
                color = HoloSecondary,
                fontSize = 13.sp,
                modifier = Modifier.padding(top = 2.dp),
            )
        }

        // Полоса состава: ответ на «что съело место» должен читаться за секунду.
        if (total > 0 && kinds.isNotEmpty()) {
            Row(
                Modifier.fillMaxWidth().padding(horizontal = ScreenPad).height(10.dp)
                    .clip(RoundedCornerShape(Tokens.Radius.badge)),
            ) {
                kinds.forEach { (kind, bytes) ->
                    Box(
                        Modifier.fillMaxHeight().weight(bytes.toFloat().coerceAtLeast(0.001f))
                            .background(kindColor(kind)),
                    )
                }
            }
            KindLegend(kinds)
        }

        ScreenSectionLabel(stringResource(R.string.storage_by_chat))
        if (model.storageChats.isEmpty()) {
            Text(
                stringResource(R.string.storage_empty),
                color = HoloSecondary,
                fontSize = 13.sp,
                modifier = Modifier.padding(horizontal = ScreenPad, vertical = Tokens.Space.s),
            )
        }
        CardSection {
            model.storageChats.forEachIndexed { i, (peer, files, bytes) ->
                val c = model.contacts.firstOrNull { it.key == peer }
                val name = c?.displayName ?: peer.take(12)
                CardRow(divider = i > 0, inset = 62.dp) {
                    SquareAvatar(
                        name,
                        size = 38.dp,
                        avatarBytes = model.contactAvatars[c?.avatarId],
                        circle = true,
                        ring = Tokens.Colors.surface,
                    )
                    Spacer(Modifier.width(Tokens.Space.m))
                    Column(Modifier.weight(1f)) {
                        Text(name, color = Color.White, fontSize = 15.sp)
                        Text(
                            stringResource(R.string.storage_files, files),
                            color = HoloSecondary,
                            fontSize = 12.sp,
                        )
                    }
                    Text(fmtSizeLong(bytes), color = Tokens.Colors.secondary, fontSize = 13.sp)
                }
            }
        }

        ScreenSectionLabel(stringResource(R.string.storage_free_up))
        CardSection {
            CardRow(onClick = if (model.storageBlobs > 0) ({ confirm = true }) else null, divider = false) {
                Icon(
                    Icons.Filled.HideImage,
                    null,
                    tint = if (model.storageBlobs > 0) HoloAccent else HoloSecondary,
                    modifier = Modifier.size(21.dp),
                )
                Spacer(Modifier.width(Tokens.Space.m))
                Text(
                    stringResource(R.string.storage_clear_attachments),
                    color = if (model.storageBlobs > 0) HoloAccent else HoloSecondary,
                    fontSize = 15.sp,
                    modifier = Modifier.weight(1f),
                )
                Text(fmtSizeLong(total), color = Tokens.Colors.secondary, fontSize = 13.sp)
            }
        }
        Text(
            stringResource(R.string.storage_attachments_hint),
            color = HoloSecondary,
            fontSize = 12.sp,
            modifier = Modifier.padding(horizontal = ScreenPad, vertical = Tokens.Space.xs),
        )

        // Резервная копия ДАННЫХ — не путать с копией КЛЮЧА: ключ возвращает личность,
        // копия данных — переписку. Разделение намеренное.
        //
        // Пока копии не было — янтарь и объяснение цены бездействия. После неё —
        // нейтральный тон и дата: тревожить дальше не за что, а дата отвечает на
        // единственный оставшийся вопрос «насколько она свежая».
        val hasBackup = model.dataBackupAt > 0
        val accentColor = if (hasBackup) HoloOnline else WarnColor
        ScreenSectionLabel(stringResource(R.string.storage_backup))
        Row(
            Modifier.fillMaxWidth().padding(horizontal = ScreenPad, vertical = Tokens.Space.s)
                .clip(RoundedCornerShape(Tokens.Radius.tile))
                .background(accentColor.copy(alpha = 0.12f))
                .clickable { onBackup() }
                .padding(Tokens.Space.l),
        ) {
            Icon(
                if (hasBackup) Icons.Filled.CheckCircle else Icons.Filled.Warning,
                null,
                tint = accentColor,
                modifier = Modifier.size(20.dp),
            )
            Spacer(Modifier.width(Tokens.Space.m))
            Column {
                Text(
                    if (hasBackup) {
                        stringResource(R.string.storage_backup_done, fmtDay(model.dataBackupAt))
                    } else {
                        stringResource(R.string.storage_backup_none)
                    },
                    color = accentColor,
                    fontSize = 13.sp,
                    fontWeight = FontWeight.Medium,
                )
                Text(
                    stringResource(
                        if (hasBackup) R.string.storage_backup_done_hint else R.string.storage_backup_hint,
                    ),
                    color = HoloSecondary,
                    fontSize = 12.sp,
                    modifier = Modifier.padding(top = 3.dp),
                )
                Text(
                    stringResource(R.string.storage_backup_open),
                    color = HoloAccent,
                    fontSize = 13.sp,
                    modifier = Modifier.padding(top = 8.dp),
                )
            }
        }
        Spacer(Modifier.height(Tokens.Space.l))
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

/** Легенда состава: цвет + подпись + вес. */
@Composable
private fun KindLegend(kinds: List<Pair<String, Long>>) {
    Column(Modifier.padding(horizontal = ScreenPad, vertical = Tokens.Space.s)) {
        kinds.forEach { (kind, bytes) ->
            Row(
                Modifier.fillMaxWidth().padding(vertical = 2.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(Modifier.size(9.dp).clip(RoundedCornerShape(3.dp)).background(kindColor(kind)))
                Spacer(Modifier.width(Tokens.Space.s))
                Text(kindLabel(kind), color = HoloSecondary, fontSize = 12.sp, modifier = Modifier.weight(1f))
                Text(fmtSizeLong(bytes), color = HoloSecondary, fontSize = 12.sp)
            }
        }
    }
}

/** Дата «1 августа 2026» — человеку важен возраст копии, а не время суток. */
private fun fmtDay(ts: Long): String =
    java.text.SimpleDateFormat("d MMMM yyyy", java.util.Locale.getDefault())
        .format(java.util.Date(ts * 1000))

private fun kindColor(kind: String): Color = when (kind) {
    "image" -> Tokens.Colors.accent
    "video" -> Tokens.Colors.kindVideo
    "voice" -> Tokens.Colors.online
    else -> Tokens.Colors.iconMuted
}

@Composable
private fun kindLabel(kind: String): String = stringResource(
    when (kind) {
        "image" -> R.string.storage_kind_image
        "video" -> R.string.storage_kind_video
        "voice" -> R.string.storage_kind_voice
        else -> R.string.storage_kind_file
    },
)
