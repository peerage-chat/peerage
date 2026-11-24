package chat.peerage.ui

import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R

/*
 * Автоудаление у себя (5.4, макеты `autodel/`). Правило живёт в движке; экран лишь
 * показывает его и спрашивает подтверждение, если новое правило что-то удалит сразу.
 * Коды — форма провода `peerage_core::autodel`: возраст `never|week|month|year`,
 * размер `any|10mb|100mb`.
 */

private val Ages = listOf("never", "week", "month", "year")
private val Sizes = listOf("any", "10mb", "100mb")

/** «Удалять старше»: Никогда · Недели · Месяца · Года. */
@Composable
private fun ageSegment(code: String): String = stringResource(
    when (code) {
        "week" -> R.string.autodel_seg_week
        "month" -> R.string.autodel_seg_month
        "year" -> R.string.autodel_seg_year
        else -> R.string.autodel_seg_never
    },
)

/** Возраст в родительном падеже для «старше …»: недели, месяца, года. */
@Composable
private fun ageGen(code: String): String = stringResource(
    when (code) {
        "week" -> R.string.autodel_gen_week
        "month" -> R.string.autodel_gen_month
        else -> R.string.autodel_gen_year
    },
)

@Composable
private fun sizeLabel(code: String): String = stringResource(
    when (code) {
        "10mb" -> R.string.autodel_size_10
        "100mb" -> R.string.autodel_size_100
        else -> R.string.autodel_size_any
    },
)

/** Строка правила в «Хранилище»: «Сообщения старше месяца · вложения от 10 МБ старше недели». */
@Composable
internal fun autodelSummary(rule: AutodelRuleDto): String {
    val parts = buildList {
        if (rule.messages != "never") add(stringResource(R.string.autodel_sum_msgs, ageGen(rule.messages)))
        if (rule.attachments != "never") {
            add(
                if (rule.minSize == "any") {
                    stringResource(R.string.autodel_sum_att, ageGen(rule.attachments))
                } else {
                    stringResource(R.string.autodel_sum_att_size, sizeLabel(rule.minSize), ageGen(rule.attachments))
                },
            )
        }
    }
    if (parts.isEmpty()) return stringResource(R.string.autodel_off)
    return parts.joinToString(" · ").replaceFirstChar { it.uppercase() }
}

/** Своё правило чата словами профиля: «Как в общих», «Никогда», «Старше недели». */
@Composable
internal fun chatAutodelValue(own: String?): String = when (own) {
    null -> stringResource(R.string.autodel_inherit)
    "never" -> stringResource(R.string.autodel_seg_never)
    else -> stringResource(R.string.autodel_older, ageGen(own))
}

/** Экран «Автоудаление»: общее правило и чаты со своим. */
@Composable
internal fun AutodelScreen(model: AppModel) {
    LaunchedEffect(Unit) { model.refreshAutodel() }
    val rule = model.autodel.rule
    // Правило, ждущее подтверждения: оно удалит что-то сразу.
    var pending by remember { mutableStateOf<Pair<AutodelRuleDto, PrunedDto>?>(null) }
    var chatSheet by remember { mutableStateOf<ChatTarget?>(null) }
    val change: (AutodelRuleDto) -> Unit = { next ->
        model.previewAutodel(next) { p ->
            if (p.messages > 0 || p.bytes > 0) pending = next to p else model.setAutodel(next)
        }
    }

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState())
            .padding(top = Tokens.Space.panel, bottom = Tokens.Space.xxl),
    ) {
        Caption(stringResource(R.string.autodel_cap))
        SectionTitle(stringResource(R.string.autodel_messages))
        CardSection {
            AgeBlock(stringResource(R.string.autodel_older_than), Ages.map { ageSegment(it) }, Ages.indexOf(rule.messages)) {
                change(rule.copy(messages = Ages[it]))
            }
        }
        SectionTitle(stringResource(R.string.autodel_attachments))
        CardSection {
            AgeBlock(stringResource(R.string.autodel_older_than), Ages.map { ageSegment(it) }, Ages.indexOf(rule.attachments)) {
                change(rule.copy(attachments = Ages[it]))
            }
            HorizontalDivider(thickness = 1.dp, color = Tokens.Colors.divider, modifier = Modifier.padding(start = Tokens.Space.l))
            AgeBlock(stringResource(R.string.autodel_min_size), Sizes.map { sizeLabel(it) }, Sizes.indexOf(rule.minSize)) {
                change(rule.copy(minSize = Sizes[it]))
            }
        }
        Caption(stringResource(R.string.autodel_att_cap), top = true)

        val own = model.autodel.chats
        if (own.isNotEmpty()) {
            SectionTitle(stringResource(R.string.autodel_own))
            CardSection {
                own.forEachIndexed { i, c ->
                    val chat = ChatTarget(c.chat, c.group)
                    val (name, _) = chatFace(model, chat)
                    CardRow(
                        onClick = { chatSheet = chat },
                        divider = i > 0,
                        inset = Tokens.Space.l + Tokens.Size.rowIcon + Tokens.Space.m,
                    ) {
                        ChatAvatar(model, chat, Tokens.Size.rowIcon)
                        Spacer(Modifier.width(Tokens.Space.m))
                        Text(name, color = Color.White, fontSize = Tokens.Font.name, modifier = Modifier.weight(1f), maxLines = 1)
                        Text(ageSegment(c.age), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.option)
                    }
                }
            }
            Caption(stringResource(R.string.autodel_own_cap), top = true)
        }
    }

    pending?.let { (next, p) ->
        AutodelConfirm(p, onOk = { model.setAutodel(next) }, onClose = { pending = null })
    }
    chatSheet?.let { chat -> ChatAutodelSheet(model, chat) { chatSheet = null } }
}

/** Подпись на панели: по вертикали с текстом в группах. */
@Composable
private fun Caption(text: String, top: Boolean = false) {
    Text(
        text,
        color = Tokens.Colors.tertiary,
        fontSize = Tokens.Font.body,
        modifier = Modifier.padding(
            start = Tokens.Space.panelText,
            end = Tokens.Space.panelText,
            top = if (top) Tokens.Space.s else 0.dp,
        ),
    )
}

/** Блок в группе: подпись и сегменты под ней (`.blk` макета). */
@Composable
private fun AgeBlock(label: String, options: List<String>, selected: Int, onSelect: (Int) -> Unit) {
    Column(
        Modifier.fillMaxWidth().padding(Tokens.Space.l),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.s + Tokens.Space.xxs),
    ) {
        Text(label, color = Color.White, fontSize = Tokens.Font.name)
        Segmented(options, selected) { if (it != selected) onSelect(it) }
    }
}

/**
 * «Удалить старые сообщения?» — правило удалит часть истории сразу (макет
 * `autodel/Confirm`). Удаление только у себя, но без возврата: поэтому шторка.
 */
@Composable
private fun AutodelConfirm(p: PrunedDto, onOk: () -> Unit, onClose: () -> Unit) {
    val n = p.messages.coerceAtMost(Int.MAX_VALUE.toLong()).toInt()
    val count = "%,d".format(p.messages)
    val size = fmtSizeLong(p.bytes)
    val what = when {
        p.messages > 0 && p.bytes > 0 -> pluralStringResource(R.plurals.autodel_confirm_both, n, count, size)
        p.messages > 0 -> pluralStringResource(R.plurals.autodel_confirm_msgs, n, count)
        else -> stringResource(R.string.autodel_confirm_att, size)
    }
    ConfirmSheet(
        title = stringResource(if (p.messages > 0) R.string.autodel_confirm_msgs_title else R.string.autodel_confirm_att_title),
        text = what + " " + stringResource(R.string.autodel_confirm_tail),
        ok = stringResource(R.string.autodel_confirm_ok),
        onOk = onOk,
        onClose = onClose,
    )
}

/**
 * «Автоудаление в этом чате» (макет `autodel/ChatRule`): как в общих, никогда или свой
 * возраст сообщений. Выбор, который удалит что-то сразу, — через подтверждение.
 */
@Composable
internal fun ChatAutodelSheet(model: AppModel, chat: ChatTarget, onClose: () -> Unit) {
    LaunchedEffect(Unit) { model.refreshAutodel() }
    val current = model.chatAutodel(chat)
    var pending by remember { mutableStateOf<Pair<String?, PrunedDto>?>(null) }
    val pick: (String?) -> Unit = { own ->
        if (own == current) {
            onClose()
        } else {
            model.previewAutodel(model.autodel.rule, chat, own) { p ->
                if (p.messages > 0 || p.bytes > 0) {
                    pending = own to p
                } else {
                    model.setChatAutodel(chat, own)
                    onClose()
                }
            }
        }
    }
    val pend = pending
    if (pend != null) {
        AutodelConfirm(pend.second, onOk = { model.setChatAutodel(chat, pend.first) }, onClose = onClose)
        return
    }
    SheetFrame(onClose) {
        Text(
            stringResource(R.string.autodel_chat_title),
            color = Color.White,
            fontSize = Tokens.Font.sheet,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.padding(horizontal = Tokens.Space.xs),
        )
        Column(Modifier.selectableGroup()) {
            val general = model.autodel.rule.messages
            OptionRow(
                stringResource(R.string.autodel_inherit),
                sub = if (general == "never") stringResource(R.string.autodel_gen_never) else stringResource(R.string.autodel_older_lc, ageGen(general)),
                on = current == null,
            ) { pick(null) }
            OptionRow(stringResource(R.string.autodel_seg_never), on = current == "never") { pick("never") }
            for (a in Ages.drop(1)) {
                OptionRow(stringResource(R.string.autodel_older, ageGen(a)), on = current == a) { pick(a) }
            }
        }
        Text(
            stringResource(R.string.autodel_chat_cap),
            color = Tokens.Colors.tertiary,
            fontSize = Tokens.Font.body,
            modifier = Modifier.padding(horizontal = Tokens.Space.xs),
        )
    }
}

/** Вариант шторки с кружком выбора справа (`.opt` макета). */
@Composable
internal fun OptionRow(text: String, sub: String? = null, on: Boolean, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = Tokens.Size.cardRow)
            .selectable(on, role = Role.RadioButton, onClick = onClick)
            .padding(horizontal = Tokens.Space.xs),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m + Tokens.Space.xxs),
    ) {
        Column(Modifier.weight(1f)) {
            Text(text, color = Color.White, fontSize = Tokens.Font.name)
            if (sub != null) {
                Text(sub, color = Tokens.Colors.tertiary, fontSize = Tokens.Font.body, modifier = Modifier.padding(top = Tokens.Space.xxs))
            }
        }
        SelectCheck(on)
    }
}

/** Строка «Автоудаление» в профиле чата (под «Медиа и файлы») со своей шторкой. */
@Composable
internal fun ProfileAutodelRow(model: AppModel, chat: ChatTarget) {
    var open by remember(chat) { mutableStateOf(false) }
    LaunchedEffect(chat) { model.refreshAutodel() }
    NavRow(
        TablerIcons.Clock,
        stringResource(R.string.autodel_title),
        value = chatAutodelValue(model.chatAutodel(chat)),
        onClick = { open = true },
    )
    if (open) ChatAutodelSheet(model, chat) { open = false }
}
