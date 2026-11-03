package chat.peerage.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.withStyle
import chat.peerage.AppModel
import chat.peerage.Message
import chat.peerage.R
import chat.peerage.Tokens

/**
 * Системная строка группы (app-ui.md → «Чат группы», макет `special/GroupSystem`): по
 * центру ленты, без подложки. Не пузырь — она комментирует ленту со стороны.
 */
@Composable
internal fun SystemRow(model: AppModel, m: Message) {
    Box(Modifier.fillMaxWidth().padding(vertical = Tokens.Space.s), contentAlignment = Alignment.Center) {
        Text(
            groupSystemLine(model, m.kind, m.sender, m.text),
            color = Tokens.Colors.secondary,
            fontSize = Tokens.Font.body,
            textAlign = TextAlign.Center,
            modifier = Modifier.widthIn(max = Tokens.Size.systemLine).padding(horizontal = Tokens.Space.m),
        )
    }
}

/**
 * Слова системной строки по виду события (журнал движка, 2.8а): движок хранит факт —
 * вид, о ком, параметр, — а строку собирает клиент. Имена выделены: строку читают по ним.
 * Та же строка — в превью списка чатов (там берут `.text`).
 */
@Composable
internal fun groupSystemLine(model: AppModel, kind: String, sender: String, param: String): AnnotatedString {
    val who = groupSenderName(model, sender)
    return when (kind) {
        "created" -> fill(stringResource(R.string.group_sys_created), who)
        "joined" -> if (param.isEmpty()) {
            fill(stringResource(R.string.group_sys_joined_plain), who)
        } else {
            fill(stringResource(R.string.group_sys_joined), who, groupSenderName(model, param))
        }
        "left" -> fill(stringResource(R.string.group_sys_left), who)
        "renamed" -> fill(stringResource(R.string.group_sys_renamed), param)
        // «You» в английском требует своего глагола.
        "admin" -> fill(stringResource(if (sender == model.me?.pubkey) R.string.group_sys_admin_you else R.string.group_sys_admin), who)
        "photo" -> fill(stringResource(R.string.group_sys_photo))
        // Строки до 2.8а знали только «состав изменился» и кто сделал коммит.
        else -> fill(stringResource(R.string.group_system_membership), who)
    }
}

private val Arg = Regex("%(\\d)\\\$s")

/** Подставить имена в шаблон строки (`%1$s`…), выделив их. */
private fun fill(template: String, vararg names: String): AnnotatedString = buildAnnotatedString {
    var at = 0
    Arg.findAll(template).forEach { m ->
        append(template.substring(at, m.range.first))
        withStyle(SpanStyle(color = Tokens.Colors.chipText, fontWeight = FontWeight.Medium)) {
            append(names.getOrElse(m.groupValues[1].toInt() - 1) { "" })
        }
        at = m.range.last + 1
    }
    append(template.substring(at))
}
