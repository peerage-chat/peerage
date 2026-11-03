package chat.peerage.ui

import android.Manifest
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R
import kotlinx.coroutines.delay

/** Где запись голосового: нет · держат кнопку · зафиксирована замком · остановлена, ждёт отправки. */
internal enum class VoicePhase { Idle, Holding, Locked, Stopped }

/**
 * Запись голосового (app-ui.md → «Поле ввода», макеты `chatact/VoiceHold`, `chatact/VoiceLocked`):
 * удержание пишет, отпускание отправляет; влево — отмена, вверх — замок, после замка запись
 * идёт без пальца: корзина, стоп, отправка. Один экземпляр на экран чата.
 */
@Stable
internal class VoiceSession(private val recorder: VoiceRecorder, private val onSend: (ByteArray, Int) -> Unit) {
    var phase by mutableStateOf(VoicePhase.Idle)
        private set
    var elapsedMs by mutableLongStateOf(0L)
        private set

    /** Громкость по времени (0..1) — волна в зафиксированной записи. */
    val levels = mutableStateListOf<Float>()
    private var startedAt = 0L
    private var stopped: ByteArray? = null

    val active: Boolean get() = phase != VoicePhase.Idle

    fun start(): Boolean {
        if (active || !recorder.start()) return false
        startedAt = System.currentTimeMillis()
        elapsedMs = 0
        levels.clear()
        phase = VoicePhase.Holding
        return true
    }

    fun lock() {
        if (phase == VoicePhase.Holding) phase = VoicePhase.Locked
    }

    /** Стоп в зафиксированной записи: байты готовы, уйдут по «Отправить». */
    fun stop() {
        if (phase != VoicePhase.Locked) return
        elapsedMs = System.currentTimeMillis() - startedAt
        stopped = recorder.stop()
        phase = if (stopped != null) VoicePhase.Stopped else VoicePhase.Idle
    }

    /** Отпустили кнопку или нажали «Отправить». Случайный тап (короче [MIN_MS]) — не запись. */
    fun send() {
        if (phase == VoicePhase.Holding || phase == VoicePhase.Locked) elapsedMs = System.currentTimeMillis() - startedAt
        val bytes = when (phase) {
            VoicePhase.Holding, VoicePhase.Locked -> recorder.stop().takeIf { elapsedMs >= MIN_MS }
            VoicePhase.Stopped -> stopped
            VoicePhase.Idle -> null
        }
        // Длительность едет в сообщении: иначе пузырь и «Медиа и файлы» знали бы её
        // только после первого проигрывания.
        val duration = elapsedMs.toInt()
        reset()
        bytes?.let { onSend(it, duration) }
    }

    fun cancel() {
        recorder.cancel()
        reset()
    }

    /** Шаг таймера и волны, пока пишется. */
    fun tick() {
        elapsedMs = System.currentTimeMillis() - startedAt
        levels.add(recorder.level())
        if (levels.size > MAX_LEVELS) levels.removeAt(0)
    }

    private fun reset() {
        stopped = null
        phase = VoicePhase.Idle
    }

    companion object {
        const val MIN_MS = 400L
        const val TICK_MS = 100L
        private const val MAX_LEVELS = 120
    }
}

/** Сеанс записи экрана: разрешение на микрофон спрашивается при первой попытке. */
@Composable
internal fun rememberVoiceSession(onSend: (ByteArray, Int) -> Unit): VoiceSession {
    val ctx = LocalContext.current
    val latest by rememberUpdatedState(onSend)
    val session = remember { VoiceSession(VoiceRecorder(ctx)) { bytes, ms -> latest(bytes, ms) } }
    // Экран ушёл посреди записи — микрофон отпускаем, ничего не отправляем.
    DisposableEffect(session) { onDispose { session.cancel() } }
    val recording = session.phase == VoicePhase.Holding || session.phase == VoicePhase.Locked
    LaunchedEffect(recording) {
        while (recording) {
            session.tick()
            delay(VoiceSession.TICK_MS)
        }
    }
    return session
}

/**
 * Кнопка справа от поля: отправка или микрофон. В режиме микрофона ([record]) — жест записи:
 * нажал — пишет, отпустил — ушло; увёл влево — отмена, вверх — замок. Пока держат, кнопка
 * вырастает и над ней — подсказка замка (макет `chatact/VoiceHold`).
 */
@Composable
internal fun ComposerButton(
    voice: VoiceSession?,
    record: Boolean,
    onClick: () -> Unit,
    content: @Composable BoxScope.() -> Unit,
) {
    val holding = voice?.phase == VoicePhase.Holding
    Box(Modifier.size(Tokens.Size.composer), contentAlignment = Alignment.Center) {
        Box(
            Modifier.fillMaxSize().clip(RoundedCornerShape(Tokens.Radius.field)).background(HoloAccent)
                .then(if (record && voice != null) Modifier.recordGesture(voice) else Modifier.clickable(role = Role.Button, onClick = onClick)),
            contentAlignment = Alignment.Center,
            content = content,
        )
        if (holding) {
            // Выросшая кнопка — поверх соседей: палец её закрывает, и маленькую не видно.
            Box(
                Modifier.requiredSize(Tokens.Size.voiceHold).clip(RoundedCornerShape(Tokens.Radius.voiceHold))
                    .background(HoloAccent),
                contentAlignment = Alignment.Center,
            ) { Icon(TablerIcons.Microphone, null, tint = HoloAccentDark, modifier = Modifier.size(Tokens.Size.iconL)) }
            // Замок над кнопкой: куда вести палец, чтобы писать без удержания.
            Column(
                Modifier.requiredSize(Tokens.Size.voiceLock, Tokens.Size.voiceLockH)
                    .offset(y = -(Tokens.Size.voiceHold / 2 + Tokens.Space.l + Tokens.Size.voiceLockH / 2))
                    .clip(RoundedCornerShape(50))
                    .background(Tokens.Colors.group)
                    .border(1.dp, Tokens.Colors.hairline, RoundedCornerShape(50))
                    .padding(vertical = Tokens.Space.s + Tokens.Space.xxs),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
            ) {
                Icon(TablerIcons.Lock, null, tint = Tokens.Colors.iconChrome, modifier = Modifier.size(Tokens.Size.iconS))
                Icon(TablerIcons.ChevronUp, null, tint = Tokens.Colors.tertiary, modifier = Modifier.size(Tokens.Size.iconXs))
            }
        }
    }
}

/** Жест записи на кнопке микрофона. Разрешение на микрофон — при первой попытке. */
@Composable
private fun Modifier.recordGesture(voice: VoiceSession): Modifier {
    val ctx = LocalContext.current
    val haptic = LocalHapticFeedback.current
    val micPerm = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {}
    return pointerInput(voice) {
        val cancelAt = Tokens.Size.voiceSlide.toPx()
        awaitEachGesture {
            val down = awaitFirstDown()
            if (!ctx.hasPermission(Manifest.permission.RECORD_AUDIO)) {
                micPerm.launch(Manifest.permission.RECORD_AUDIO)
                return@awaitEachGesture
            }
            if (!voice.start()) return@awaitEachGesture
            haptic.performHapticFeedback(HapticFeedbackType.LongPress)
            while (true) {
                val ch = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                val d = ch.position - down.position
                ch.consume()
                when {
                    !ch.pressed -> {
                        voice.send()
                        break
                    }
                    d.x < -cancelAt -> {
                        voice.cancel()
                        break
                    }
                    d.y < -cancelAt -> {
                        voice.lock()
                        haptic.performHapticFeedback(HapticFeedbackType.LongPress)
                        break
                    }
                }
            }
        }
    }
}

/** Красная точка и время записи — общие для удержания и замка. */
@Composable
private fun RecTimer(voice: VoiceSession) {
    Box(
        Modifier.size(Tokens.Size.recDot).clip(CircleShape)
            .background(if (voice.phase == VoicePhase.Stopped) Tokens.Colors.tertiary else Tokens.Colors.error),
    )
    Spacer(Modifier.width(Tokens.Space.m))
    Text(
        fmtDuration(voice.elapsedMs.toInt()),
        style = Tokens.Text.base.copy(color = Tokens.Colors.onDark, fontFeatureSettings = "tnum"),
    )
}

/** Содержимое поля, пока кнопку держат: точка, время, «Влево — отмена». */
@Composable
internal fun RowScope.VoiceHoldField(voice: VoiceSession) {
    Spacer(Modifier.width(Tokens.Space.m))
    RecTimer(voice)
    Row(Modifier.weight(1f), horizontalArrangement = Arrangement.Center, verticalAlignment = Alignment.CenterVertically) {
        Icon(TablerIcons.ChevronLeft, null, tint = Tokens.Colors.tertiary, modifier = Modifier.size(Tokens.Size.iconXs))
        Spacer(Modifier.width(Tokens.Space.xs))
        Text(stringResource(R.string.voice_cancel_hint), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.preview)
    }
}

/**
 * Зафиксированная запись (макет `chatact/VoiceLocked`): корзина · поле с точкой, временем,
 * волной и «стоп» · отправить. Заменяет всю строку ввода.
 */
@Composable
internal fun VoiceLockedRow(voice: VoiceSession) {
    Row(
        Modifier.fillMaxWidth().padding(start = Tokens.Space.m, end = Tokens.Space.m, top = Tokens.Space.s, bottom = Tokens.Space.l),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.size(Tokens.Size.touch).clip(RoundedCornerShape(Tokens.Radius.field))
                .clickable(role = Role.Button) { voice.cancel() },
            contentAlignment = Alignment.Center,
        ) {
            Box(
                Modifier.size(Tokens.Size.ib).clip(RoundedCornerShape(Tokens.Radius.button)).background(Tokens.Colors.group),
                contentAlignment = Alignment.Center,
            ) {
                Icon(TablerIcons.Trash, stringResource(R.string.voice_delete), tint = Tokens.Colors.failText, modifier = Modifier.size(Tokens.Size.iconS))
            }
        }
        Spacer(Modifier.width(Tokens.Space.xs))
        Row(
            Modifier.weight(1f).height(Tokens.Size.composer).clip(RoundedCornerShape(Tokens.Radius.field))
                .background(Tokens.Colors.group).padding(start = Tokens.Space.m),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            RecTimer(voice)
            Spacer(Modifier.width(Tokens.Space.m))
            Waveform(voice.levels, Modifier.weight(1f).height(Tokens.Size.waveH))
            if (voice.phase == VoicePhase.Locked) {
                Box(
                    Modifier.size(Tokens.Size.touch).clickable(role = Role.Button) { voice.stop() },
                    contentAlignment = Alignment.Center,
                ) { Icon(TablerIcons.PlayerStop, stringResource(R.string.voice_stop), tint = Tokens.Colors.iconChrome, modifier = Modifier.size(Tokens.Size.iconS)) }
            } else {
                Spacer(Modifier.width(Tokens.Space.m))
            }
        }
        Spacer(Modifier.width(Tokens.Space.s))
        Box(
            Modifier.size(Tokens.Size.composer).clip(RoundedCornerShape(Tokens.Radius.field)).background(HoloAccent)
                .clickable(role = Role.Button) { voice.send() },
            contentAlignment = Alignment.Center,
        ) { Icon(TablerIcons.Send, stringResource(R.string.common_send), tint = HoloAccentDark, modifier = Modifier.size(Tokens.Size.iconS)) }
    }
}

/** Волна записи: столбики по громкости, свежие справа; не влезающие старые уходят влево. */
@Composable
private fun Waveform(levels: List<Float>, modifier: Modifier) {
    val color = Tokens.Colors.secondary
    Canvas(modifier) {
        val bar = Tokens.Size.waveBar.toPx()
        val step = bar + Tokens.Space.xxs.toPx()
        val minH = Tokens.Size.waveMin.toPx()
        val fit = (size.width / step).toInt()
        val shown = levels.takeLast(fit)
        shown.forEachIndexed { i, v ->
            val h = minH + (size.height - minH) * v
            drawRoundRect(
                color,
                topLeft = Offset(size.width - (shown.size - i) * step, (size.height - h) / 2),
                size = Size(bar, h),
                cornerRadius = CornerRadius(bar / 2),
            )
        }
    }
}
