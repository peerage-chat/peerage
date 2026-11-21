package chat.peerage.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import chat.peerage.*
import chat.peerage.R

/**
 * Поток трафика и смена адреса (ADR-007; макеты `misc/Flood`, `misc/Rotate`,
 * `misc/RotateProgress`). Остановить чужие пакеты на устройстве нельзя — трафик за них
 * уже списан; экран называет цену и два действия, которые реально помогают.
 */

/** Поток трафика: темп, потраченное, источники и два действия. Счёт — с запуска. */
@Composable
internal fun FloodScreen(model: AppModel, onRotate: () -> Unit) {
    val list = model.floodPeers
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState())
            .padding(top = Tokens.Space.l, bottom = Tokens.Space.xxl),
    ) {
        Row(
            Modifier.padding(horizontal = Tokens.Space.panel),
            horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
        ) {
            val tile = Modifier.weight(1f)
            StatTile(rateOf(list.sumOf { it.bps }), stringResource(R.string.flood_tile_now), tile, bg = Tokens.Colors.group, size = Tokens.Font.sheet)
            StatTile(fmtSizeLong(list.sumOf { it.bytes }), stringResource(R.string.flood_tile_spent), tile, bg = Tokens.Colors.group, size = Tokens.Font.sheet)
            StatTile(
                "${list.size}",
                pluralStringResource(R.plurals.flood_tile_sources, list.size),
                tile,
                bg = Tokens.Colors.group,
                size = Tokens.Font.sheet,
            )
        }
        Caption(stringResource(R.string.flood_explain))

        if (list.isNotEmpty()) {
            SectionTitle(stringResource(R.string.flood_sources))
            CardSection {
                list.forEachIndexed { i, f ->
                    SourceRow(f, model.contacts.firstOrNull { it.key == f.peer }?.displayName, divider = i > 0)
                }
            }
        }

        SectionTitle(stringResource(R.string.flood_what_to_do))
        CardSection {
            ActionRow(
                TablerIcons.WifiOff,
                stringResource(R.string.flood_go_offline),
                stringResource(R.string.flood_go_offline_desc),
                divider = false,
            ) {
                if (model.offline) {
                    PillButton(stringResource(R.string.net_go_online)) { model.goOnline() }
                } else {
                    PillButton(stringResource(R.string.flood_offline_btn), neutral = true) { model.goOffline() }
                }
            }
            ActionRow(
                TablerIcons.SwitchHorizontal,
                stringResource(R.string.flood_rotate),
                stringResource(R.string.flood_rotate_desc),
            ) {
                PillButton(stringResource(R.string.flood_rotate_pill), onClick = onRotate)
            }
        }
    }
}

/** Темп словами: «2,4 МБ/с»; стихший поток — «0 Б/с». */
@Composable
private fun rateOf(bps: Long): String = stringResource(R.string.net_rate, fmtSizeLong(bps))

/** Источник: контакт — именем, незнакомец — ключом; темп и потраченное, пометка блока. */
@Composable
private fun SourceRow(f: FloodPeer, name: String?, divider: Boolean) {
    val volume = if (f.bps > 0) "${rateOf(f.bps)} · ${fmtSizeLong(f.bytes)}" else fmtSizeLong(f.bytes)
    CardRow(divider = divider, inset = PersonInset, modifier = Modifier.heightIn(min = Tokens.Size.personRow)) {
        KeyOrAvatar(name)
        Spacer(Modifier.width(Tokens.Space.m + Tokens.Space.xxs))
        Column(Modifier.weight(1f)) {
            Text(
                name ?: stringResource(R.string.blocked_key, f.peer.take(8)),
                color = Color.White,
                fontSize = Tokens.Font.label,
                fontWeight = FontWeight.Medium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                if (name == null) stringResource(R.string.flood_src_stranger, volume) else volume,
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
                modifier = Modifier.padding(top = Tokens.Space.xxs),
            )
        }
        if (f.blocked) {
            Spacer(Modifier.width(Tokens.Space.s))
            Text(
                stringResource(R.string.flood_blocked_tag),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.small,
                modifier = Modifier.clip(RoundedCornerShape(Tokens.Radius.chip)).background(Tokens.Colors.raised)
                    .padding(horizontal = Tokens.Space.s, vertical = Tokens.Space.xxs),
            )
        }
    }
}

/** Аватар контакта или ключ на плитке raised — у незнакомца имени нет. */
@Composable
private fun KeyOrAvatar(name: String?) {
    if (name != null) {
        SquareAvatar(name, size = Tokens.Size.avatarPick, circle = true)
    } else {
        SquareAvatar("", size = Tokens.Size.avatarPick, circle = true, bg = Tokens.Colors.raised) {
            Icon(TablerIcons.Key, null, tint = Tokens.Colors.fieldIcon, modifier = Modifier.size(Tokens.Size.iconRow))
        }
    }
}

/** Действие: иконка, название, последствие и пилюля справа. */
@Composable
private fun ActionRow(
    icon: ImageVector,
    title: String,
    desc: String,
    divider: Boolean = true,
    pill: @Composable () -> Unit,
) {
    CardRow(divider = divider, inset = Tokens.Space.l) {
        Icon(icon, null, tint = Tokens.Colors.fieldIcon, modifier = Modifier.size(Tokens.Size.iconRow))
        Spacer(Modifier.width(Tokens.Space.m + Tokens.Space.xxs))
        Column(Modifier.weight(1f).padding(vertical = Tokens.Space.xs)) {
            Text(title, color = Color.White, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium)
            Text(
                desc,
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
                lineHeight = Tokens.Font.lineHeight,
                modifier = Modifier.padding(top = Tokens.Space.xxs),
            )
        }
        Spacer(Modifier.width(Tokens.Space.m))
        pill()
    }
}

/**
 * Смена адреса: кому сообщить новый. Адрес меняется для всех, узнают только отмеченные —
 * не отмеченный теряет связь. Источник потока снят заранее, но выбор за человеком.
 */
@Composable
internal fun RotateAddressScreen(model: AppModel, onDone: () -> Unit) {
    val flooders = model.floodPeers.map { it.peer }.toSet()
    val selected = remember {
        mutableStateMapOf<String, Boolean>().apply {
            model.contacts.forEach { put(it.key, it.key !in flooders) }
        }
    }
    val chosen = model.contacts.filter { selected[it.key] == true }

    Column(Modifier.fillMaxSize()) {
        Column(
            Modifier.weight(1f).verticalScroll(rememberScrollState())
                .padding(top = Tokens.Space.s, bottom = Tokens.Space.l),
        ) {
            Caption(stringResource(R.string.rotate_explain))
            if (model.contacts.isNotEmpty()) {
                Row(
                    Modifier.padding(
                        start = Tokens.Space.panelText,
                        end = Tokens.Space.panelText,
                        top = Tokens.Space.l,
                        bottom = Tokens.Space.s,
                    ),
                ) {
                    SectionText(stringResource(R.string.rotate_tell_whom), Modifier.weight(1f))
                    SectionText(stringResource(R.string.rotate_count, chosen.size, model.contacts.size))
                }
                CardSection {
                    model.contacts.forEachIndexed { i, c ->
                        val on = selected[c.key] == true
                        CardRow(
                            divider = i > 0,
                            inset = PersonInset,
                            modifier = Modifier.heightIn(min = Tokens.Size.personRow)
                                .clickable(role = Role.Checkbox) { selected[c.key] = !on },
                        ) {
                            SquareAvatar(c.displayName, size = Tokens.Size.avatarPick, circle = true)
                            Spacer(Modifier.width(Tokens.Space.m + Tokens.Space.xxs))
                            Column(Modifier.weight(1f)) {
                                Text(
                                    c.displayName,
                                    color = Color.White,
                                    fontSize = Tokens.Font.label,
                                    fontWeight = FontWeight.Medium,
                                    maxLines = 1,
                                    overflow = TextOverflow.Ellipsis,
                                )
                                Text(
                                    stringResource(
                                        when {
                                            c.key in flooders -> R.string.rotate_hint_flooder
                                            model.online[c.key] == true -> R.string.rotate_hint_online
                                            else -> R.string.rotate_hint_offline
                                        },
                                    ),
                                    color = Tokens.Colors.tertiary,
                                    fontSize = Tokens.Font.body,
                                    modifier = Modifier.padding(top = Tokens.Space.xxs),
                                )
                            }
                            Spacer(Modifier.width(Tokens.Space.m))
                            SelectCheck(on)
                        }
                    }
                }
                Caption(stringResource(R.string.rotate_will_lose))
            }

            SectionTitle(stringResource(R.string.rotate_how))
            CardSection {
                StepsColumn {
                    StepLine(1, stringResource(R.string.rotate_step_offline), RotStep.Wait)
                    StepLine(2, stringResource(R.string.rotate_p_up), RotStep.Wait)
                    StepLine(3, stringResource(R.string.rotate_step_tell), RotStep.Wait)
                }
            }
        }
        Text(
            stringResource(R.string.rotate_no_undo),
            color = Tokens.Colors.tertiary,
            fontSize = Tokens.Font.body,
            textAlign = TextAlign.Center,
            modifier = Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.xl),
        )
        ScreenFooter(stringResource(R.string.rotate_btn)) {
            model.rotateAddress(chosen.map { it.key })
            onDone()
        }
    }
}

/** Ход смены адреса: пауза офлайн с отсчётом, подъём, «уведомлено N из M». */
@Composable
internal fun RotationProgress(model: AppModel, onFinish: () -> Unit) {
    val st = model.rotation ?: return
    val up = st.newAddress.isNotEmpty()
    val paused = st.secondsLeft > 0
    Column(Modifier.fillMaxSize()) {
        Column(
            Modifier.weight(1f).verticalScroll(rememberScrollState())
                .padding(top = Tokens.Space.l, bottom = Tokens.Space.l),
        ) {
            CardSection {
                StepsColumn {
                    StepLine(1, stringResource(R.string.rotate_p_stopped), RotStep.Done)
                    StepLine(
                        2,
                        if (paused) stringResource(R.string.rotate_p_pause, st.secondsLeft) else stringResource(R.string.rotate_p_paused),
                        if (paused) RotStep.Now else RotStep.Done,
                    )
                    StepLine(
                        3,
                        stringResource(R.string.rotate_p_up),
                        when {
                            up -> RotStep.Done
                            !paused -> RotStep.Now
                            else -> RotStep.Wait
                        },
                    )
                    StepLine(
                        4,
                        stringResource(R.string.rotate_p_told, st.told, st.total),
                        when {
                            up && st.told >= st.total -> RotStep.Done
                            up -> RotStep.Now
                            else -> RotStep.Wait
                        },
                    )
                }
            }
            if (!up) {
                Caption(stringResource(R.string.rotate_wait_caption))
            } else {
                SectionTitle(stringResource(R.string.rotate_done))
                CardSection {
                    CardRow(divider = false) {
                        Column(Modifier.weight(1f)) {
                            Text(stringResource(R.string.rotate_new_address), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.body)
                            MonoText(st.newAddress, Color.White)
                        }
                    }
                }
                Caption(stringResource(R.string.rotate_if_continues))
            }
        }
        if (up) {
            ScreenFooter(stringResource(R.string.common_done)) {
                model.dismissRotation()
                onFinish()
            }
        }
    }
}

/** Подпись над группой с числом справа («Сообщить новый адрес · 4 из 5»). */
@Composable
private fun SectionText(text: String, modifier: Modifier = Modifier) {
    Text(text, color = Tokens.Colors.tertiary, fontSize = Tokens.Font.body, fontWeight = FontWeight.Medium, modifier = modifier)
}

private enum class RotStep { Done, Now, Wait }

/** Группа шагов: поля 16 по бокам, 4 сверху и снизу (макет `.grp` с шагами). */
@Composable
private fun StepsColumn(content: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.xs), content = content)
}

/** Шаг: кружок с номером (сделан — галочка на зелёном, сейчас — акцент) и текст. */
@Composable
private fun StepLine(n: Int, text: String, state: RotStep) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = Tokens.Size.stepRow),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        Box(
            Modifier.size(Tokens.Size.stepDot).clip(CircleShape).background(
                when (state) {
                    RotStep.Done -> Tokens.Colors.stateOk
                    RotStep.Now -> Tokens.Colors.accentMuted
                    RotStep.Wait -> Tokens.Colors.raised
                },
            ),
            contentAlignment = Alignment.Center,
        ) {
            if (state == RotStep.Done) {
                Icon(TablerIcons.Check, null, tint = Tokens.Colors.online, modifier = Modifier.size(Tokens.Size.checkMark))
            } else {
                Text(
                    "$n",
                    color = if (state == RotStep.Now) Tokens.Colors.accentSoft else Tokens.Colors.tertiary,
                    fontSize = Tokens.Font.small,
                    fontWeight = FontWeight.Bold,
                )
            }
        }
        Text(text, color = if (state == RotStep.Wait) Tokens.Colors.tertiary else Color.White, fontSize = Tokens.Font.label)
    }
}

/** Отступ разделителя у строки человека: поле строки + аватар + зазор. */
private val PersonInset = Tokens.Space.l + Tokens.Size.avatarPick + Tokens.Space.m + Tokens.Space.xxs
