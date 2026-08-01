@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import android.os.Build
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Group
import androidx.compose.material.icons.filled.Headset
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.R

@Composable
internal fun ContactsScreen(
    model: AppModel,
    activeFilter: String,
    onOpenDm: (String) -> Unit,
    onOpenGroup: (String) -> Unit,
) {
    // Пользовательская папка — это ФИЛЬТР по правилам, встроенные — по типу чата.
    val folder = model.folderById(activeFilter)
    val showDm = folder != null || activeFilter == "all" || activeFilter == "dm"
    val showRoom = folder != null || activeFilter == "all" || activeFilter == "room"
    val dmList = if (folder == null) model.contacts else model.contacts.filter { model.folderHasDm(folder, it.key) }
    val groupList = if (folder == null) model.groups else model.groups.filter { model.folderHasGroup(folder, it.gid) }

    Column(Modifier.fillMaxSize()) {
        // Очередь недоставленного разрослась. НИЧЕГО не удаляем — сообщения ждут
        // доставки; просто предупреждаем, чтобы человек знал (тумблер в Настройках).
        if (model.outboxWarn && model.warnOutboxGrowth) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = ScreenPad, vertical = 6.dp)
                    .clip(RoundedCornerShape(10.dp))
                    .background(Color(0xFF3A2F12))
                    .padding(horizontal = 12.dp, vertical = 10.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("⚠", color = Color(0xFFF6C453), fontSize = 15.sp)
                Spacer(Modifier.width(10.dp))
                Text(
                    stringResource(R.string.outbox_warn, model.outboxPending),
                    color = Color(0xFFF6C453),
                    fontSize = 13.sp,
                    modifier = Modifier.weight(1f),
                )
            }
        }
        // Счётчик пиров переехал в шапку (PeerIndicator). Здесь — только бенч-замер
        // приёма, пока он активен (диагностика; запуск — долгим тапом по индикатору).
        if (model.benchRecv) {
            Text(
                stringResource(R.string.bench_recv_rate, "%.1f".format(model.benchRecvMbps)),
                color = HoloAccent,
                fontSize = 11.sp,
                modifier = Modifier.padding(horizontal = ScreenPad, vertical = 6.dp),
            )
        }
        when {
            activeFilter == "channel" -> CenterMessage(stringResource(R.string.channels_soon))
            model.contacts.isEmpty() && model.groups.isEmpty() &&
                model.contactRequests.isEmpty() && model.groupInvites.isEmpty() ->
                CenterMessage(stringResource(R.string.contacts_empty))
            else -> LazyColumn(Modifier.fillMaxSize()) {
                // Гейт: входящие запросы от незнакомцев — карточками наверху (фильтр DM/all).
                if (showDm && model.contactRequests.isNotEmpty()) {
                    item(key = "req_header") {
                        Text(
                            stringResource(R.string.requests_header, model.contactRequests.size),
                            color = HoloAccent,
                            fontSize = 12.sp,
                            fontWeight = FontWeight.SemiBold,
                            modifier = Modifier.padding(start = ScreenPad, end = ScreenPad, top = 10.dp, bottom = 4.dp),
                        )
                    }
                    items(model.contactRequests, key = { "req_${it.key}" }) { r ->
                        ContactRequestCard(model, r)
                    }
                }
                // Приглашения в группы — карточками (фильтр Комнаты/all).
                if (showRoom && model.groupInvites.isNotEmpty()) {
                    item(key = "ginv_header") {
                        Text(
                            stringResource(R.string.group_invites_header, model.groupInvites.size),
                            color = HoloAccent,
                            fontSize = 12.sp,
                            fontWeight = FontWeight.SemiBold,
                            modifier = Modifier.padding(start = ScreenPad, end = ScreenPad, top = 10.dp, bottom = 4.dp),
                        )
                    }
                    items(model.groupInvites, key = { "ginv_${it.gid}" }) { inv ->
                        GroupInviteCard(model, inv)
                    }
                }
                // Комнаты — наверху, если фильтр позволяет
                if (showRoom) {
                    items(groupList, key = { "g_${it.gid}" }) { g ->
                        val presenceCount = model.groupPresence[g.gid]?.size ?: 0
                        val inRoom = model.currentRoom == g.gid
                        ListRow(onClick = { onOpenGroup(g.gid) }) {
                            SquareAvatar(g.name, size = 44.dp, bg = Color(0xFF0D2A0D)) {
                                Icon(Icons.Filled.Headset, null, tint = HoloOnline, modifier = Modifier.size(22.dp))
                            }
                            Spacer(Modifier.width(12.dp))
                            Column(Modifier.weight(1f)) {
                                Text(g.name, color = Color.White, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                Text(
                                    if (inRoom) {
                                        stringResource(R.string.presence_on_air)
                                    } else if (presenceCount > 0) {
                                        stringResource(R.string.presence_in_room, presenceCount)
                                    } else {
                                        stringResource(R.string.members_count, g.members)
                                    },
                                    color = if (inRoom || presenceCount > 0) HoloOnline else HoloSecondary,
                                    fontSize = 11.sp,
                                )
                            }
                        }
                    }
                }
                // DM-контакты
                if (showDm) {
                    items(dmList, key = { "c_${it.key}" }) { c ->
                        val online = model.online[c.key] == true
                        ListRow(onClick = { onOpenDm(c.key) }) {
                            SquareAvatar(c.displayName, size = 44.dp, online = online, avatarBytes = model.contactAvatars[c.avatarId])
                            Spacer(Modifier.width(12.dp))
                            Column(Modifier.weight(1f)) {
                                Row(verticalAlignment = Alignment.CenterVertically) {
                                    Text(
                                        c.displayName,
                                        color = Color.White,
                                        fontSize = 15.sp,
                                        maxLines = 1,
                                        overflow = TextOverflow.Ellipsis,
                                        modifier = Modifier.weight(1f, fill = false),
                                    )
                                    if (c.verified) {
                                        Spacer(Modifier.width(4.dp))
                                        Icon(Icons.Filled.Verified, null, tint = HoloVerified, modifier = Modifier.size(13.dp))
                                    }
                                }
                                Text(
                                    stringResource(if (online) R.string.status_online else R.string.status_offline),
                                    color = if (online) HoloOnline else HoloSecondary,
                                    fontSize = 11.sp,
                                )
                            }
                            val unread = model.unread[c.key] ?: 0
                            if (unread > 0) {
                                Box(Modifier.clip(RoundedCornerShape(percent = 30)).background(HoloAccent).padding(horizontal = 7.dp, vertical = 2.dp)) {
                                    Text("$unread", color = HoloAccentDark, fontSize = 11.sp)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/** Карточка входящего запроса в контакты (гейт вместо TOFU): аватар-буква, короткий
 *  ключ, превью первого сообщения и кнопки Принять / Отклонить / Заблокировать. */
@Composable
private fun ContactRequestCard(model: AppModel, r: ContactRequest) {
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = ScreenPad, vertical = 4.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(HoloSurface)
            .padding(12.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            SquareAvatar(r.shortKey, size = 40.dp)
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    r.shortKey + "…",
                    color = Color.White,
                    fontSize = 15.sp,
                    fontFamily = FontFamily.Monospace,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                if (r.preview.isNotBlank()) {
                    Text(
                        r.preview,
                        color = HoloSecondary,
                        fontSize = 13.sp,
                        maxLines = 2,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }
        Spacer(Modifier.height(10.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Button(
                onClick = { model.acceptRequest(r.key) },
                colors = ButtonDefaults.buttonColors(containerColor = HoloAccent, contentColor = HoloAccentDark),
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 6.dp),
            ) { Text(stringResource(R.string.req_accept), fontSize = 13.sp) }
            Spacer(Modifier.width(8.dp))
            TextButton(onClick = { model.declineRequest(r.key) }) {
                Text(stringResource(R.string.req_decline), color = HoloSecondary, fontSize = 13.sp)
            }
            Spacer(Modifier.weight(1f))
            TextButton(onClick = { model.blockRequest(r.key) }) {
                Text(stringResource(R.string.req_block), color = HoloError, fontSize = 13.sp)
            }
        }
    }
}

/** Карточка входящего приглашения в группу (вместо тихого авто-входа): аватар-иконка,
 *  имя группы, кто пригласил, кнопки Принять / Отклонить. */
@Composable
private fun GroupInviteCard(model: AppModel, inv: GroupInvite) {
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = ScreenPad, vertical = 4.dp)
            .clip(RoundedCornerShape(12.dp))
            .background(HoloSurface)
            .padding(12.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            SquareAvatar(inv.name.ifBlank { "#" }, size = 40.dp, bg = Color(0xFF0D2A0D)) {
                Icon(Icons.Filled.Group, null, tint = HoloOnline, modifier = Modifier.size(20.dp))
            }
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    inv.name.ifBlank { stringResource(R.string.group_default_name) },
                    color = Color.White,
                    fontSize = 15.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    stringResource(R.string.group_invite_from, model.peerName(inv.inviter)),
                    color = HoloSecondary,
                    fontSize = 13.sp,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
        Spacer(Modifier.height(10.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            Button(
                onClick = { model.acceptGroupInvite(inv.gid) },
                colors = ButtonDefaults.buttonColors(containerColor = HoloAccent, contentColor = HoloAccentDark),
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 6.dp),
            ) { Text(stringResource(R.string.group_invite_join), fontSize = 13.sp) }
            Spacer(Modifier.width(8.dp))
            TextButton(onClick = { model.declineGroupInvite(inv.gid) }) {
                Text(stringResource(R.string.req_decline), color = HoloSecondary, fontSize = 13.sp)
            }
        }
    }
}

/** Полный список контактов (раздел меню «Контакты»). Тап → личный чат.
 *  Добавление — через кнопку в шапке (Root). */
@Composable
internal fun ContactListScreen(model: AppModel, onOpen: (String) -> Unit) {
    if (model.contacts.isEmpty()) {
        EmptyState(Icons.Filled.Group, stringResource(R.string.contacts_empty_title), stringResource(R.string.contacts_empty_hint))
        return
    }
    LazyColumn(Modifier.fillMaxSize()) {
        items(model.contacts, key = { it.key }) { c ->
            val online = model.online[c.key] == true
            ListRow(onClick = { onOpen(c.key) }) {
                SquareAvatar(c.displayName, size = 44.dp, online = online, avatarBytes = model.contactAvatars[c.avatarId])
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            c.displayName,
                            color = Color.White,
                            fontSize = 15.sp,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f, fill = false),
                        )
                        if (c.verified) {
                            Spacer(Modifier.width(4.dp))
                            Icon(Icons.Filled.Verified, null, tint = HoloVerified, modifier = Modifier.size(13.dp))
                        }
                    }
                    Text(
                        if (online) "онлайн" else "не в сети",
                        color = if (online) HoloOnline else HoloSecondary,
                        fontSize = 11.sp,
                    )
                }
            }
        }
    }
}

@Composable
internal fun AddScreen(
    model: AppModel,
    scannedKey: String? = null,
    onScan: () -> Unit = {},
    onDone: () -> Unit,
) {
    var keyHex by remember { mutableStateOf("") }
    var name by remember { mutableStateOf("") }
    // Операция асинхронна; результат приходит в model.addStatus.
    LaunchedEffect(Unit) { model.addStatus = "" }
    val st = model.addStatus
    LaunchedEffect(st) {
        if (st == "added") {
            model.addStatus = ""
            onDone()
        }
    }
    // Получаем ключ из сканера (приходит через Root → scannedKey).
    LaunchedEffect(scannedKey) { if (scannedKey != null) keyHex = scannedKey }
    Column(Modifier.padding(ScreenPad), verticalArrangement = Arrangement.spacedBy(FormGap)) {
        FieldLabel(stringResource(R.string.add_label_pubkey))
        TextField(
            keyHex,
            { keyHex = it },
            Modifier.fillMaxWidth(),
            colors = inputColors(),
            placeholder = { Text(stringResource(R.string.add_hint_hex), color = HoloSecondary) },
        )
        AccentButton(stringResource(R.string.title_scan_qr), modifier = Modifier.fillMaxWidth()) { onScan() }
        FieldLabel(stringResource(R.string.label_name))
        TextField(name, { name = it }, Modifier.fillMaxWidth(), colors = inputColors())
        AccentButton(stringResource(R.string.add_btn)) { model.addContact(keyHex.trim(), name.trim()) }
        val msg = when (st) {
            "known" -> stringResource(R.string.add_known)
            "error" -> stringResource(R.string.add_error)
            else -> ""
        }
        if (msg.isNotEmpty()) Text(msg, color = HoloError, fontSize = 13.sp)
    }
}

@Composable
internal fun MeScreen(model: AppModel) {
    val me = model.me ?: return
    // В QR — код обмена, а не голый ключ: после смены адреса по одной лишь
    // личности до нас не достучаться (ADR-007).
    val code = remember(me.pubkey) { model.myContactCode() }
    val qr = remember(code) { generateQrBitmap(code) }
    Column(
        Modifier.verticalScroll(rememberScrollState()).padding(ScreenPad).fillMaxWidth(),
        verticalArrangement = Arrangement.spacedBy(FormGap),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        if (qr != null) {
            Image(
                bitmap = qr.asImageBitmap(),
                contentDescription = stringResource(R.string.qr_cd),
                modifier = Modifier.size(240.dp).clip(RoundedCornerShape(8.dp)),
            )
            Spacer(Modifier.height(4.dp))
        }
        Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(FormGap)) {
            FieldLabel(stringResource(R.string.me_fingerprint_label))
            FingerprintGrid(model.visualFingerprint(), Modifier.padding(vertical = 4.dp))
            Text(
                stringResource(R.string.fp_self_hint),
                color = HoloSecondary,
                fontSize = 11.sp,
            )
            Text(me.fingerprint, color = HoloAccent, fontSize = 13.sp)
            FieldLabel(stringResource(R.string.me_address_label))
            Text(me.address, color = Color.White, fontSize = 13.sp, fontFamily = FontFamily.Monospace)
            FieldLabel(stringResource(R.string.me_pubkey_label))
            Text(me.pubkey, color = Color.White, fontSize = 13.sp, fontFamily = FontFamily.Monospace)
            val ctx = LocalContext.current
            AccentButton(stringResource(R.string.me_copy_key), modifier = Modifier.fillMaxWidth()) {
                val cm = ctx.getSystemService(android.content.Context.CLIPBOARD_SERVICE)
                    as android.content.ClipboardManager
                cm.setPrimaryClip(android.content.ClipData.newPlainText("p2p pubkey", me.pubkey))
                // API 33+ сам показывает тост «Скопировано», на старых — делаем вручную.
                if (android.os.Build.VERSION.SDK_INT < android.os.Build.VERSION_CODES.TIRAMISU) {
                    android.widget.Toast.makeText(ctx, ctx.getString(R.string.common_copied), android.widget.Toast.LENGTH_SHORT).show()
                }
            }
        }
    }
}

@Composable
internal fun InfoScreen(model: AppModel, onDeleted: () -> Unit) {
    val key = model.currentChat ?: return
    val c = model.contacts.firstOrNull { it.key == key } ?: return
    var name by remember(key) { mutableStateOf(c.name) }
    Column(
        Modifier.verticalScroll(rememberScrollState()).padding(ScreenPad),
        verticalArrangement = Arrangement.spacedBy(FormGap),
    ) {
        // Шапка: аватар + имя для показа (M2) — сразу видно, кто это.
        Row(verticalAlignment = Alignment.CenterVertically) {
            SquareAvatar(c.displayName, size = 56.dp, avatarBytes = model.contactAvatars[c.avatarId])
            Spacer(Modifier.width(14.dp))
            Text(c.displayName, color = Color.White, fontSize = 20.sp, fontWeight = FontWeight.Medium)
        }
        FieldLabel(stringResource(R.string.label_name))
        Row(verticalAlignment = Alignment.CenterVertically) {
            TextField(name, { name = it }, Modifier.weight(1f), colors = inputColors())
            Spacer(Modifier.width(8.dp))
            AccentButton("OK") { model.rename(key, name) }
        }
        // Самоназвание контакта (как он назвал СЕБЯ) — подсказка для выбора заметки.
        if (c.profileName.isNotBlank()) {
            Text(
                stringResource(R.string.info_self_name, c.profileName),
                color = HoloSecondary,
                fontSize = 11.sp,
            )
        }
        // Сверка глазами: знаки рисует само приложение, поэтому картинка одинакова
        // на любом устройстве. Слова BIP39 оставлены рядом — их удобно диктовать
        // голосом, когда сверяются по видеосвязи.
        FieldLabel(stringResource(R.string.fp_label))
        FingerprintGrid(model.visualFingerprint(key), Modifier.padding(vertical = 4.dp))
        Text(
            stringResource(R.string.fp_hint, c.displayName),
            color = HoloSecondary,
            fontSize = 11.sp,
        )
        Text(c.fingerprint, color = HoloAccent, fontSize = 13.sp)
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (c.verified) {
                Icon(Icons.Filled.Verified, null, tint = HoloVerified, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(6.dp))
            }
            Text(
                stringResource(if (c.verified) R.string.info_verified else R.string.info_not_verified),
                color = if (c.verified) HoloVerified else WarnColor,
                modifier = Modifier.weight(1f),
            )
            AccentButton(stringResource(if (c.verified) R.string.info_unmark else R.string.info_mark)) { model.toggleVerified(key) }
        }
        FieldLabel(stringResource(R.string.me_pubkey_label))
        Text(c.key, color = Color.White, fontSize = 12.sp, fontFamily = FontFamily.Monospace)
        Button(
            onClick = {
                model.remove(key)
                onDeleted()
            },
            modifier = Modifier.fillMaxWidth(),
            colors = ButtonDefaults.buttonColors(containerColor = HoloError),
        ) {
            Icon(Icons.Filled.Delete, null, modifier = Modifier.size(18.dp))
            Spacer(Modifier.width(8.dp))
            Text(stringResource(R.string.info_delete), color = Color.White)
        }
        // Диагностика: замер скорости меша до этого контакта (~30 МБ блба).
        FieldLabel(stringResource(R.string.info_diagnostics))
        AccentButton(stringResource(R.string.info_speed_test)) { model.blastBench(key) }
        if (model.benchResult.isNotEmpty()) {
            Text(model.benchResult, color = HoloSecondary, fontSize = 12.sp)
        }
    }
}
