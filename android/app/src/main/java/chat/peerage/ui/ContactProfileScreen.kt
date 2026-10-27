package chat.peerage.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Block
import androidx.compose.material.icons.filled.Call
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.DeleteSweep
import androidx.compose.material.icons.filled.ExpandLess
import androidx.compose.material.icons.filled.ExpandMore
import androidx.compose.material.icons.filled.Message
import androidx.compose.material.icons.filled.Notifications
import androidx.compose.material.icons.filled.NotificationsOff
import androidx.compose.material.icons.filled.Photo
import androidx.compose.material.icons.filled.Share
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R

/**
 * Профиль контакта (макет `docs/design/mockups/contact-profile.html`).
 *
 * Это КАРТОЧКА для чтения и сверки, а не форма: переименование и удаление живут в
 * меню, диагностика — в «Сети». Главное на экране — карточка верификации: набор
 * знаков, который сверяют глазами с экраном собеседника.
 */
@Composable
internal fun ContactProfileScreen(
    model: AppModel,
    onMessage: () -> Unit,
    onScanQr: () -> Unit,
    onDeleted: () -> Unit,
) {
    val key = model.currentChat ?: return
    val c = model.contacts.firstOrNull { it.key == key } ?: return
    val ctx = LocalContext.current
    val online = model.online[key] == true
    var renaming by remember(key) { mutableStateOf(false) }
    var confirmDelete by remember(key) { mutableStateOf(false) }
    var confirmBlock by remember(key) { mutableStateOf(false) }
    var confirmShare by remember(key) { mutableStateOf(false) }
    var confirmClear by remember(key) { mutableStateOf(false) }
    // Флаг чата приходит из сводки — она же кормит список на главной, так что
    // «без звука» здесь и значок на главной не разъедутся.
    val muted = model.chatSummaries[model.chatKey(key, false)]?.muted == true

    // Сколько вложений от этого чата лежит на диске — цифра уже посчитана движком
    // для «Хранилища», второй раз считать незачем.
    LaunchedEffect(key) {
        model.refreshStorage()
        model.refreshChatSummaries()
    }
    val files = model.storageChats.firstOrNull { it.first == key }?.second ?: 0L
    val commonGroups = model.groupMemberKeys.count { (_, members) -> key in members }

    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
        // ── Шапка ────────────────────────────────────────────────────────────
        Column(
            Modifier.fillMaxWidth().padding(top = Tokens.Space.xxl, bottom = Tokens.Space.l),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            SquareAvatar(
                c.displayName,
                size = Tokens.Size.avatarBig,
                online = online,
                avatarBytes = model.contactAvatars[c.avatarId],
                circle = true, // собеседник — человек
                ring = HoloBg,
            )
            Row(
                Modifier.padding(top = Tokens.Space.m),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(c.displayName, color = Color.White, fontSize = Tokens.Font.title, fontWeight = FontWeight.Medium)
                if (c.verified) {
                    Spacer(Modifier.width(6.dp))
                    Icon(Icons.Filled.Verified, null, tint = HoloVerified, modifier = Modifier.size(19.dp))
                }
            }
            Text(
                stringResource(if (online) R.string.status_online else R.string.status_offline),
                color = if (online) HoloOnline else HoloSecondary,
                fontSize = Tokens.Font.body,
            )
        }

        // Ключ — строкой в карточке, сокращённо и в одну строку. Целиком он занимал
        // две строки моноширинного текста прямо под именем и читался как ошибка
        // вёрстки. Сверяют собеседника всё равно не по нему, а по отпечатку ниже
        // (знаки + слова); ключ нужен, чтобы его СКОПИРОВАТЬ — тап это и делает.
        CardSection {
            CardRow(onClick = { copyToClipboard(ctx, c.key) }, divider = false, inset = 0.dp) {
                Column(Modifier.weight(1f)) {
                    FieldLabel(stringResource(R.string.me_pubkey_label))
                    Text(
                        c.key.take(10) + "…" + c.key.takeLast(6),
                        color = Color.White,
                        fontSize = Tokens.Font.small,
                        fontFamily = FontFamily.Monospace,
                        maxLines = 1,
                    )
                }
                Icon(
                    Icons.Filled.ContentCopy,
                    stringResource(R.string.me_copy_key),
                    tint = HoloAccent,
                    modifier = Modifier.size(Tokens.Size.iconS),
                )
            }
        }

        Spacer(Modifier.height(Tokens.Space.s))

        // ── Действия. Кнопки «Видео» нет: камеру включают уже внутри звонка. ──
        Row(
            Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.s),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
        ) {
            ActionTile(Icons.Filled.Message, R.string.profile_message, Modifier.weight(1f), onMessage)
            ActionTile(Icons.Filled.Call, R.string.profile_call, Modifier.weight(1f)) {
                model.callDm(key)
            }
        }

        // ── Карточка верификации ─────────────────────────────────────────────
        // Свёрнута по умолчанию: развёрнутая сверка (предупреждение + сетка знаков +
        // слова + две кнопки) занимала пол-экрана и отодвигала всё остальное вниз,
        // хотя сверяются один раз за знакомство. Статус при этом виден всегда —
        // именно он и есть то, ради чего сюда заходят.
        Spacer(Modifier.height(Tokens.Space.m))
        var verifyOpen by remember(key) { mutableStateOf(false) }
        CardSection {
            CardRow(onClick = { verifyOpen = !verifyOpen }, divider = false, inset = 0.dp) {
                Icon(
                    if (c.verified) Icons.Filled.Verified else Icons.Filled.Block,
                    null,
                    tint = if (c.verified) HoloVerified else WarnColor,
                    modifier = Modifier.size(20.dp),
                )
                Spacer(Modifier.width(Tokens.Space.s))
                Column(Modifier.weight(1f)) {
                    Text(
                        stringResource(if (c.verified) R.string.info_verified else R.string.info_not_verified),
                        color = if (c.verified) HoloVerified else WarnColor,
                        fontSize = Tokens.Font.label,
                        fontWeight = FontWeight.Medium,
                    )
                    Text(
                        stringResource(
                            if (c.verified) R.string.profile_verify_done else R.string.profile_verify_todo,
                        ),
                        color = HoloSecondary,
                        fontSize = Tokens.Font.small,
                        modifier = Modifier.padding(top = 2.dp),
                    )
                }
                Icon(
                    if (verifyOpen) Icons.Filled.ExpandLess else Icons.Filled.ExpandMore,
                    null,
                    tint = Tokens.Colors.secondary,
                    modifier = Modifier.size(Tokens.Size.iconM),
                )
            }
            AnimatedVisibility(verifyOpen) {
                Column(
                    Modifier.padding(
                        start = Tokens.Space.l,
                        end = Tokens.Space.l,
                        bottom = Tokens.Space.l,
                    ),
                ) {
                    Text(
                        stringResource(
                            if (c.verified) R.string.profile_verified_hint else R.string.profile_unverified_hint,
                            c.displayName,
                        ),
                        color = HoloSecondary,
                        fontSize = Tokens.Font.small,
                    )
                    FingerprintGrid(model.visualFingerprint(key), Modifier.padding(top = Tokens.Space.l))
                    // Слова — тот же отпечаток, но их удобно диктовать голосом.
                    Text(
                        c.fingerprint,
                        color = HoloAccent,
                        fontSize = Tokens.Font.small,
                        modifier = Modifier.padding(top = Tokens.Space.m),
                    )
                    Spacer(Modifier.height(Tokens.Space.l))
                    Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s)) {
                        AccentButton(
                            stringResource(if (c.verified) R.string.profile_recheck else R.string.profile_scan_qr),
                            Modifier.weight(1f),
                        ) { onScanQr() }
                        OutlineButton(
                            stringResource(if (c.verified) R.string.info_unmark else R.string.info_mark),
                            Modifier.weight(1f),
                            tint = if (c.verified) Tokens.Colors.secondary else HoloAccent,
                        ) { model.toggleVerified(key) }
                    }
                }
            }
        }

        // ── Общее ────────────────────────────────────────────────────────────
        ScreenSectionLabel(stringResource(R.string.profile_common))
        CardSection {
            InfoRow(TablerIcons.Users, stringResource(R.string.profile_common_groups), "$commonGroups", divider = false)
            InfoRow(Icons.Filled.Photo, stringResource(R.string.profile_attachments), "$files")
        }

        // ── Опасное — внизу и с подтверждением ───────────────────────────────
        ScreenSectionLabel(stringResource(R.string.profile_manage))
        CardSection {
            DangerRow(TablerIcons.Pencil, stringResource(R.string.profile_rename), HoloAccent, divider = false) {
                renaming = true
            }
            // «Без звука» — локальная пометка чата: собеседник о ней не узнаёт,
            // поэтому она и живёт здесь, а не в общих настройках уведомлений.
            CardRow {
                Icon(
                    if (muted) Icons.Filled.NotificationsOff else Icons.Filled.Notifications,
                    null,
                    tint = Tokens.Colors.secondary,
                    modifier = Modifier.size(Tokens.Size.iconM),
                )
                Spacer(Modifier.width(Tokens.Space.m))
                Text(
                    stringResource(R.string.profile_mute),
                    color = Color.White,
                    fontSize = Tokens.Font.label,
                    modifier = Modifier.weight(1f),
                )
                Switch(checked = muted, onCheckedChange = { model.muteChat(key, false, it) })
            }
            DangerRow(Icons.Filled.Share, stringResource(R.string.profile_share), HoloAccent) {
                confirmShare = true
            }
        }
        // Блокировка и удаление — в СВОЕЙ карточке: их отделяет от остального не
        // заголовок, а разрыв, иначе «удалить» стоит вплотную к «переименовать».
        CardSection {
            DangerRow(Icons.Filled.DeleteSweep, stringResource(R.string.profile_clear_history), HoloError, divider = false) {
                confirmClear = true
            }
            DangerRow(Icons.Filled.Block, stringResource(R.string.profile_block), HoloError) {
                confirmBlock = true
            }
            DangerRow(Icons.Filled.Delete, stringResource(R.string.info_delete), HoloError) {
                confirmDelete = true
            }
        }
        Spacer(Modifier.height(Tokens.Space.l))
    }

    if (renaming) RenameDialog(model, key, c.displayName) { renaming = false }

    if (confirmBlock) {
        AlertDialog(
            onDismissRequest = { confirmBlock = false },
            title = { Text(stringResource(R.string.profile_block)) },
            text = { Text(stringResource(R.string.profile_block_confirm, c.displayName)) },
            confirmButton = {
                TextButton(onClick = {
                    model.blockRequest(key)
                    confirmBlock = false
                }) { Text(stringResource(R.string.profile_block), color = HoloError) }
            },
            dismissButton = {
                TextButton(onClick = { confirmBlock = false }) {
                    Text(stringResource(R.string.common_cancel), color = HoloSecondary)
                }
            },
        )
    }

    // Делимся ЧИСТЫМ ключом, без имени и пояснений: получатель вставляет строку
    // в «Добавить контакт» как есть. Имя ушло в заголовок системного листа.
    //
    // Спрашиваем, потому что раздаётся чужая личность: после этого третий человек
    // сможет постучаться к собеседнику, и отозвать это уже нельзя.
    if (confirmShare) {
        AlertDialog(
            onDismissRequest = { confirmShare = false },
            title = { Text(stringResource(R.string.profile_share)) },
            text = { Text(stringResource(R.string.profile_share_confirm, c.displayName)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmShare = false
                    shareText(ctx, c.key)
                }) { Text(stringResource(R.string.profile_share_ok), color = HoloAccent) }
            },
            dismissButton = {
                TextButton(onClick = { confirmShare = false }) {
                    Text(stringResource(R.string.common_cancel), color = HoloSecondary)
                }
            },
        )
    }

    // Стирается только У СЕБЯ — это и сказано в подтверждении: человек должен знать,
    // что у собеседника переписка останется.
    if (confirmClear) {
        AlertDialog(
            onDismissRequest = { confirmClear = false },
            title = { Text(stringResource(R.string.profile_clear_history)) },
            text = { Text(stringResource(R.string.profile_clear_history_confirm)) },
            confirmButton = {
                TextButton(onClick = {
                    model.clearHistory(key)
                    confirmClear = false
                }) { Text(stringResource(R.string.profile_clear_history_ok), color = HoloError) }
            },
            dismissButton = {
                TextButton(onClick = { confirmClear = false }) {
                    Text(stringResource(R.string.common_cancel), color = HoloSecondary)
                }
            },
        )
    }

    if (confirmDelete) {
        AlertDialog(
            onDismissRequest = { confirmDelete = false },
            title = { Text(stringResource(R.string.info_delete)) },
            text = { Text(stringResource(R.string.profile_delete_confirm)) },
            confirmButton = {
                TextButton(onClick = {
                    model.remove(key)
                    confirmDelete = false
                    onDeleted()
                }) { Text(stringResource(R.string.folder_delete_ok), color = HoloError) }
            },
            dismissButton = {
                TextButton(onClick = { confirmDelete = false }) {
                    Text(stringResource(R.string.common_cancel), color = HoloSecondary)
                }
            },
        )
    }
}

@Composable
private fun RenameDialog(model: AppModel, key: String, current: String, onClose: () -> Unit) {
    var name by remember { mutableStateOf(current) }
    AlertDialog(
        onDismissRequest = onClose,
        title = { Text(stringResource(R.string.profile_rename)) },
        text = {
            Box(
                Modifier.fillMaxWidth().height(48.dp).clip(RoundedCornerShape(Tokens.Radius.tile))
                    .background(HoloBg).padding(horizontal = Tokens.Space.m),
                contentAlignment = Alignment.CenterStart,
            ) {
                BasicTextField(
                    value = name,
                    onValueChange = { name = it },
                    singleLine = true,
                    textStyle = Tokens.Text.base.copy(color = Color.White),
                    cursorBrush = SolidColor(HoloAccent),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        },
        confirmButton = {
            TextButton(onClick = {
                model.rename(key, name.trim())
                onClose()
            }) { Text(stringResource(R.string.common_save), color = HoloAccent) }
        },
        dismissButton = {
            TextButton(onClick = onClose) {
                Text(stringResource(R.string.common_cancel), color = HoloSecondary)
            }
        },
    )
}

@Composable
private fun ActionTile(icon: ImageVector, labelRes: Int, modifier: Modifier, onClick: () -> Unit) {
    Column(
        modifier.clip(RoundedCornerShape(Tokens.Radius.tile)).background(HoloSurface)
            .clickable { onClick() }.padding(vertical = Tokens.Space.m),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(icon, null, tint = HoloAccent, modifier = Modifier.size(22.dp))
        Spacer(Modifier.height(5.dp))
        Text(stringResource(labelRes), color = HoloAccent, fontSize = Tokens.Font.small)
    }
}

@Composable
private fun InfoRow(icon: ImageVector, label: String, value: String, divider: Boolean = true) {
    CardRow(divider = divider) {
        Icon(icon, null, tint = Tokens.Colors.secondary, modifier = Modifier.size(Tokens.Size.iconM))
        Spacer(Modifier.width(Tokens.Space.m))
        Text(label, color = Color.White, fontSize = Tokens.Font.label, modifier = Modifier.weight(1f))
        Text(value, color = Tokens.Colors.secondary, fontSize = Tokens.Font.body)
    }
}

@Composable
private fun DangerRow(
    icon: ImageVector,
    label: String,
    tint: Color,
    divider: Boolean = true,
    onClick: () -> Unit,
) {
    CardRow(onClick = onClick, divider = divider) {
        Icon(icon, null, tint = tint, modifier = Modifier.size(Tokens.Size.iconM))
        Spacer(Modifier.width(Tokens.Space.m))
        Text(label, color = tint, fontSize = Tokens.Font.label)
    }
}

private fun copyToClipboard(ctx: android.content.Context, text: String) {
    val cm = ctx.getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
    cm.setPrimaryClip(android.content.ClipData.newPlainText("key", text))
}
