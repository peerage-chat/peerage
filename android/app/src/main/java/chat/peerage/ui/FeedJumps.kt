package chat.peerage.ui

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import chat.peerage.AppModel
import chat.peerage.ChatTarget
import chat.peerage.Message
import chat.peerage.MsgRef
import chat.peerage.R
import chat.peerage.Tokens
import chat.peerage.jumpToMention
import chat.peerage.jumpToReaction
import chat.peerage.popQuoteBack
import chat.peerage.sawMessages
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.launch

/** Сколько держится подсветка ряда после перехода. */
private const val FLASH_MS = 1600L

/**
 * Что из ленты на экране — снимается с капсулы (5.7), и у хвоста ли лента. Лента перевёрнута:
 * элемент `j` — `msgs[size - 1 - j]`. Очереди капсулы тоже в выражении: реакции приезжают из
 * движка после истории, и видимое к тому моменту сообщение иначе ждало бы прокрутки.
 */
@Composable
internal fun FeedSeen(model: AppModel, listState: LazyListState, msgs: List<Message>, chat: ChatTarget) {
    LaunchedEffect(chat, msgs) {
        snapshotFlow {
            val marks = model.feedMarks
            val seen = listState.layoutInfo.visibleItemsInfo.mapNotNull { info ->
                msgs.getOrNull(msgs.size - 1 - info.index)?.let { MsgRef(it.id, it.sender) }
            }.toSet()
            Triple(seen, listState.firstVisibleItemIndex <= 1, marks.reactions.size + marks.mentions.size + marks.below.size)
        }.distinctUntilChanged().collect { (seen, atBottom, _) ->
            model.feedAtBottom = atBottom
            model.sawMessages(chat, seen)
        }
    }
    // Подсветка после перехода гаснет сама: она показывает, куда пришли, а не метит ряд.
    val flash = model.flash
    LaunchedEffect(flash) {
        if (flash == null) return@LaunchedEffect
        delay(FLASH_MS)
        if (model.flash == flash) model.flash = null
    }
}

/**
 * Капсула переходов (макет `jump/Capsule`, app-ui.md → «Непрочитанные»): сверху вниз
 * «❤ новые реакции» · «@ упоминания» · «вниз», числа внутри, часть — только при ненулевом
 * счёте. «Вниз» сначала возвращает по стеку цитат, потом — к хвосту.
 */
@Composable
internal fun JumpCapsule(model: AppModel, listState: LazyListState, chat: ChatTarget, modifier: Modifier = Modifier) {
    val scope = rememberCoroutineScope()
    val marks = model.feedMarks
    val detached = model.detached[model.chatKey(chat.id, chat.group)] == true
    val away by remember { derivedStateOf { listState.firstVisibleItemIndex > 2 } }
    val reactions = marks.reactions.size
    val mentions = marks.mentions.size
    val down = away || detached || marks.quoteBack.isNotEmpty()
    AnimatedVisibility(reactions > 0 || mentions > 0 || down, modifier, enter = fadeIn(), exit = fadeOut()) {
        val shape = RoundedCornerShape(Tokens.Radius.jumpCapsule)
        Column(
            Modifier.padding(end = Tokens.Space.m, bottom = Tokens.Space.m)
                .width(Tokens.Size.jumpDown)
                .shadow(Tokens.Size.menuShadow, shape)
                .clip(shape)
                .background(Tokens.Colors.raised),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            if (reactions > 0) {
                CapsuleCell(TablerIcons.Heart, reactions, stringResource(R.string.jump_reactions, reactions)) {
                    model.jumpToReaction(chat)
                }
            }
            if (mentions > 0) {
                CapsuleCell(TablerIcons.At, mentions, stringResource(R.string.jump_mentions, mentions)) {
                    model.jumpToMention(chat)
                }
            }
            if (down) {
                val back = marks.quoteBack.isNotEmpty()
                val desc = stringResource(if (back) R.string.jump_back else R.string.jump_down)
                CapsuleCell(TablerIcons.ChevronDown, marks.below.size, desc) {
                    when {
                        model.popQuoteBack(chat) -> {}
                        detached -> model.toTail(chat)
                        else -> scope.launch { listState.animateScrollToItem(0) }
                    }
                }
            }
        }
    }
}

/** Ячейка капсулы: значок, под ним число акцентом (нуля не пишем). */
@Composable
private fun CapsuleCell(icon: ImageVector, count: Int, desc: String, onClick: () -> Unit) {
    Column(
        Modifier.fillMaxWidth().heightIn(min = Tokens.Size.jumpCell)
            .clickable(role = Role.Button, onClickLabel = desc, onClick = onClick)
            .padding(vertical = Tokens.Space.s),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Icon(icon, desc, tint = Tokens.Colors.iconChrome, modifier = Modifier.size(Tokens.Size.iconS))
        if (count > 0) {
            Text(
                count.toString(),
                color = Tokens.Colors.accent,
                fontSize = Tokens.Font.caption,
                fontWeight = FontWeight.SemiBold,
                style = Tokens.Text.tabular,
            )
        }
    }
}
