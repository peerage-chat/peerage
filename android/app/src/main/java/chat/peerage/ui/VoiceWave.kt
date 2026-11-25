package chat.peerage.ui

import android.content.Context
import android.media.MediaPlayer
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.height
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.setProgress
import androidx.compose.ui.unit.Dp
import chat.peerage.R
import chat.peerage.Tokens
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Шаг опроса позиции играющего голосового — для волны и «0:05 / 0:12». */
internal const val VOICE_TICK_MS = 50L

/**
 * Столбиков в волне при её естественной длине: длиннее запись — длиннее волна, но в
 * разумных пределах (34 — пузырь телефона ещё вмещает кнопку, волну и время).
 */
internal fun voiceBars(durationMs: Int) = (20 + durationMs / 2000).coerceIn(20, 34)

/** Естественная длина волны из [bars] столбиков. */
private fun waveWidth(bars: Int): Dp = Tokens.Size.waveBar * bars + Tokens.Space.xxs * (bars - 1)

/**
 * Волна: столбики громкости; проигранная часть — акцентом. Пока не посчитана — ровная черта.
 * Длина — естественная ([bars] столбиков) или больше, если [modifier] растягивает (в пузыре —
 * на всю его ширину, п. 79): тогда столбиков больше, а [levels] берутся ближайшие.
 * [onSeek] (п. 68) — перемотка: тап или протяжка по волне, доля 0..1 от начала; для чтеца —
 * ползунок с прогрессом.
 */
@Composable
internal fun VoiceBars(
    levels: FloatArray?,
    bars: Int,
    played: Float,
    onSeek: ((Float) -> Unit)? = null,
    modifier: Modifier = Modifier,
) {
    val seek by rememberUpdatedState(onSeek)
    val label = stringResource(R.string.voice_seek)
    val accent = Tokens.Colors.accent
    val idle = Tokens.Colors.waveIdle
    Box(
        modifier.defaultMinSize(minWidth = waveWidth(bars))
            .height(Tokens.Size.mediaWave)
            .then(
                if (onSeek == null) {
                    Modifier
                } else {
                    Modifier
                        .pointerInput(Unit) {
                            detectTapGestures { seek?.invoke((it.x / size.width).coerceIn(0f, 1f)) }
                        }
                        .pointerInput(Unit) {
                            detectHorizontalDragGestures { change, _ ->
                                change.consume()
                                seek?.invoke((change.position.x / size.width).coerceIn(0f, 1f))
                            }
                        }
                        .semantics {
                            contentDescription = label
                            setProgress { f ->
                                seek?.invoke(f.coerceIn(0f, 1f))
                                true
                            }
                        }
                },
            )
            .drawBehind {
                val bar = Tokens.Size.waveBar.toPx()
                val gap = Tokens.Space.xxs.toPx()
                val minH = Tokens.Size.waveMin.toPx()
                val n = ((size.width + gap) / (bar + gap)).toInt().coerceAtLeast(1)
                val step = if (n > 1) (size.width - bar) / (n - 1) else 0f
                val r = CornerRadius(Tokens.Radius.thumb.toPx())
                for (j in 0 until n) {
                    val level = levels?.getOrNull(j * bars / n) ?: 0f
                    val h = minH + (size.height - minH) * level
                    drawRoundRect(
                        color = if ((j + 0.5f) / n <= played) accent else idle,
                        topLeft = Offset(j * step, (size.height - h) / 2),
                        size = Size(bar, h),
                        cornerRadius = r,
                    )
                }
            },
    )
}

/**
 * Плеер голосового: копия байтов — в `viewer/` (расшифрованное оттуда убирается на старте),
 * имя — по блобу (два голосовых одной длины не подменяют друг друга). Не на главном потоке;
 * null — звук не разобрался.
 */
internal suspend fun prepareVoicePlayer(ctx: Context, blobId: String, bytes: ByteArray): MediaPlayer? =
    withContext(Dispatchers.IO) {
        runCatching {
            val dir = java.io.File(ctx.cacheDir, "viewer").apply { mkdirs() }
            val f = java.io.File(dir, "voice_$blobId.m4a").apply { writeBytes(bytes) }
            MediaPlayer().apply {
                setDataSource(f.absolutePath)
                prepare()
            }
        }.getOrNull()
    }
