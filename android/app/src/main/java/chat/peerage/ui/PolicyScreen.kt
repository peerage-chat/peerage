package chat.peerage.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import chat.peerage.DocBlock
import chat.peerage.Legal
import chat.peerage.Tokens

/**
 * Политика конфиденциальности (app-ui.md → «О приложении»): текст из `assets/legal`, без
 * сети — открыть её можно и до первого подключения, и в онбординге.
 */
@Composable
internal fun PolicyScreen() {
    val ctx = LocalContext.current
    val blocks = remember { Legal.privacy(ctx) }
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState())
            .padding(horizontal = Tokens.Space.l).padding(top = Tokens.Space.s, bottom = Tokens.Space.xxl),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.s),
    ) {
        blocks.forEach { b ->
            when (b) {
                is DocBlock.Heading -> Text(
                    b.text,
                    color = Color.White,
                    fontSize = Tokens.Font.name,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.padding(top = Tokens.Space.l).semantics { heading() },
                )
                is DocBlock.Para -> Text(b.text, color = Tokens.Colors.secondary, fontSize = Tokens.Font.option, lineHeight = Tokens.Font.lineHeightLoose)
                is DocBlock.Bullet -> Row {
                    Text("•", color = Tokens.Colors.accent, fontSize = Tokens.Font.option)
                    Spacer(Modifier.width(Tokens.Space.s))
                    Text(b.text, color = Tokens.Colors.secondary, fontSize = Tokens.Font.option, lineHeight = Tokens.Font.lineHeightLoose)
                }
            }
        }
    }
}
