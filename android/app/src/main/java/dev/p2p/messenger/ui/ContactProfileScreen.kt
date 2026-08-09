package dev.p2p.messenger.ui

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
import androidx.compose.material.icons.filled.Message
import androidx.compose.material.icons.filled.Photo
import androidx.compose.material.icons.filled.Speed
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
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.R

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

    // Сколько вложений от этого чата лежит на диске — цифра уже посчитана движком
    // для «Хранилища», второй раз считать незачем.
    LaunchedEffect(key) { model.refreshStorage() }
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
                Text(c.displayName, color = Color.White, fontSize = 22.sp, fontWeight = FontWeight.Medium)
                if (c.verified) {
                    Spacer(Modifier.width(6.dp))
                    Icon(Icons.Filled.Verified, null, tint = HoloVerified, modifier = Modifier.size(19.dp))
                }
            }
            Text(
                stringResource(if (online) R.string.status_online else R.string.status_offline),
                color = if (online) HoloOnline else HoloSecondary,
                fontSize = 13.sp,
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
                        fontSize = 12.sp,
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
        Spacer(Modifier.height(Tokens.Space.m))
        CardSection {
            Column(Modifier.padding(Tokens.Space.l)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Icon(
                        if (c.verified) Icons.Filled.Verified else Icons.Filled.Block,
                        null,
                        tint = if (c.verified) HoloVerified else WarnColor,
                        modifier = Modifier.size(20.dp),
                    )
                    Spacer(Modifier.width(Tokens.Space.s))
                    Text(
                        stringResource(if (c.verified) R.string.info_verified else R.string.info_not_verified),
                        color = if (c.verified) HoloVerified else WarnColor,
                        fontSize = 15.sp,
                        fontWeight = FontWeight.Medium,
                    )
                }
                Text(
                    stringResource(
                        if (c.verified) R.string.profile_verified_hint else R.string.profile_unverified_hint,
                        c.displayName,
                    ),
                    color = HoloSecondary,
                    fontSize = 12.sp,
                    modifier = Modifier.padding(top = Tokens.Space.s),
                )
                FingerprintGrid(model.visualFingerprint(key), Modifier.padding(top = Tokens.Space.l))
                // Слова — тот же отпечаток, но их удобно диктовать голосом.
                Text(
                    c.fingerprint,
                    color = HoloAccent,
                    fontSize = 12.sp,
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
            DangerRow(Icons.Filled.Speed, stringResource(R.string.info_speed_test), Tokens.Colors.secondary) {
                model.blastBench(key)
            }
        }
        if (model.benchResult.isNotEmpty()) {
            Text(
                model.benchResult,
                color = HoloSecondary,
                fontSize = 12.sp,
                modifier = Modifier.padding(horizontal = ScreenPad, vertical = Tokens.Space.xs),
            )
        }
        // Блокировка и удаление — в СВОЕЙ карточке: их отделяет от остального не
        // заголовок, а разрыв, иначе «удалить» стоит вплотную к «переименовать».
        CardSection {
            DangerRow(Icons.Filled.Block, stringResource(R.string.profile_block), HoloError, divider = false) {
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
                    textStyle = TextStyle(color = Color.White, fontSize = 15.sp),
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
        Text(stringResource(labelRes), color = HoloAccent, fontSize = 12.sp)
    }
}

@Composable
private fun InfoRow(icon: ImageVector, label: String, value: String, divider: Boolean = true) {
    CardRow(divider = divider) {
        Icon(icon, null, tint = Tokens.Colors.secondary, modifier = Modifier.size(Tokens.Size.iconM))
        Spacer(Modifier.width(Tokens.Space.m))
        Text(label, color = Color.White, fontSize = 15.sp, modifier = Modifier.weight(1f))
        Text(value, color = Tokens.Colors.secondary, fontSize = 13.sp)
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
        Text(label, color = tint, fontSize = 15.sp)
    }
}

private fun copyToClipboard(ctx: android.content.Context, text: String) {
    val cm = ctx.getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
    cm.setPrimaryClip(android.content.ClipData.newPlainText("key", text))
}
