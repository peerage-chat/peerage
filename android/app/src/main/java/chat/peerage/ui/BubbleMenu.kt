package chat.peerage.ui

import androidx.activity.compose.BackHandler
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.systemBars
import androidx.compose.foundation.shape.CircleShape
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
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.ClipOp
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.addOutline
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import chat.peerage.AppModel
import chat.peerage.BubbleActions
import chat.peerage.MsgRef
import chat.peerage.R
import chat.peerage.Tokens
import kotlin.math.roundToInt

/**
 * Меню у пузыря (app-ui.md → «Действия», макет `chatact/ActMenu`): фон затемнён, пузырь
 * остаётся ярким на своём месте, под ним (не влезает — над ним) меню со статусом сверху.
 * Живёт над всем экраном, у корня: пузырь сообщает, где он, через [AppModel.bubbleActions].
 * Полоса реакций над пузырём — вместе с реакциями (шаг 2.7).
 *
 * Удаление из меню — одного сообщения, без режима выбора: шторка держится здесь же, после
 * того как меню закрылось.
 */
@Composable
internal fun BubbleMenuHost(model: AppModel) {
    var deleting by remember { mutableStateOf<BubbleActions?>(null) }
    model.bubbleActions?.let { t ->
        if (t.msg.deleted) {
            // У «Сообщение удалено» одно действие — убрать у себя: меню не нужно.
            LaunchedEffect(t) {
                model.bubbleActions = null
                deleting = t
            }
        } else {
            BubbleMenuOverlay(model, t) {
                model.bubbleActions = null
                deleting = t
            }
        }
    }
    deleting?.let { t ->
        DeleteSheet(model, t.chat.id, t.chat.group, listOf(MsgRef(t.msg.id, t.msg.sender))) { deleting = null }
    }
}

@Composable
private fun BubbleMenuOverlay(model: AppModel, t: BubbleActions, onDelete: () -> Unit) {
    val close = { model.bubbleActions = null }
    BackHandler(onBack = close)
    // Появление — короткое проявление и чуть заметный рост меню от пузыря: меню пришло
    // из него, а не из ниоткуда.
    val shown = remember(t) { Animatable(0f) }
    LaunchedEffect(t) { shown.animateTo(1f, tween(Tokens.Anim.fast)) }
    Box(
        Modifier.fillMaxSize()
            .graphicsLayer { alpha = shown.value }
            // Затемнение обходит пузырь по его форме: пузырь — то, о чём меню.
            .drawBehind {
                val hole = Path().apply {
                    addOutline(t.shape.createOutline(t.bounds.size, layoutDirection, this@drawBehind))
                    translate(t.bounds.topLeft)
                }
                clipPath(hole, ClipOp.Difference) { drawRect(Tokens.Colors.dim) }
            }
            .pointerInput(t) { detectTapGestures { close() } },
    ) {
        NearBubble(t.bounds, t.msg.mine) {
            MenuSurface(
                Modifier.graphicsLayer {
                    val s = 0.92f + 0.08f * shown.value
                    scaleX = s
                    scaleY = s
                    transformOrigin = TransformOrigin(if (t.msg.mine) 1f else 0f, 0f)
                },
            ) { BubbleMenuItems(model, t, close, onDelete) }
        }
    }
}

@Composable
private fun BubbleMenuItems(model: AppModel, t: BubbleActions, close: () -> Unit, onDelete: () -> Unit) {
    val ctx = LocalContext.current
    val m = t.msg
    val ref = MsgRef(m.id, m.sender)
    val act = { f: () -> Unit ->
        close()
        f()
    }
    Text(
        statusLine(m.mine, m.status, m.time),
        color = Tokens.Colors.tertiary,
        fontSize = Tokens.Font.body,
        modifier = Modifier.padding(start = Tokens.Space.l, end = Tokens.Space.l, top = Tokens.Space.m, bottom = Tokens.Space.m),
    )
    PopMenuDivider()
    PopMenuItem(TablerIcons.Reply, stringResource(R.string.chat_reply)) { act { model.startReply(m) } }
    if (m.text.isNotEmpty()) {
        PopMenuItem(TablerIcons.Copy, stringResource(R.string.chat_copy)) {
            act { copyToClipboard(ctx, model.plainText(m), "message") }
        }
    }
    PopMenuItem(TablerIcons.Forward, stringResource(R.string.forward_action)) {
        act {
            model.selection = setOf(ref)
            model.beginForward(t.chat.id, t.chat.group)
        }
    }
    // Закреп в группе — только у админа (как и было в шапке выбора).
    val mayPin = !t.chat.group || model.groups.firstOrNull { it.gid == t.chat.id }?.admin == true
    if (mayPin) {
        val pinned = model.pinned[t.chat.id].orEmpty().any { it.id == m.id && it.sender == m.sender }
        PopMenuItem(TablerIcons.Pin, stringResource(if (pinned) R.string.pin_remove else R.string.pin_action)) {
            act { model.setPinned(t.chat.id, t.chat.group, m, !pinned) }
        }
    }
    if (m.mine) {
        PopMenuItem(TablerIcons.Pencil, stringResource(R.string.chat_edit)) { act { model.editTarget = m } }
    }
    PopMenuItem(TablerIcons.Bookmark, stringResource(R.string.act_save)) { act { model.saveToSaved(t.chat, m) } }
    PopMenuItem(TablerIcons.CircleCheck, stringResource(R.string.act_select)) { act { model.toggleSelect(m) } }
    PopMenuDivider()
    PopMenuItem(TablerIcons.Trash, stringResource(R.string.chat_delete), danger = true, onClick = onDelete)
}

/** Строка над пунктами: что с сообщением и когда — «Доставлено · сегодня в 14:22». */
@Composable
private fun statusLine(mine: Boolean, status: String, time: Long): String {
    val word = when {
        !mine -> R.string.act_received
        status == "delivered" -> R.string.act_delivered
        status == "sent" -> R.string.act_sent
        status == "pending" -> R.string.act_sending
        status == "queued" -> R.string.act_queued
        status == "failed" -> R.string.act_failed
        else -> null
    }
    val whenText = shortWhen(time)
    return word?.let { stringResource(R.string.act_status, stringResource(it), whenText) } ?: whenText
}

/** Когда — коротко, чтобы строка статуса не переносилась: «сегодня, 14:22», «29 октября, 14:22». */
@Composable
private fun shortWhen(time: Long): String {
    val ctx = LocalContext.current
    val date = java.util.Date(time * 1000)
    val clock = android.text.format.DateFormat.getTimeFormat(ctx).format(date)
    val nowSec = System.currentTimeMillis() / 1000
    return when (dayKey(time)) {
        dayKey(nowSec) -> stringResource(R.string.act_today, clock)
        dayKey(nowSec - 86_400) -> stringResource(R.string.act_yesterday, clock)
        else -> {
            val pattern = if (yearOf(time) == yearOf(nowSec)) "d MMMM" else "d MMMM yyyy"
            val day = java.text.SimpleDateFormat(pattern, java.util.Locale.getDefault()).format(date)
            stringResource(R.string.act_on_day, day, clock)
        }
    }
}

/**
 * Раскладка меню у пузыря: по ведущему краю пузыря (свой — справа, чужой — слева), под ним;
 * не влезает снизу — над ним; не влезает нигде (пузырь во весь экран) — у нижнего края.
 */
@Composable
private fun NearBubble(bounds: Rect, mine: Boolean, content: @Composable () -> Unit) {
    val density = LocalDensity.current
    val insetTop = WindowInsets.systemBars.getTop(density)
    val insetBottom = WindowInsets.systemBars.getBottom(density)
    val gap = with(density) { Tokens.Space.s.roundToPx() }
    val edge = with(density) { Tokens.Space.m.roundToPx() }
    Layout(content, Modifier.fillMaxSize()) { ms, c ->
        val p = ms.first().measure(c.copy(minWidth = 0, minHeight = 0))
        layout(c.maxWidth, c.maxHeight) {
            val maxX = (c.maxWidth - p.width - edge).coerceAtLeast(edge)
            val x = (if (mine) bounds.right.roundToInt() - p.width else bounds.left.roundToInt()).coerceIn(edge, maxX)
            val minY = insetTop + edge
            val maxY = (c.maxHeight - insetBottom - edge - p.height).coerceAtLeast(minY)
            val below = bounds.bottom.roundToInt() + gap
            val above = bounds.top.roundToInt() - gap - p.height
            val y = when {
                below <= maxY -> below
                above >= minY -> above
                else -> maxY
            }
            p.place(x, y)
        }
    }
}

/**
 * Удаление шторкой (макет `chatact/Delete`): «у всех» — только если все свои (в группе автор
 * известен из MLS), «у себя» — со снекбаром «Отменить». «У всех» — просьба: прочитанное или
 * сохранённое не отменить, это и сказано. Удалённое выходит из выбора.
 */
@Composable
internal fun DeleteSheet(model: AppModel, key: String, group: Boolean, refs: List<MsgRef>, onClose: () -> Unit) {
    // «У всех» — только своё и ещё не отозванное.
    val retracted = refs.any { model.isRetracted(key, group, it) }
    val allMine = !retracted && refs.all { model.isMine(it) }
    val note = when {
        retracted -> R.string.delete_note_retracted
        allMine -> R.string.delete_note_all
        else -> R.string.chat_delete_note_foreign
    }
    ActionSheet(
        pluralStringResource(R.plurals.delete_title, refs.size, refs.size),
        stringResource(note),
        onClose,
    ) {
        if (allMine) {
            PlateButton(stringResource(R.string.chat_delete_for_all), modifier = Modifier.fillMaxWidth(), danger = true) {
                model.retractRefs(key, group, refs)
                model.clearSelection()
                onClose()
            }
        }
        PlateButton(stringResource(R.string.chat_delete_for_me), modifier = Modifier.fillMaxWidth()) {
            model.deleteRefs(key, group, refs)
            model.clearSelection()
            onClose()
        }
        GhostButton(stringResource(R.string.common_cancel), color = Tokens.Colors.secondary, modifier = Modifier.fillMaxWidth(), onClick = onClose)
    }
}

/** Кружок выбора у ведущего края ряда: пустой контур или акцент с галочкой. */
@Composable
internal fun SelectCheck(on: Boolean) {
    Box(
        Modifier.size(Tokens.Size.check).clip(CircleShape).then(
            if (on) {
                Modifier.background(Tokens.Colors.accent)
            } else {
                Modifier.border(Tokens.Size.checkStroke, Tokens.Colors.checkEdge, CircleShape)
            },
        ),
        contentAlignment = Alignment.Center,
    ) {
        if (on) Icon(TablerIcons.Check, null, tint = Tokens.Colors.accentDark, modifier = Modifier.size(Tokens.Size.checkMark))
    }
}

/** Подсказка на месте поля ввода, пока идёт выбор. */
@Composable
internal fun SelectHint() {
    Text(
        stringResource(R.string.select_hint),
        color = Tokens.Colors.tertiary,
        fontSize = Tokens.Font.body,
        textAlign = TextAlign.Center,
        modifier = Modifier.fillMaxWidth()
            .padding(start = Tokens.Space.m, end = Tokens.Space.m, top = Tokens.Space.s, bottom = Tokens.Space.l),
    )
}
