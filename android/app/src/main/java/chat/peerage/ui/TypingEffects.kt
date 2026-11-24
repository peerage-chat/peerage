package chat.peerage.ui

import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.keyframes
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.unit.TextUnit
import chat.peerage.R

/** За сколько набранная буква проявляется из фона поля. */
private const val REVEAL_NS = 140_000_000L

/** Больше стольких символов за раз — вставка (буфер, эмодзи из панели), а не набор: сразу. */
private const val TYPED_MAX = 4

/** Поле длиннее стольких строк прокручено — где буква на экране, по разметке не понять. */
private const val REVEAL_LINES = 4

/**
 * Плавное появление набранных букв (п. 17 с телефона). Поле рисует текст как есть, а поверх
 * только что набранного — заплатку цвета фона, которая за [REVEAL_NS] становится прозрачной:
 * буква проявляется, а не выскакивает. Стирание и вставка — без эффекта.
 */
@Stable
internal class TypeReveal {
    private class Reveal(val start: Int, val end: Int, val at: Long)

    private val active = mutableStateListOf<Reveal>()
    private var now by mutableLongStateOf(0L)
    private var prev = ""

    /** Разметка текста поля (из `onTextLayout`). */
    var layout: (() -> TextLayoutResult?)? = null

    fun onText(text: String, at: Long) {
        val old = prev
        prev = text
        val added = text.length - old.length
        val p = text.commonPrefixWith(old).length
        if (added <= 0 || added > TYPED_MAX || !text.regionMatches(p + added, old, p, old.length - p)) {
            active.clear()
            return
        }
        // Вставка посреди текста сдвигает ещё проявляющиеся буквы правее неё.
        for (i in active.indices) {
            val r = active[i]
            if (r.start >= p) active[i] = Reveal(r.start + added, r.end + added, r.at)
        }
        active += Reveal(p, p + added, at)
    }

    /** Пока что-то проявляется — тикать кадрами. */
    suspend fun run() {
        snapshotFlow { active.isNotEmpty() }.collect { busy ->
            while (busy && active.isNotEmpty()) {
                withFrameNanos { }
                now = System.nanoTime()
                active.removeAll { now - it.at >= REVEAL_NS }
            }
        }
    }

    fun Modifier.revealOver(mask: Color): Modifier = drawWithContent {
        drawContent()
        val l = layout?.invoke() ?: return@drawWithContent
        if (active.isEmpty() || l.lineCount > REVEAL_LINES) return@drawWithContent
        val t = now
        for (r in active) {
            val left = 1f - ((t - r.at).toFloat() / REVEAL_NS).coerceIn(0f, 1f)
            if (left <= 0f) continue
            val a = FastOutSlowInEasing.transform(left)
            for (i in r.start until minOf(r.end, l.layoutInput.text.length)) {
                val box = l.getBoundingBox(i)
                drawRect(mask.copy(alpha = a), box.topLeft, box.size)
            }
        }
    }
}

/** [TypeReveal] поля [field], живущий, пока поле на экране. */
@Composable
internal fun rememberTypeReveal(field: TextFieldState): TypeReveal {
    val reveal = remember(field) { TypeReveal() }
    LaunchedEffect(reveal) {
        reveal.onText(field.text.toString(), 0L) // уже набранное — без эффекта
        snapshotFlow { field.text.toString() }.collect { reveal.onText(it, System.nanoTime()) }
    }
    LaunchedEffect(reveal) { reveal.run() }
    return reveal
}

/**
 * «печатает» с бегущими точками (п. 17 с телефона): точки по очереди загораются, как волна, —
 * видно, что идёт сейчас, а не застыло.
 */
@Composable
internal fun TypingLabel(color: Color, fontSize: TextUnit, modifier: Modifier = Modifier) {
    val wave = rememberInfiniteTransition(label = "typing")
    Row(modifier) {
        Text(stringResource(R.string.status_typing_word), color = color, fontSize = fontSize, maxLines = 1)
        repeat(3) { i ->
            val a by wave.animateFloat(
                initialValue = 0.25f,
                targetValue = 0.25f,
                animationSpec = infiniteRepeatable(
                    keyframes {
                        durationMillis = 1200
                        0.25f at i * 200
                        1f at i * 200 + 300
                        0.25f at i * 200 + 600
                    },
                    RepeatMode.Restart,
                ),
                label = "dot$i",
            )
            Text(".", color = color, fontSize = fontSize, modifier = Modifier.alpha(a))
        }
    }
}
