package chat.peerage.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import chat.peerage.R
import chat.peerage.Tokens

/**
 * «Витрина» профиля (макет `prof/`, вариант А): на фоне — «назад», карандаш, крупный
 * аватар, имя, статус и плитки действий; ниже — панель «листа» с группами строк.
 * Витрина едет вместе со списком: у большой группы участники длиннее экрана, а
 * прибитая шапка съела бы треть его.
 */
@Composable
internal fun ShowcaseScreen(
    onBack: () -> Unit,
    /** Карандаш справа (переименовать, изменить группу); null — его нет. */
    onEdit: (() -> Unit)?,
    editDesc: String,
    header: @Composable ColumnScope.() -> Unit,
    actions: @Composable RowScope.() -> Unit,
    content: @Composable ColumnScope.() -> Unit,
) {
    Box(Modifier.fillMaxSize().background(Tokens.Colors.bg)) {
        // Подложка листа снизу: короткое содержимое кончается раньше экрана, а лист под
        // ним должен идти до низа. Витрина и панель непрозрачны и закрывают её сверху.
        Box(Modifier.align(Alignment.BottomCenter).fillMaxWidth().fillMaxHeight(0.5f).background(Tokens.Colors.panel))
        Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState())) {
            Column(Modifier.fillMaxWidth().background(Tokens.Colors.bg)) {
                Row(
                    Modifier.fillMaxWidth().height(Tokens.Size.topBar).padding(horizontal = Tokens.Space.s),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    BarIcon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.common_back), onClick = onBack)
                    Spacer(Modifier.weight(1f))
                    if (onEdit != null) BarIcon(TablerIcons.Pencil, editDesc, onClick = onEdit)
                }
                Column(
                    Modifier.fillMaxWidth().padding(start = Tokens.Space.l, end = Tokens.Space.l, bottom = Tokens.Space.l),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
                ) {
                    header()
                    Row(
                        Modifier.fillMaxWidth().padding(top = Tokens.Space.m),
                        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.panel),
                    ) { actions() }
                }
            }
            Column(
                Modifier.fillMaxWidth()
                    .clip(RoundedCornerShape(topStart = Tokens.Radius.panel, topEnd = Tokens.Radius.panel))
                    .background(Tokens.Colors.panel)
                    .padding(top = Tokens.Space.panel, bottom = Tokens.Space.xxl),
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.m),
            ) { content() }
        }
    }
}

/** Имя на витрине: крупно, до двух строк — длинное имя переносится, а не режется. */
@Composable
internal fun ShowcaseName(name: String) {
    Text(
        name,
        color = Color.White,
        fontSize = Tokens.Font.profile,
        fontWeight = FontWeight.Medium,
        textAlign = TextAlign.Center,
        maxLines = 2,
        modifier = Modifier.padding(top = Tokens.Space.s),
    )
}

/**
 * Плитка действия под витриной. Первая — главное действие экрана (у контакта
 * «Написать», у группы «Звонок») — залита акцентом; остальные — на ступени панели.
 */
@Composable
internal fun RowScope.ShowAction(icon: ImageVector, label: String, primary: Boolean = false, onClick: () -> Unit) {
    val press = remember { MutableInteractionSource() }
    val fg = if (primary) Tokens.Colors.accentDark else Color.White
    Column(
        Modifier.weight(1f)
            .pressScale(press)
            .heightIn(min = Tokens.Size.showAction)
            .clip(RoundedCornerShape(Tokens.Radius.group))
            .background(if (primary) Tokens.Colors.accent else Tokens.Colors.panel)
            .clickable(interactionSource = press, indication = null, role = Role.Button) { onClick() }
            .padding(vertical = Tokens.Space.s),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.xs, Alignment.CenterVertically),
    ) {
        Icon(icon, null, tint = fg, modifier = Modifier.size(Tokens.Size.iconM))
        Text(label, color = fg, fontSize = Tokens.Font.small, fontWeight = FontWeight.Medium, maxLines = 1)
    }
}

/** Опасное действие профиля — красным текстом, без значка (макет `prof/`). */
@Composable
internal fun DangerTextRow(label: String, divider: Boolean = true, onClick: () -> Unit) {
    CardRow(onClick = onClick, divider = divider, inset = Tokens.Space.l) {
        Text(label, color = Tokens.Colors.destructive, fontSize = Tokens.Font.name)
    }
}
