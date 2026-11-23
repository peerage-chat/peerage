package chat.peerage.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R

/**
 * «Узлы входа» (макет `peers/`): к кому узел Yggdrasil подключается сам — узлы из сборки
 * (одним тумблером) и свои адреса. Правка применяется сразу, без перезапуска.
 */

/** Исходящее соединение к узлу [uri], если оно есть в снимке сети. */
private fun NetStats?.link(uri: String): NetPeer? = this?.peers?.firstOrNull { !it.inbound && it.uri == uri }

/** Строка «Узлы входа» в «Сети»: «8 из сборки · 2 своих». */
@Composable
internal fun entryNodesSummary(s: PeerSettings?): String {
    if (s == null) return ""
    val parts = buildList {
        if (s.builtinOn) add(stringResource(R.string.entry_row_builtin, s.builtin.size))
        if (s.own.isNotEmpty()) add(pluralStringResource(R.plurals.entry_row_own, s.own.size, s.own.size))
    }
    return if (parts.isEmpty()) stringResource(R.string.entry_row_none) else parts.joinToString(" · ")
}

/** Шапка: заголовок и со сколькими узлами входа есть связь. */
@Composable
internal fun EntryNodesHeader(model: AppModel, onBack: () -> Unit) {
    val s = model.peerSettings
    val uris = s?.let { (if (it.builtinOn) it.builtin else emptyList()) + it.own }.orEmpty()
    val up = uris.count { model.netStats.link(it)?.up == true }
    val (status, color) = when {
        model.offline -> stringResource(R.string.net_status_away) to Tokens.Colors.warn
        up > 0 -> pluralStringResource(R.plurals.entry_status_n, up, up) to Tokens.Colors.online
        s != null && uris.isEmpty() -> stringResource(R.string.entry_status_none) to Tokens.Colors.warn
        else -> stringResource(R.string.entry_status_none) to Tokens.Colors.tertiary
    }
    TopBar(stringResource(R.string.entry_title), onBack = onBack, subtitle = status, subtitleColor = color)
}

@Composable
internal fun EntryNodesScreen(model: AppModel) {
    LaunchedEffect(Unit) { model.refreshPeerSettings() }
    val s = model.peerSettings ?: return
    var adding by remember { mutableStateOf(false) }
    var allBuiltin by remember { mutableStateOf(false) }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState())
            .padding(top = Tokens.Space.s, bottom = Tokens.Space.xxl),
    ) {
        if (!s.builtinOn && s.own.isEmpty()) {
            NoNodesPlate { model.setBuiltinNodes(true) }
            Spacer(Modifier.height(Tokens.Space.m))
        }

        CardSection {
            CardRow(
                divider = false,
                inset = Tokens.Space.l,
                modifier = Modifier.switchRow(s.builtinOn) { model.setBuiltinNodes(it) },
            ) {
                Column(Modifier.weight(1f).padding(vertical = Tokens.Space.xs)) {
                    Text(stringResource(R.string.entry_builtin), color = Color.White, fontSize = Tokens.Font.name)
                    val count = pluralStringResource(R.plurals.entry_builtin_count, s.builtin.size, s.builtin.size)
                    Text(
                        stringResource(if (s.builtinOn) R.string.entry_builtin_on else R.string.entry_builtin_off, count),
                        color = Tokens.Colors.tertiary,
                        fontSize = Tokens.Font.body,
                        modifier = Modifier.padding(top = Tokens.Space.xxs),
                    )
                }
                Spacer(Modifier.width(Tokens.Space.m))
                Switch(checked = s.builtinOn, onCheckedChange = null)
            }
        }

        SectionTitle(
            if (s.own.isEmpty()) {
                stringResource(R.string.entry_own_label_empty)
            } else {
                stringResource(R.string.entry_own_label, s.own.size)
            },
        )
        CardSection {
            if (s.own.isEmpty()) {
                CardRow(divider = false) {
                    Text(stringResource(R.string.entry_own_none), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.label)
                }
            }
            s.own.forEachIndexed { i, uri ->
                NodeRow(uri, model.netStats.link(uri), divider = i > 0) { model.removeEntryNode(uri) }
            }
        }

        Spacer(Modifier.height(Tokens.Space.l))
        PlateButton(
            stringResource(R.string.entry_add),
            icon = TablerIcons.Plus,
            primary = true,
            modifier = Modifier.padding(horizontal = Tokens.Space.panel).fillMaxWidth(),
        ) { adding = true }

        if (s.builtinOn) {
            SectionTitle(stringResource(R.string.entry_builtin_label, s.builtin.size))
            CardSection {
                val shown = if (allBuiltin) s.builtin else s.builtin.take(BUILTIN_SHOWN)
                shown.forEachIndexed { i, uri -> NodeRow(uri, model.netStats.link(uri), divider = i > 0) }
                if (!allBuiltin && s.builtin.size > BUILTIN_SHOWN) {
                    CardRow(inset = Tokens.Space.l, modifier = Modifier.clickable(role = Role.Button) { allBuiltin = true }) {
                        Text(
                            stringResource(R.string.entry_show_all, s.builtin.size),
                            color = Tokens.Colors.accent,
                            fontSize = Tokens.Font.label,
                        )
                    }
                }
            }
        }

        Text(
            stringResource(R.string.entry_ip_note),
            color = Tokens.Colors.tertiary,
            fontSize = Tokens.Font.body,
            modifier = Modifier.padding(start = Tokens.Space.panelText, end = Tokens.Space.panelText, top = Tokens.Space.m),
        )
    }

    if (adding) AddNodeSheet(model) { adding = false }
}

/** Сколько узлов из сборки видно до «Показать все». */
private const val BUILTIN_SHOWN = 3

/** Узел: адрес и статус соединения; у своего — крестик «Убрать». */
@Composable
private fun NodeRow(uri: String, link: NetPeer?, divider: Boolean, onRemove: (() -> Unit)? = null) {
    CardRow(divider = divider, inset = Tokens.Space.l) {
        Column(Modifier.weight(1f).padding(vertical = Tokens.Space.xxs)) {
            MonoText(uri, Color.White, Tokens.Font.preview)
            val up = link?.up == true
            val ms = link?.latencyMs ?: 0.0
            Text(
                when {
                    up && ms > 0 -> stringResource(R.string.entry_up_ms, stringResource(R.string.net_ms_value, "%.0f".format(ms)))
                    up -> stringResource(R.string.entry_up)
                    else -> stringResource(R.string.entry_down)
                },
                color = if (up) Tokens.Colors.online else Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
                modifier = Modifier.padding(top = Tokens.Space.xxs),
            )
        }
        if (onRemove != null) {
            Box(
                Modifier.size(Tokens.Size.touch).clip(RoundedCornerShape(Tokens.Radius.button))
                    .clickable(role = Role.Button, onClickLabel = stringResource(R.string.entry_remove)) { onRemove() },
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    TablerIcons.X,
                    stringResource(R.string.entry_remove),
                    tint = Tokens.Colors.fieldIcon,
                    modifier = Modifier.size(Tokens.Size.iconRow),
                )
            }
        }
    }
}

/** Плашка «Нет узлов для подключения»: что это значит и как вернуть связь одним нажатием. */
@Composable
private fun NoNodesPlate(onEnable: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.panel)
            .clip(RoundedCornerShape(Tokens.Radius.group))
            .background(Tokens.Colors.stateWarn)
            .padding(start = Tokens.Space.l, end = Tokens.Space.l, top = Tokens.Space.m),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        Icon(TablerIcons.AlertTriangle, null, tint = Tokens.Colors.stripAmber, modifier = Modifier.size(Tokens.Size.iconRow))
        Column(Modifier.weight(1f)) {
            Text(
                stringResource(R.string.entry_empty_title),
                color = Tokens.Colors.stateWarnText,
                fontSize = Tokens.Font.name,
                fontWeight = FontWeight.Medium,
            )
            Text(
                stringResource(R.string.entry_empty_sub),
                color = Tokens.Colors.stateWarnText,
                fontSize = Tokens.Font.preview,
                lineHeight = Tokens.Font.lineHeight,
                modifier = Modifier.padding(top = Tokens.Space.xxs),
            )
            Text(
                stringResource(R.string.entry_empty_action),
                color = Tokens.Colors.stateWarnLink,
                fontSize = Tokens.Font.preview,
                fontWeight = FontWeight.Medium,
                modifier = Modifier.clickable(role = Role.Button, onClick = onEnable)
                    .padding(top = Tokens.Space.s, bottom = Tokens.Space.m),
            )
        }
    }
}

/** Шторка «Добавить узел»: адрес, подсказка формата; отказ движка — под полем. */
@Composable
private fun AddNodeSheet(model: AppModel, onClose: () -> Unit) {
    var text by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    val focus = remember { FocusRequester() }
    LaunchedEffect(Unit) { focus.requestFocus() }
    val submit = {
        model.addEntryNode(text) { r -> if (r == "ok") onClose() else error = r }
    }
    SheetFrame(onClose) {
        Text(
            stringResource(R.string.entry_add),
            color = Color.White,
            fontSize = Tokens.Font.sheet,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.padding(horizontal = Tokens.Space.xs).semantics { heading() },
        )
        Column(verticalArrangement = Arrangement.spacedBy(Tokens.Space.xs)) {
            Text(
                stringResource(R.string.entry_add_field),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
                fontWeight = FontWeight.Medium,
                modifier = Modifier.padding(horizontal = Tokens.Space.xs),
            )
            val shape = RoundedCornerShape(Tokens.Radius.field)
            Box(
                Modifier.fillMaxWidth().heightIn(min = Tokens.Size.formField).clip(shape)
                    .background(Tokens.Colors.raised)
                    .border(1.dp, if (error != null) Tokens.Colors.error else Tokens.Colors.accent, shape)
                    .padding(horizontal = Tokens.Space.l),
                contentAlignment = Alignment.CenterStart,
            ) {
                if (text.isEmpty()) {
                    Text("tls://host:port", color = Tokens.Colors.tertiary, fontSize = Tokens.Font.name, fontFamily = FontFamily.Monospace)
                }
                BasicTextField(
                    text,
                    {
                        text = it
                        error = null
                    },
                    singleLine = true,
                    textStyle = Tokens.Text.base.copy(color = Color.White, fontSize = Tokens.Font.name, fontFamily = FontFamily.Monospace),
                    cursorBrush = SolidColor(Tokens.Colors.accent),
                    keyboardOptions = KeyboardOptions(
                        capitalization = KeyboardCapitalization.None,
                        autoCorrectEnabled = false,
                        keyboardType = KeyboardType.Uri,
                        imeAction = ImeAction.Done,
                    ),
                    keyboardActions = KeyboardActions(onDone = { submit() }),
                    modifier = Modifier.fillMaxWidth().focusRequester(focus),
                )
            }
            Text(
                when (error) {
                    null -> stringResource(R.string.entry_add_hint)
                    "duplicate" -> stringResource(R.string.entry_add_err_duplicate)
                    else -> stringResource(R.string.entry_add_err_invalid)
                },
                color = if (error != null) Tokens.Colors.error else Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
                modifier = Modifier.padding(horizontal = Tokens.Space.xs),
            )
        }
        Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s)) {
            PlateButton(stringResource(R.string.common_cancel), modifier = Modifier.weight(1f), onClick = onClose)
            PlateButton(stringResource(R.string.entry_add_confirm), modifier = Modifier.weight(1f), primary = true) { submit() }
        }
    }
}
