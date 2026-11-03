package chat.peerage.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import chat.peerage.AppModel
import chat.peerage.Contact
import chat.peerage.R
import chat.peerage.Tokens

/**
 * Сверка ключа (макеты `security/Pair`, `Verified`, `Match`, `Mismatch`): один набор на пару —
 * 12 знаков и 8 слов, одинаковых на обоих телефонах; отметка «по коду» или «по набору» с датой.
 * Результат сканирования — шторкой [VerifyResultSheet], не тостом.
 */
@Composable
internal fun KeyVerifyScreen(model: AppModel, onScan: () -> Unit, onMyCode: () -> Unit) {
    val key = model.currentChat ?: return
    val c = model.contacts.firstOrNull { it.key == key } ?: return
    val name = c.displayName
    Column(Modifier.fillMaxSize()) {
        Column(
            Modifier.weight(1f).fillMaxWidth().verticalScroll(rememberScrollState())
                .padding(horizontal = Tokens.Space.panel)
                .padding(top = Tokens.Space.l),
        ) {
            if (c.verified) {
                StateBlock(
                    TablerIcons.ShieldTick,
                    Tokens.Colors.online,
                    Tokens.Colors.stateOk,
                    stringResource(R.string.profile_key_ok),
                    verifiedLine(c)?.let { stringResource(R.string.verify_ok_text, it) }
                        ?: stringResource(R.string.verify_ok_text_plain),
                )
            } else {
                StateBlock(
                    TablerIcons.ShieldQuestion,
                    Tokens.Colors.stateWarnText,
                    Tokens.Colors.stateWarn,
                    stringResource(R.string.profile_key_todo),
                    stringResource(R.string.profile_unverified_hint, name),
                )
            }
            Spacer(Modifier.heightIn(min = Tokens.Space.l))
            PairBlock(model, key, name)
            if (!c.verified) {
                Text(
                    stringResource(R.string.verify_pair_caption, name),
                    color = Tokens.Colors.tertiary,
                    fontSize = Tokens.Font.body,
                    lineHeight = Tokens.Font.lineHeight,
                    modifier = Modifier.padding(start = Tokens.Space.s - Tokens.Space.xxs, end = Tokens.Space.s - Tokens.Space.xxs, top = Tokens.Space.s),
                )
            }
        }
        Column(
            Modifier.fillMaxWidth().navigationBarsPadding()
                .padding(start = Tokens.Space.xl, end = Tokens.Space.xl, top = Tokens.Space.m, bottom = Tokens.Space.xxl),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.s),
        ) {
            if (c.verified) {
                PlateButton(stringResource(R.string.profile_recheck), modifier = Modifier.fillMaxWidth(), onClick = onScan)
                GhostButton(stringResource(R.string.verify_unmark), Tokens.Colors.iconDangerMuted, Modifier.fillMaxWidth()) {
                    model.setVerified(key, false)
                }
            } else {
                Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s)) {
                    PlateButton(stringResource(R.string.verify_scan), TablerIcons.Scan, Modifier.weight(1f), primary = true, onClick = onScan)
                    PlateButton(stringResource(R.string.verify_my_code), TablerIcons.Qrcode, Modifier.weight(1f), onClick = onMyCode)
                }
                GhostButton(stringResource(R.string.verify_mark_set), modifier = Modifier.fillMaxWidth()) {
                    model.setVerified(key, true)
                }
            }
        }
    }
}

/** «1 ноября, по коду» — когда и как сверен; null, если сверен до того, как это стали хранить. */
@Composable
internal fun verifiedLine(c: Contact): String? {
    if (!c.verified || c.verifiedAt <= 0) return null
    val day = dayLabel(c.verifiedAt)
    return when (c.verifiedHow) {
        "code" -> stringResource(R.string.verify_how_code, day)
        "set" -> stringResource(R.string.verify_how_set, day)
        else -> day
    }
}

/** Плашка состояния: щит, заголовок цветом состояния, пояснение. */
@Composable
private fun StateBlock(icon: ImageVector, color: Color, bg: Color, title: String, text: String) {
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.group)).background(bg)
            .padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.m),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = color, modifier = Modifier.size(Tokens.Size.iconL))
        Spacer(Modifier.size(Tokens.Space.m))
        Column {
            Text(title, color = color, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium)
            Text(
                text,
                color = Tokens.Colors.snippet,
                fontSize = Tokens.Font.body,
                lineHeight = Tokens.Font.lineHeight,
                modifier = Modifier.padding(top = Tokens.Space.xxs),
            )
        }
    }
}

/** Набор пары: «Алиса и этот телефон · одинаково у обоих», 12 знаков, 8 слов. */
@Composable
private fun PairBlock(model: AppModel, key: String, name: String) {
    val fp = model.pairFingerprint(key)
    Column(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.group)).background(Tokens.Colors.group)
            .padding(start = Tokens.Space.l, end = Tokens.Space.l, top = Tokens.Space.m + Tokens.Space.xxs, bottom = Tokens.Space.l),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                stringResource(R.string.verify_pair_title, name),
                color = Tokens.Colors.onDark,
                fontSize = Tokens.Font.label,
                fontWeight = FontWeight.Medium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f).padding(end = Tokens.Space.s),
            )
            Text(stringResource(R.string.verify_pair_same), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.body)
        }
        if (fp != null) {
            FingerprintGrid(fp.glyphs)
            // Слова — тот же набор голосом: по видеосвязи и при дальтонизме.
            Text(
                fp.words.joinToString(" · "),
                color = Tokens.Colors.accentSoft,
                fontSize = Tokens.Font.label,
                lineHeight = Tokens.Font.lineHeightLoose,
            )
        }
    }
}

/** Итог сканирования кода: «match» | «mismatch» | «invalid». */
@Composable
internal fun VerifyResultSheet(result: String, onMyCode: () -> Unit, onScanAgain: () -> Unit, onClose: () -> Unit) {
    SheetFrame(onClose) {
        val ok = result == "match"
        Column(
            Modifier.fillMaxWidth().padding(top = Tokens.Space.m, bottom = Tokens.Space.xs),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.s + Tokens.Space.xxs),
        ) {
            Icon(
                if (ok) TablerIcons.ShieldTick else TablerIcons.AlertTriangle,
                null,
                tint = if (ok) Tokens.Colors.online else Tokens.Colors.iconDangerMuted,
                modifier = Modifier.size(Tokens.Size.iconHero),
            )
            Text(
                stringResource(
                    when (result) {
                        "match" -> R.string.verify_match_title
                        "mismatch" -> R.string.verify_mismatch_title
                        else -> R.string.verify_invalid_title
                    },
                ),
                color = if (ok) Tokens.Colors.onDark else Tokens.Colors.errorSoft,
                fontSize = Tokens.Font.sheet,
                fontWeight = FontWeight.Bold,
            )
            Text(
                stringResource(
                    when (result) {
                        "match" -> R.string.verify_match_text
                        "mismatch" -> R.string.verify_mismatch_text
                        else -> R.string.verify_invalid_text
                    },
                ),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.preview,
                lineHeight = Tokens.Font.lineHeight,
                textAlign = TextAlign.Center,
            )
        }
        if (ok) {
            PlateButton(stringResource(R.string.verify_show_code), TablerIcons.Qrcode, Modifier.fillMaxWidth()) {
                onClose()
                onMyCode()
            }
            PlateButton(stringResource(R.string.common_done), modifier = Modifier.fillMaxWidth(), primary = true, onClick = onClose)
        } else {
            PlateButton(stringResource(R.string.verify_scan_again), modifier = Modifier.fillMaxWidth()) {
                onClose()
                onScanAgain()
            }
            PlateButton(stringResource(R.string.verify_got_it), modifier = Modifier.fillMaxWidth(), primary = true, onClick = onClose)
        }
    }
}
