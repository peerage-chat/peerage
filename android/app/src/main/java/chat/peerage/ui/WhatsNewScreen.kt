package chat.peerage.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import chat.peerage.Changelog
import chat.peerage.ChangelogEntry
import chat.peerage.R
import chat.peerage.Tokens

/**
 * «Что нового» (п. 51, app-ui.md → «Что нового»): журнал до установленной версии, свежая
 * сверху. Открыли — полоса «Обновлено до X» на главной больше не нужна.
 */
@Composable
internal fun WhatsNewScreen() {
    val ctx = LocalContext.current
    val entries = remember { Changelog.load(ctx) }
    LaunchedEffect(Unit) { Changelog.seen() }
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(top = Tokens.Space.s, bottom = Tokens.Space.xxl),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.l),
    ) {
        entries.forEach { VersionCard(it) }
    }
}

@Composable
private fun VersionCard(e: ChangelogEntry) {
    CardSection {
        Column(Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.m)) {
            Row(verticalAlignment = androidx.compose.ui.Alignment.Bottom) {
                Text(e.version, color = Color.White, fontSize = Tokens.Font.name, fontWeight = FontWeight.SemiBold)
                Spacer(Modifier.width(Tokens.Space.s))
                Text(longDate(e.date), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.body)
            }
            Text(
                e.summary,
                color = Tokens.Colors.secondary,
                fontSize = Tokens.Font.option,
                lineHeight = Tokens.Font.lineHeightLoose,
                modifier = Modifier.padding(top = Tokens.Space.xs),
            )
            Points(stringResource(R.string.whatsnew_new), e.new)
            Points(stringResource(R.string.whatsnew_fixed), e.fixed)
        }
    }
}

/** Подзаголовок и пункты; пустой список — ничего. */
@Composable
private fun Points(title: String, items: List<String>) {
    if (items.isEmpty()) return
    Text(
        title,
        color = Tokens.Colors.tertiary,
        fontSize = Tokens.Font.body,
        fontWeight = FontWeight.Medium,
        modifier = Modifier.padding(top = Tokens.Space.m, bottom = Tokens.Space.xs),
    )
    items.forEach { item ->
        Row(Modifier.padding(vertical = Tokens.Space.xxs)) {
            Text("•", color = Tokens.Colors.accent, fontSize = Tokens.Font.option)
            Spacer(Modifier.width(Tokens.Space.s))
            Text(item, color = Color.White, fontSize = Tokens.Font.option, lineHeight = Tokens.Font.lineHeightLoose)
        }
    }
}

/** «2026-11-25» → «25 ноября 2026 г.» в языке интерфейса; не разобралось — как есть. */
@Composable
private fun longDate(iso: String): String {
    val ctx = LocalContext.current
    return remember(iso) {
        runCatching {
            val d = java.text.SimpleDateFormat("yyyy-MM-dd", java.util.Locale.US).parse(iso)!!
            java.text.DateFormat.getDateInstance(java.text.DateFormat.LONG, ctx.resources.configuration.locales[0]).format(d)
        }.getOrDefault(iso)
    }
}
