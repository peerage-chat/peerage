package chat.peerage.ui

import android.content.Context
import android.media.MediaPlayer
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.setProgress
import chat.peerage.R
import chat.peerage.Tokens
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Шаг опроса позиции играющего голосового — для волны и «0:05 / 0:12». */
internal const val VOICE_TICK_MS = 50L

/** Столбиков в волне: длиннее запись — длиннее волна, но в разумных пределах. */
internal fun voiceBars(durationMs: Int) = (10 + durationMs / 2000).coerceIn(10, 30)

/**
 * Волна: столбики громкости; проигранная часть — акцентом. Пока не посчитана — ровная черта.
 * [onSeek] (п. 68) — перемотка: тап или протяжка по волне, доля 0..1 от начала; для чтеца —
 * ползунок с прогрессом.
 */
@Composable
internal fun VoiceBars(levels: FloatArray?, bars: Int, played: Float, onSeek: ((Float) -> Unit)? = null) {
    val seek by rememberUpdatedState(onSeek)
    val label = stringResource(R.string.voice_seek)
    Row(
        Modifier.height(Tokens.Size.mediaWave)
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
            ),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.xxs),
    ) {
        repeat(bars) { i ->
            val level = levels?.getOrNull(i) ?: 0f
            val h = Tokens.Size.waveMin + (Tokens.Size.mediaWave - Tokens.Size.waveMin) * level
            val on = (i + 0.5f) / bars <= played
            Box(
                Modifier.width(Tokens.Size.waveBar).height(h)
                    .clip(RoundedCornerShape(Tokens.Radius.thumb))
                    .background(if (on) Tokens.Colors.accent else Tokens.Colors.waveIdle),
            )
        }
    }
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
