package chat.peerage.ui

import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.keyframes
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.foundation.layout.Row
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.TextUnit
import chat.peerage.R

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
