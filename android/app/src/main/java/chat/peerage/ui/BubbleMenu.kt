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
import androidx.compose.ui.graphics.addOutline
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import chat.peerage.AppModel
import chat.peerage.BubbleActions
import chat.peerage.MsgRef
import chat.peerage.R
import chat.peerage.Tokens
import chat.peerage.isSaved
import kotlin.math.roundToInt

/**
 * Меню у пузыря (app-ui.md → «Действия», макет `chatact/ActMenu`): фон затемнён, пузырь
 * остаётся ярким на своём месте, под ним (не влезает — над ним) меню со статусом сверху.
 * Живёт над всем экраном, у корня: пузырь сообщает, где он, через [AppModel.bubbleActions].
 * Над пузырём — полоса реакций (кроме «Сохранённого»); «+» в ней открывает полную панель
 * эмодзи, тоже после того как меню закрылось.
 *
 * Удаление из меню — одного сообщения, без режима выбора: шторка держится здесь же, после
 * того как меню закрылось.
 */
@Composable
internal fun BubbleMenuHost(model: AppModel) {
    var deleting by remember { mutableStateOf<BubbleActions?>(null) }
    var picking by remember { mutableStateOf<BubbleActions?>(null) }
    model.bubbleActions?.let { t ->
        if (t.msg.deleted) {
            // У «Сообщение удалено» одно действие — убрать у себя: меню не нужно.
            LaunchedEffect(t) {
                model.bubbleActions = null
                deleting = t
            }
        } else {
            BubbleMenuOverlay(
                model,
                t,
                onDelete = {
                    model.bubbleActions = null
                    deleting = t
                },
                onMoreReactions = {
                    model.bubbleActions = null
                    picking = t
                },
            )
        }
    }
    picking?.let { t ->
        ReactionPicker(model, onPick = { e ->
            picking = null
            model.react(t.chat, t.msg, e)
        }, onClose = { picking = null })
    }
    deleting?.let { t ->
        DeleteSheet(model, t.chat.id, t.chat.group, listOf(MsgRef(t.msg.id, t.msg.sender))) { deleting = null }
    }
}

@Composable
private fun BubbleMenuOverlay(model: AppModel, t: BubbleActions, onDelete: () -> Unit, onMoreReactions: () -> Unit) {
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
        val grow = { originY: Float ->
            Modifier.menuGrow({ shown.value }, originX = if (t.msg.mine) 1f else 0f, originY = { originY })
        }
        // Реакции — везде, и в «Сохранённом» тоже (п. 23): пометить заметку — тоже дело.
        val strip: @Composable () -> Unit = {
            ReactionStrip(
                model.quickReactions,
                model.myReaction(t.msg),
                onPick = { e ->
                    close()
                    model.react(t.chat, t.msg, e)
                },
                onMore = onMoreReactions,
                modifier = grow(1f),
            )
        }
        NearBubble(t.bounds, t.msg.mine, strip) {
            MenuSurface(grow(0f)) { BubbleMenuItems(model, t, close, onDelete) }
        }
    }
}

/** Пункты меню сообщения. Из шапки выбора ([fromSelection]) — без «Выбрать»: оно уже выбрано. */
@Composable
internal fun BubbleMenuItems(model: AppModel, t: BubbleActions, close: () -> Unit, onDelete: () -> Unit, fromSelection: Boolean = false) {
    val ctx = LocalContext.current
    val m = t.msg
    val ref = MsgRef(m.id, m.sender)
    val saved = !t.chat.group && model.isSaved(t.chat.id)
    val act = { f: () -> Unit ->
        close()
        f()
    }
    Text(
        // В «Сохранённом» доставки нет — только когда.
        statusLine(m.mine, if (saved) "" else shownStatus(model, m, t.chat.group), m.time),
        color = Tokens.Colors.tertiary,
        fontSize = Tokens.Font.body,
        modifier = Modifier.padding(
            start = Tokens.Space.l,
            end = Tokens.Space.l,
            top = Tokens.Space.m,
            bottom = if (m.editedAt > 0) Tokens.Space.xxs else Tokens.Space.m,
        ),
    )
    // Когда изменено (п. 73): в пузыре — только слово «изменено», время правки — здесь.
    if (m.editedAt > 0) {
        Text(
            stringResource(R.string.act_status, stringResource(R.string.act_edited), shortWhen(m.editedAt)),
            color = Tokens.Colors.tertiary,
            fontSize = Tokens.Font.body,
            modifier = Modifier.padding(start = Tokens.Space.l, end = Tokens.Space.l, bottom = Tokens.Space.m),
        )
    }
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
    if (!saved) {
        PopMenuItem(TablerIcons.Bookmark, stringResource(R.string.act_save)) { act { model.saveToSaved(t.chat, m) } }
    }
    // Файл — в «Загрузки» (п. 60). Фото и видео сохраняются в галерею из просмотрщика.
    if (m.blobId.isNotEmpty() && m.downloaded && m.kind != "image" && m.kind != "video") {
        val storage = rememberPermissionRunner()
        PopMenuItem(TablerIcons.Download, stringResource(R.string.act_save_downloads)) {
            // До Android 10 нужна запись в общее хранилище; меню закрывается после ответа,
            // иначе вместе с ним ушёл бы и ответ на запрос.
            if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.Q) {
                act { model.saveToDownloads(m) }
            } else {
                storage(arrayOf(android.Manifest.permission.WRITE_EXTERNAL_STORAGE), R.string.perm_denied_storage) {
                    act { model.saveToDownloads(m) }
                }
            }
        }
    }
    if (!fromSelection) {
        PopMenuItem(TablerIcons.CircleCheck, stringResource(R.string.act_select)) { act { model.toggleSelect(m) } }
    }
    PopMenuDivider()
    PopMenuItem(TablerIcons.Trash, stringResource(R.string.chat_delete), danger = true, onClick = onDelete)
}

/** Строка над пунктами: что с сообщением и когда — «Доставлено · сегодня в 14:22». */
@Composable
private fun statusLine(mine: Boolean, status: String, time: Long): String {
    val word = when {
        !mine -> R.string.act_received
        status == "read" -> R.string.act_read
        status == "delivered" -> R.string.act_delivered
        status == "sent" -> R.string.act_sent
        status == "pending" -> R.string.act_sending
        status == "queued" -> R.string.act_queued
        status == "held" -> R.string.status_held
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
    val clock = fmtMsgTime(time)
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
 * Раскладка у пузыря: всё по ведущему краю пузыря (свой — справа, чужой — слева). Обычно
 * полоса реакций над пузырём, меню под ним. Не влезает — полоса и меню встают ОДНИМ блоком
 * (полоса над меню): под пузырём, над ним, сбоку (альбомная) или у нижнего края поверх
 * пузыря. Друг на друга они не налезают никогда: меню мерится по месту, оставшемуся от
 * полосы, и прокручивается.
 */
@Composable
private fun NearBubble(bounds: Rect, mine: Boolean, strip: (@Composable () -> Unit)?, menu: @Composable () -> Unit) {
    val density = LocalDensity.current
    val insetTop = WindowInsets.systemBars.getTop(density)
    val insetBottom = WindowInsets.systemBars.getBottom(density)
    val gap = with(density) { Tokens.Space.s.roundToPx() }
    val stripGap = with(density) { Tokens.Space.m.roundToPx() }
    val edge = with(density) { Tokens.Space.m.roundToPx() }
    Layout({
        strip?.invoke()
        menu()
    }, Modifier.fillMaxSize()) { ms, c ->
        val loose = c.copy(minWidth = 0, minHeight = 0)
        val minY = insetTop + edge
        val bottomY = c.maxHeight - insetBottom - edge
        val s = if (strip != null) ms[0].measure(loose) else null
        val sH = s?.height ?: 0
        val sGap = if (s != null) gap else 0
        val p = ms.last().measure(loose.copy(maxHeight = (bottomY - minY - sH - sGap).coerceAtLeast(0)))
        layout(c.maxWidth, c.maxHeight) {
            val maxX = { w: Int -> (c.maxWidth - w - edge).coerceAtLeast(edge) }
            val xOf = { w: Int ->
                (if (mine) bounds.right.roundToInt() - w else bounds.left.roundToInt()).coerceIn(edge, maxX(w))
            }
            // Сбоку — с той стороны, где у пузыря свободно: у своего слева, у чужого справа.
            val sideOf = { w: Int -> if (mine) bounds.left.roundToInt() - gap - w else bounds.right.roundToInt() + gap }
            val top = bounds.top.roundToInt()
            val bottom = bounds.bottom.roundToInt()
            // Как задумано: полоса над пузырём, меню под ним.
            if (s != null && top - stripGap - sH >= minY && bottom + gap + p.height <= bottomY) {
                s.place(xOf(s.width), top - stripGap - sH)
                p.place(xOf(p.width), bottom + gap)
                return@layout
            }
            val blockH = sH + sGap + p.height
            val blockW = maxOf(s?.width ?: 0, p.width)
            val block = { x: (Int) -> Int, y: Int ->
                s?.place(x(s.width), y)
                p.place(x(p.width), y + sH + sGap)
            }
            val sideFits = sideOf(blockW).let { it >= edge && it <= maxX(blockW) }
            when {
                bottom + gap + blockH <= bottomY -> block(xOf, bottom + gap)
                top - gap - blockH >= minY -> block(xOf, top - gap - blockH)
                sideFits -> block(sideOf, top.coerceIn(minY, (bottomY - blockH).coerceAtLeast(minY)))
                else -> block(xOf, (bottomY - blockH).coerceAtLeast(minY))
            }
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
    // «Сохранённое»: «у всех» не бывает, но удаление всё равно подтверждается (п. 22 с телефона):
    // заметка есть только здесь, и случайный тап не должен её стирать.
    if (!group && model.isSaved(key)) {
        ActionSheet(
            pluralStringResource(R.plurals.delete_title, refs.size, refs.size),
            stringResource(R.string.saved_delete_note),
            onClose,
        ) {
            PlateButton(stringResource(R.string.chat_delete), modifier = Modifier.fillMaxWidth(), danger = true) {
                model.deleteRefs(key, false, refs)
                model.clearSelection()
                onClose()
            }
            GhostButton(stringResource(R.string.common_cancel), color = Tokens.Colors.secondary, modifier = Modifier.fillMaxWidth(), onClick = onClose)
        }
        return
    }
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
