package chat.peerage.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBars
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsBottomHeight
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import chat.peerage.AppModel
import chat.peerage.ChatTarget
import chat.peerage.Message
import chat.peerage.R
import chat.peerage.Tokens

/**
 * Полоса реакций над пузырём: шесть частых своих ([ReactionUsage]) и «+» — полная панель. Своя реакция
 * подсвечена: нажатие на неё снимает.
 */
@Composable
internal fun ReactionStrip(quick: List<String>, mine: String?, onPick: (String) -> Unit, onMore: () -> Unit, modifier: Modifier = Modifier) {
    val shape = RoundedCornerShape(percent = 50)
    Row(
        modifier.shadow(Tokens.Size.menuShadow, shape)
            .clip(shape)
            .background(Tokens.Colors.group)
            .border(1.dp, Tokens.Colors.menuEdge, shape)
            .padding(Tokens.Space.xs),
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xxs),
    ) {
        quick.forEach { e ->
            val label = stringResource(R.string.react_with, e)
            Box(
                Modifier.size(Tokens.Size.reactButton)
                    .clip(CircleShape)
                    .background(if (e == mine) Tokens.Colors.accentMuted else Color.Transparent)
                    .clickable(role = Role.Button) { onPick(e) }
                    .clearAndSetSemantics {
                        contentDescription = label
                        role = Role.Button
                    },
                contentAlignment = Alignment.Center,
            ) { Text(e, fontSize = Tokens.Font.emojiStrip) }
        }
        Box(
            Modifier.size(Tokens.Size.reactButton)
                .clip(CircleShape)
                .background(Tokens.Colors.raised)
                .clickable(role = Role.Button) { onMore() },
            contentAlignment = Alignment.Center,
        ) {
            Icon(
                TablerIcons.Plus,
                stringResource(R.string.react_more),
                tint = Tokens.Colors.iconChrome,
                modifier = Modifier.size(Tokens.Size.iconS),
            )
        }
    }
}

/**
 * Полная панель эмодзи для реакции («+» в полосе): та же, что у поля ввода, но без
 * забоя — стирать нечего. Выбор сразу ставит реакцию.
 */
@Composable
internal fun ReactionPicker(model: AppModel, onPick: (String) -> Unit, onClose: () -> Unit) {
    BackHandler(onBack = onClose)
    Box(
        Modifier.fillMaxSize()
            .background(Tokens.Colors.dim)
            .pointerInput(Unit) { detectTapGestures { onClose() } },
        contentAlignment = Alignment.BottomCenter,
    ) {
        // Касание по самой панели мимо эмодзи не должно закрывать её как касание фона.
        Column(Modifier.fillMaxWidth().pointerInput(Unit) { detectTapGestures { } }) {
            EmojiPanel(
                recent = model.recentEmoji,
                onPick = { e ->
                    model.useEmoji(e)
                    onPick(e)
                },
                onBackspace = null,
                onClose = onClose,
            )
            Spacer(Modifier.fillMaxWidth().background(Tokens.Colors.surface).windowInsetsBottomHeight(WindowInsets.navigationBars))
        }
    }
}

/**
 * Чипы реакций под пузырём (макет `chatact/Reactions`): «эмодзи · число», своя — обведена.
 * Нажатие ставит эту реакцию или снимает свою; долгое — шторка «Реакции» с тем, кто поставил.
 */
@OptIn(ExperimentalLayoutApi::class, ExperimentalFoundationApi::class)
@Composable
internal fun ReactionChips(model: AppModel, m: Message, chat: ChatTarget) {
    // Открытая вкладка шторки: null — закрыта, "" — «Все».
    var sheet by remember { mutableStateOf<String?>(null) }
    val mine = model.myReaction(m)
    val chipShape = RoundedCornerShape(percent = 50)
    FlowRow(
        Modifier.widthIn(max = LocalBubbleMax.current).padding(top = Tokens.Space.xs),
        horizontalArrangement = Arrangement.spacedBy(
            Tokens.Space.xs,
            if (m.mine) Alignment.End else Alignment.Start,
        ),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
    ) {
        // Пришли капсулой «❤» (5.7): чужие реакции обведены — видно, какая новая.
        val flashed = model.flash?.let { it.reaction && it.ref.id == m.id && it.ref.sender == m.sender } == true
        val me = model.me?.pubkey
        m.reactions.forEach { r ->
            val own = r.emoji == mine
            val fresh = flashed && r.who.any { it != me }
            val label = pluralStringResource(R.plurals.react_chip, r.who.size, r.emoji, r.who.size)
            val reactionsLabel = stringResource(R.string.react_title)
            Row(
                Modifier.heightIn(min = Tokens.Size.reactChip)
                    .clip(chipShape)
                    .background(if (own) Tokens.Colors.accentMuted else Tokens.Colors.bubbleIn)
                    .then(
                        when {
                            fresh -> Modifier.border(1.dp, Tokens.Colors.accentSoft, chipShape)
                            own -> Modifier.border(1.dp, Tokens.Colors.accent, chipShape)
                            else -> Modifier
                        },
                    )
                    .combinedClickable(
                        role = Role.Button,
                        onLongClickLabel = reactionsLabel,
                        onLongClick = { sheet = r.emoji },
                        onClick = { model.react(chat, m, r.emoji) },
                    )
                    .clearAndSetSemantics {
                        contentDescription = label
                        role = Role.Button
                    }
                    .padding(horizontal = Tokens.Space.s + 1.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
            ) {
                Text(r.emoji, fontSize = Tokens.Font.emojiChip)
                Text(
                    r.who.size.toString(),
                    color = if (own) Tokens.Colors.accentSoft else Tokens.Colors.chipText,
                    fontSize = Tokens.Font.body,
                    style = Tokens.Text.tabular,
                )
            }
        }
    }
    sheet?.let { start -> ReactionsSheet(model, m, chat, start) { sheet = null } }
}

/**
 * Шторка «Реакции» (макет `chatact/ReactWho`): вкладки «Все N» и по эмодзи, ниже — кто
 * поставил. Своя строка первой; нажатие на неё снимает реакцию.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ReactionsSheet(model: AppModel, m: Message, chat: ChatTarget, start: String, onClose: () -> Unit) {
    var tab by remember { mutableStateOf(start) }
    val me = model.me?.pubkey
    val all = m.reactions.flatMap { r -> r.who.map { it to r.emoji } }.sortedByDescending { it.first == me }
    // Последнюю реакцию сняли, пока шторка открыта, — показывать нечего.
    LaunchedEffect(all.isEmpty()) { if (all.isEmpty()) onClose() }
    // Вкладка эмодзи, которого больше нет, — значит «Все».
    val active = tab.takeIf { t -> m.reactions.any { it.emoji == t } } ?: ""
    val shown = if (active.isEmpty()) all else all.filter { it.second == active }
    ModalBottomSheet(
        onDismissRequest = onClose,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        containerColor = Tokens.Colors.group,
        tonalElevation = 0.dp,
        shape = RoundedCornerShape(topStart = Tokens.Radius.panel, topEnd = Tokens.Radius.panel),
        dragHandle = { SheetHandle() },
    ) {
        Column(
            Modifier.fillMaxWidth().navigationBarsPadding()
                .padding(start = Tokens.Space.l, end = Tokens.Space.l, bottom = Tokens.Space.xl),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.m),
        ) {
            Text(
                stringResource(R.string.react_title),
                color = Color.White,
                fontSize = Tokens.Font.sheet,
                fontWeight = FontWeight.Bold,
                modifier = Modifier.padding(horizontal = Tokens.Space.xs).semantics { heading() },
            )
            Row(
                Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs + Tokens.Space.xxs),
            ) {
                ReactionTab(active.isEmpty(), { tab = "" }) {
                    Text(stringResource(R.string.react_all, all.size), fontSize = Tokens.Font.preview, color = it)
                }
                m.reactions.forEach { r ->
                    ReactionTab(active == r.emoji, { tab = r.emoji }) {
                        Text(r.emoji, fontSize = Tokens.Font.emojiTab)
                        Text(r.who.size.toString(), fontSize = Tokens.Font.preview, color = it, style = Tokens.Text.tabular)
                    }
                }
            }
            Column(Modifier.weight(1f, fill = false).verticalScroll(rememberScrollState())) {
                shown.forEach { (who, emoji) ->
                    val own = who == me
                    Row(
                        Modifier.fillMaxWidth().heightIn(min = Tokens.Size.navRow)
                            .clip(RoundedCornerShape(Tokens.Radius.inner))
                            .then(if (own) Modifier.clickable(role = Role.Button) { model.react(chat, m, emoji) } else Modifier)
                            .padding(horizontal = Tokens.Space.xs),
                        verticalAlignment = Alignment.CenterVertically,
                        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
                    ) {
                        // Буква аватара — от имени, а не от слова «Вы».
                        SquareAvatar(
                            if (own) model.selfName.ifBlank { groupSenderName(model, who) } else groupSenderName(model, who),
                            size = Tokens.Size.avatarList,
                            avatarBytes = model.peerAvatar(who),
                            circle = true,
                            ring = Tokens.Colors.group,
                        )
                        Text(
                            groupSenderName(model, who),
                            color = Color.White,
                            fontSize = Tokens.Font.label,
                            fontWeight = FontWeight.Medium,
                            maxLines = 1,
                            modifier = Modifier.weight(1f),
                        )
                        Text(emoji, fontSize = Tokens.Font.emojiRow)
                    }
                }
            }
        }
    }
}

/** Вкладка шторки «Реакции»: выбранная — на приглушённом акценте. [content] получает цвет текста. */
@Composable
private fun ReactionTab(on: Boolean, onClick: () -> Unit, content: @Composable (Color) -> Unit) {
    Row(
        Modifier.heightIn(min = Tokens.Size.reactTab)
            .clip(RoundedCornerShape(Tokens.Radius.pill))
            .background(if (on) Tokens.Colors.accentMuted else Tokens.Colors.raised)
            .clickable(role = Role.Tab) { onClick() }
            .padding(horizontal = Tokens.Space.m),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xs + Tokens.Space.xxs),
    ) { content(if (on) Tokens.Colors.accentSoft else Tokens.Colors.chipText) }
}
