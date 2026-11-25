package chat.peerage.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.zIndex
import chat.peerage.AppModel
import chat.peerage.ChatTarget
import chat.peerage.R
import chat.peerage.Tokens
import chat.peerage.isSaved

/**
 * Порядок закреплённых чатов (п. 67): строки с ручкой справа, перетаскивание — за ручку.
 * Отдельный экран, а не перетаскивание в самом списке: долгое нажатие там открывает меню, а
 * горизонтальный жест листает папки. Порядок пишется по отпусканию; он общий для всех папок
 * (закреп — пометка чата, не папки). Для чтеца экрана — действия «Выше» / «Ниже».
 */
@Composable
internal fun PinnedOrderScreen(model: AppModel) {
    val order = remember { mutableStateListOf<ChatTarget>().apply { addAll(pinnedChats(model)) } }
    var dragging by remember { mutableStateOf<ChatTarget?>(null) }
    var offset by remember { mutableFloatStateOf(0f) }
    var rowPx by remember { mutableFloatStateOf(1f) }
    val haptic = LocalHapticFeedback.current
    fun move(t: ChatTarget, by: Int): Boolean {
        val from = order.indexOf(t)
        val to = from + by
        if (from < 0 || to !in order.indices) return false
        order.add(to, order.removeAt(from))
        return true
    }
    fun save() = model.reorderPinned(order.toList())

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState())
            .padding(top = Tokens.Space.s, bottom = Tokens.Space.xxl),
    ) {
        Text(
            stringResource(R.string.pinned_order_hint),
            color = Tokens.Colors.tertiary,
            fontSize = Tokens.Font.body,
            modifier = Modifier.padding(horizontal = Tokens.Space.xl).padding(bottom = Tokens.Space.m),
        )
        CardSection {
            val up = stringResource(R.string.pinned_order_up)
            val down = stringResource(R.string.pinned_order_down)
            order.forEachIndexed { i, t ->
                key(t) {
                    val lifted = dragging == t
                    Column(
                        Modifier.fillMaxWidth()
                            .zIndex(if (lifted) 1f else 0f)
                            .graphicsLayer { translationY = if (lifted) offset else 0f }
                            .background(if (lifted) Tokens.Colors.raised else Color.Transparent)
                            .onSizeChanged { rowPx = it.height.toFloat().coerceAtLeast(1f) }
                            .semantics {
                                customActions = listOf(
                                    CustomAccessibilityAction(up) { move(t, -1).also { if (it) save() } },
                                    CustomAccessibilityAction(down) { move(t, 1).also { if (it) save() } },
                                )
                            },
                    ) {
                        if (i > 0 && !lifted) {
                            HorizontalDivider(
                                color = Tokens.Colors.divider,
                                modifier = Modifier.padding(start = Tokens.Space.l + Tokens.Size.avatarPick + Tokens.Space.m),
                            )
                        }
                        Row(
                            Modifier.padding(start = Tokens.Space.l).heightIn(min = Tokens.Size.personRow),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            ChatAvatar(model, t)
                            Spacer(Modifier.width(Tokens.Space.m))
                            Text(
                                chatFace(model, t).first,
                                color = Color.White,
                                fontSize = Tokens.Font.label,
                                fontWeight = FontWeight.Medium,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f),
                            )
                            // Ручка — вся правая часть строки по высоте: попасть пальцем легко.
                            Box(
                                Modifier.heightIn(min = Tokens.Size.personRow)
                                    .padding(horizontal = Tokens.Space.l)
                                    .pointerInput(t) {
                                        detectDragGestures(
                                            onDragStart = {
                                                haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                                                dragging = t
                                                offset = 0f
                                            },
                                            onDrag = { change, d ->
                                                change.consume()
                                                offset += d.y
                                                // Перешли за половину соседней строки — меняемся местами.
                                                if (offset > rowPx / 2 && move(t, 1)) offset -= rowPx
                                                if (offset < -rowPx / 2 && move(t, -1)) offset += rowPx
                                            },
                                            onDragEnd = {
                                                dragging = null
                                                offset = 0f
                                                save()
                                            },
                                            onDragCancel = {
                                                dragging = null
                                                offset = 0f
                                                save()
                                            },
                                        )
                                    },
                                contentAlignment = Alignment.Center,
                            ) {
                                Icon(
                                    TablerIcons.GripVertical,
                                    stringResource(R.string.cd_drag_handle),
                                    tint = Tokens.Colors.tertiary,
                                    modifier = Modifier.size(Tokens.Size.iconS),
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

/** Закреплённые чаты сверху вниз — как на главной: позже закреплён — выше, при равенстве
 *  свежее переписка. Только существующие: контакт, группа или «Сохранённое». */
private fun pinnedChats(model: AppModel): List<ChatTarget> =
    model.chatSummaries.values
        .filter { it.pinned }
        .filter { s ->
            if (s.group) {
                model.groups.any { it.gid == s.id }
            } else {
                model.isSaved(s.id) || model.contacts.any { it.key == s.id }
            }
        }
        .sortedWith(compareByDescending<chat.peerage.ChatSummary> { it.pinnedAt }.thenByDescending { it.lastTime })
        .map { ChatTarget(it.id, it.group) }
