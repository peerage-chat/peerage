package chat.peerage.ui

import android.app.Activity
import android.content.Intent
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.TextUnit
import chat.peerage.AppLanguage
import chat.peerage.AppModel
import chat.peerage.R
import chat.peerage.Screen
import chat.peerage.Tokens
import chat.peerage.openSaved
import chat.peerage.refreshStorage

/**
 * «Профиль» — узел всего, что не список чатов (app-ui.md → «Навигация»): карточка себя,
 * разделы и категории настроек прямо здесь. Отдельного экрана «Настройки» нет — он был
 * лишним шагом между профилем и пятью категориями.
 */
@Composable
internal fun ProfileScreen(model: AppModel, navigate: (Screen) -> Unit) {
    val me = model.me ?: return
    LaunchedEffect(Unit) { model.refreshStorage() }
    val name = model.selfName.ifBlank { stringResource(R.string.common_you) }
    val ctx = LocalContext.current
    var langSheet by remember { mutableStateOf(false) }
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState())
            .padding(top = Tokens.Space.xl, bottom = Tokens.Space.xxl),
    ) {
        Row(
            Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.panelText),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
        ) {
            OpenableAvatar(model, name, model.selfAvatar) {
                SquareAvatar(name, size = Tokens.Size.avatarProfile, avatarBytes = model.selfAvatar, circle = true)
            }
            Column(Modifier.weight(1f)) {
                Text(
                    name,
                    color = Color.White,
                    fontSize = Tokens.Font.sheet,
                    fontWeight = FontWeight.Medium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                NetStatus(model.netPeers, Tokens.Font.body)
                Text(
                    me.address,
                    color = Tokens.Colors.tertiary,
                    fontSize = Tokens.Font.small,
                    fontFamily = FontFamily.Monospace,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            val codeLabel = stringResource(R.string.my_code_title)
            Box(
                Modifier.size(Tokens.Size.touch)
                    .clip(RoundedCornerShape(Tokens.Radius.button))
                    .background(Tokens.Colors.group)
                    .clickable(role = Role.Button, onClickLabel = codeLabel) { navigate(Screen.MyCode) },
                contentAlignment = Alignment.Center,
            ) {
                Icon(TablerIcons.Qrcode, codeLabel, tint = Tokens.Colors.iconBar, modifier = Modifier.size(Tokens.Size.iconRow))
            }
        }

        Spacer(Modifier.height(Tokens.Space.xl))
        CardSection {
            NavRow(TablerIcons.Users, stringResource(R.string.menu_contacts), "${model.contacts.size}", divider = false) {
                navigate(Screen.ContactList)
            }
            NavRow(TablerIcons.Phone, stringResource(R.string.menu_calls)) { navigate(Screen.Calls) }
            NavRow(TablerIcons.Bookmark, stringResource(R.string.menu_saved)) {
                model.openSaved()
                navigate(Screen.Chat)
            }
        }

        Spacer(Modifier.height(Tokens.Space.l))
        CardSection {
            NavRow(
                TablerIcons.Topology,
                stringResource(R.string.menu_network),
                pluralStringResource(R.plurals.net_nodes, model.netPeers, model.netPeers),
                divider = false,
            ) { navigate(Screen.Network) }
            NavRow(
                TablerIcons.Database,
                stringResource(R.string.menu_storage),
                if (model.storageBlobBytes > 0) fmtSizeLong(model.storageBlobBytes) else "",
            ) { navigate(Screen.Storage) }
        }

        SectionTitle(stringResource(R.string.menu_settings))
        CardSection {
            NavRow(TablerIcons.ShieldLock, stringResource(R.string.settings_cat_security), divider = false) {
                navigate(Screen.SettingsSecurity)
            }
            NavRow(TablerIcons.EyeOff, stringResource(R.string.settings_cat_privacy)) { navigate(Screen.SettingsPrivacy) }
            NavRow(TablerIcons.Bell, stringResource(R.string.settings_cat_notifications)) {
                navigate(Screen.SettingsNotifications)
            }
            NavRow(TablerIcons.Video, stringResource(R.string.settings_cat_media)) { navigate(Screen.SettingsMedia) }
            NavRow(
                TablerIcons.Language,
                stringResource(R.string.settings_cat_language),
                AppLanguage.currentName(ctx) ?: stringResource(R.string.lang_system),
            ) { langSheet = true }
            NavRow(TablerIcons.InfoCircle, stringResource(R.string.settings_cat_about)) { navigate(Screen.SettingsAbout) }
        }
    }
    if (langSheet) LanguageSheet(onClose = { langSheet = false })
}

/** Выбор языка интерфейса: как в системе или один из переводов. Окно перерисуется на выбранном. */
@Composable
private fun LanguageSheet(onClose: () -> Unit) {
    val ctx = LocalContext.current
    val current = remember { AppLanguage.current(ctx) }
    val options = listOf("" to stringResource(R.string.lang_system)) + AppLanguage.choices
    SheetFrame(onClose = onClose) {
        Text(
            stringResource(R.string.settings_cat_language),
            color = Color.White,
            fontSize = Tokens.Font.sheet,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.padding(horizontal = Tokens.Space.xs),
        )
        Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.group)).background(Tokens.Colors.raised)) {
            options.forEachIndexed { i, (tag, label) ->
                CardRow(
                    onClick = {
                        onClose()
                        if (tag != current) (ctx as? Activity)?.let { AppLanguage.set(it, tag) }
                    },
                    divider = i > 0,
                    inset = Tokens.Space.l,
                ) {
                    Text(label, color = Color.White, fontSize = Tokens.Font.name, modifier = Modifier.weight(1f))
                    if (tag == current) Icon(TablerIcons.Check, null, tint = Tokens.Colors.accent, modifier = Modifier.size(Tokens.Size.iconRow))
                }
            }
        }
    }
}

/** Статус меша словами: «В сети · 8 узлов» зелёным или «Нет соединения · поиск узлов» янтарём. */
@Composable
internal fun NetStatus(peers: Int, size: TextUnit, modifier: Modifier = Modifier) {
    Text(
        if (peers > 0) {
            pluralStringResource(R.plurals.net_status_online, peers, peers)
        } else {
            stringResource(R.string.net_status_offline)
        },
        color = if (peers > 0) Tokens.Colors.online else Tokens.Colors.warn,
        fontSize = size,
        maxLines = 1,
        overflow = TextOverflow.Ellipsis,
        modifier = modifier,
    )
}

/**
 * «Мой код» — то, что показывают другому телефону: QR кода обмена и он же текстом. Адреса и
 * знаков отпечатка здесь нет: адрес — в «Сети», сверка — набором на пару в профиле контакта.
 */
@Composable
internal fun MyCodeScreen(model: AppModel, onScan: () -> Unit) {
    // В QR — код обмена, а не голый ключ: после смены адреса по одной личности до нас не
    // достучаться (ADR-007). Код живёт в модели и обновляется при смене имени и адреса.
    val code = model.contactCode
    val qr = remember(code) { generateQrBitmap(code) }
    val ctx = LocalContext.current
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState())
            .padding(horizontal = Tokens.Space.panel)
            .padding(top = Tokens.Space.xxl, bottom = Tokens.Space.xxl),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        if (qr != null) {
            Image(
                bitmap = qr.asImageBitmap(),
                contentDescription = stringResource(R.string.qr_cd),
                modifier = Modifier.size(Tokens.Size.qr).clip(RoundedCornerShape(Tokens.Radius.group)),
            )
        }
        Text(
            model.selfName.ifBlank { stringResource(R.string.common_you) },
            color = Color.White,
            fontSize = Tokens.Font.title,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.padding(top = Tokens.Space.xl),
        )
        Text(
            stringResource(R.string.my_code_hint),
            color = Tokens.Colors.tertiary,
            fontSize = Tokens.Font.body,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = Tokens.Space.s, start = Tokens.Space.s, end = Tokens.Space.s),
        )
        Spacer(Modifier.height(Tokens.Space.xl))
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s)) {
            PlateButton(stringResource(R.string.my_code_copy), TablerIcons.Copy, Modifier.weight(1f)) {
                copyToClipboard(ctx, code, "exchange code")
            }
            val chooser = stringResource(R.string.my_code_share)
            PlateButton(chooser, TablerIcons.Share, Modifier.weight(1f)) {
                val send = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, code)
                ctx.startActivity(Intent.createChooser(send, chooser))
            }
        }
        Spacer(Modifier.height(Tokens.Space.s))
        PlateButton(stringResource(R.string.my_code_scan), TablerIcons.Scan, Modifier.fillMaxWidth(), onClick = onScan)
    }
}
