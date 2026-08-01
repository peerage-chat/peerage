@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Autorenew
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.PowerSettingsNew
import androidx.compose.material.icons.filled.RadioButtonUnchecked
import androidx.compose.material.icons.filled.Send
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.R

/**
 * Экраны потока трафика и смены адреса (ADR-007).
 *
 * Тон текстов здесь намеренно прямой: остановить чужие пакеты на устройстве нельзя,
 * трафик за них уже списан. Обещать защиту, которой нет, — худшее, что можно сделать
 * с человеком, у которого прямо сейчас тают деньги на счету.
 */

/** Карточка на экране «Сеть»: появляется, только когда поток идёт. */
@Composable
internal fun FloodCard(model: AppModel, onDetails: () -> Unit) {
    val flood = model.floodPeers
    if (flood.isEmpty()) return
    Box(
        Modifier.fillMaxWidth()
            .clip(RoundedCornerShape(14.dp))
            .background(HoloWarn.copy(alpha = 0.08f))
            .clickable { onDetails() }
            .padding(12.dp),
    ) {
        Column {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Filled.Warning, null, tint = HoloWarn, modifier = Modifier.size(20.dp))
                Spacer(Modifier.width(8.dp))
                Text(
                    stringResource(R.string.flood_title),
                    color = HoloWarn,
                    fontSize = 15.sp,
                    fontWeight = FontWeight.Medium,
                )
            }
            Text(
                stringResource(R.string.flood_card_body, flood.size),
                color = HoloSecondary,
                fontSize = 13.sp,
                modifier = Modifier.padding(top = 6.dp),
            )
        }
    }
}

/** Детали потока: кто, с каким темпом, и два настоящих действия. */
@Composable
internal fun FloodScreen(model: AppModel, onRotate: () -> Unit) {
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(ScreenPad),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        SurfaceCard {
            Text(stringResource(R.string.flood_explain), color = HoloSecondary, fontSize = 13.sp)
        }

        FieldLabel(stringResource(R.string.flood_sources))
        model.floodPeers.forEach { f ->
            val name = model.contacts.firstOrNull { it.key == f.peer }?.displayName
            SurfaceCard {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Text(
                                name ?: (f.peer.take(8) + "…"),
                                color = Color.White,
                                fontSize = 15.sp,
                                fontFamily = if (name == null) FontFamily.Monospace else FontFamily.Default,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                                modifier = Modifier.weight(1f, fill = false),
                            )
                            if (f.blocked) {
                                Spacer(Modifier.width(6.dp))
                                Text(
                                    stringResource(R.string.flood_blocked_tag),
                                    color = HoloError,
                                    fontSize = 11.sp,
                                )
                            }
                        }
                        Text(
                            stringResource(
                                if (name == null) R.string.flood_src_stranger else R.string.flood_src_dropped,
                                f.dropped,
                            ),
                            color = HoloSecondary,
                            fontSize = 12.sp,
                            modifier = Modifier.padding(top = 2.dp),
                        )
                    }
                    Spacer(Modifier.width(10.dp))
                    Column(horizontalAlignment = Alignment.End) {
                        Text("%.0f".format(f.pps), color = HoloWarn, fontSize = 15.sp, fontWeight = FontWeight.Medium)
                        Text(stringResource(R.string.flood_pps), color = HoloSecondary, fontSize = 11.sp)
                    }
                }
            }
        }

        FieldLabel(stringResource(R.string.flood_what_to_do))
        ActionExplain(
            Icons.Filled.PowerSettingsNew,
            stringResource(R.string.flood_go_offline),
            stringResource(R.string.flood_go_offline_desc),
        )
        ActionExplain(
            Icons.Filled.Autorenew,
            stringResource(R.string.flood_rotate),
            stringResource(R.string.flood_rotate_desc),
        )
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            OutlineAction(
                stringResource(if (model.offline) R.string.flood_back_online else R.string.flood_offline_btn),
                Icons.Filled.PowerSettingsNew,
                Modifier.weight(1f),
            ) { if (model.offline) model.goOnline() else model.goOffline() }
            Box(Modifier.weight(1f)) { AccentButton(stringResource(R.string.flood_rotate_btn), onClick = onRotate) }
        }
        Text(
            stringResource(R.string.flood_footer),
            color = HoloSecondary,
            fontSize = 12.sp,
        )
    }
}

/** Строка «действие + честное объяснение цены». */
@Composable
private fun ActionExplain(icon: androidx.compose.ui.graphics.vector.ImageVector, title: String, desc: String) {
    SurfaceCard {
        Row {
            Icon(icon, null, tint = HoloSecondary, modifier = Modifier.size(22.dp))
            Spacer(Modifier.width(12.dp))
            Column {
                Text(title, color = Color.White, fontSize = 15.sp)
                Text(desc, color = HoloSecondary, fontSize = 12.sp, modifier = Modifier.padding(top = 2.dp))
            }
        }
    }
}

/**
 * Смена адреса: выбор, КОМУ сообщить новый.
 *
 * Здесь главный смысл операции. Адрес меняется для всех, но узнают о нём только
 * отмеченные — не отмеченный контакт теряет с нами связь. Источник потока снят
 * заранее, однако галочка остаётся кликабельной: решение за пользователем.
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

    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(ScreenPad),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        SurfaceCard {
            Text(stringResource(R.string.rotate_explain), color = HoloSecondary, fontSize = 13.sp)
        }

        FieldLabel(stringResource(R.string.rotate_tell_whom))
        model.contacts.forEach { c ->
            val on = selected[c.key] == true
            SurfaceCard(onClick = { selected[c.key] = !on }) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Checkbox(
                        checked = on,
                        onCheckedChange = { selected[c.key] = it },
                        colors = CheckboxDefaults.colors(checkedColor = HoloAccent, uncheckedColor = HoloSecondary),
                    )
                    Spacer(Modifier.width(4.dp))
                    SquareAvatar(c.displayName, size = 34.dp, online = model.online[c.key] == true)
                    Spacer(Modifier.width(10.dp))
                    Column(Modifier.weight(1f)) {
                        Text(c.displayName, color = Color.White, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        val hint = when {
                            c.key in flooders -> stringResource(R.string.rotate_hint_flooder)
                            model.online[c.key] != true -> stringResource(R.string.rotate_hint_offline)
                            else -> null
                        }
                        if (hint != null) {
                            Text(hint, color = HoloWarn, fontSize = 12.sp, modifier = Modifier.padding(top = 2.dp))
                        }
                    }
                }
            }
        }

        SurfaceCard {
            Column {
                Text(
                    stringResource(R.string.rotate_selected, chosen.size, model.contacts.size),
                    color = Color.White,
                    fontSize = 15.sp,
                )
                val lost = model.contacts.filter { selected[it.key] != true }
                if (lost.isNotEmpty()) {
                    Text(
                        stringResource(R.string.rotate_will_lose, lost.joinToString { it.displayName }),
                        color = HoloSecondary,
                        fontSize = 12.sp,
                        modifier = Modifier.padding(top = 2.dp),
                    )
                }
            }
        }

        FieldLabel(stringResource(R.string.rotate_how))
        StepRow(Icons.Filled.PowerSettingsNew, stringResource(R.string.rotate_step_offline, AppModel.ROTATE_GAP_SECS))
        StepRow(Icons.Filled.Autorenew, stringResource(R.string.rotate_step_up))
        StepRow(Icons.Filled.Send, stringResource(R.string.rotate_step_tell))

        AccentButton(stringResource(R.string.rotate_btn)) {
            model.rotateAddress(chosen.map { it.key })
            onDone()
        }
        Text(stringResource(R.string.rotate_no_undo), color = HoloSecondary, fontSize = 12.sp)
    }
}

@Composable
private fun StepRow(icon: androidx.compose.ui.graphics.vector.ImageVector, text: String) {
    Row(Modifier.padding(vertical = 2.dp)) {
        Icon(icon, null, tint = HoloIconMuted, modifier = Modifier.size(18.dp))
        Spacer(Modifier.width(12.dp))
        Text(text, color = HoloSecondary, fontSize = 13.sp)
    }
}

/**
 * Ход смены адреса: обратный отсчёт паузы, затем итог.
 *
 * Паузу не прячем — пользователь минуту офлайн, и он должен понимать, почему.
 */
@Composable
internal fun RotationProgress(model: AppModel, onFinish: () -> Unit) {
    val st = model.rotation ?: return
    val done = st.newAddress.isNotEmpty()
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(ScreenPad),
        verticalArrangement = Arrangement.spacedBy(14.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        if (!done) {
            Text(
                "0:%02d".format(st.secondsLeft),
                color = Color.White,
                fontSize = 44.sp,
                fontFamily = FontFamily.Monospace,
                modifier = Modifier.padding(top = 28.dp),
            )
            Text(
                stringResource(R.string.rotate_wait_body),
                color = HoloSecondary,
                fontSize = 13.sp,
            )
        }
        Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            ProgressStep(true, stringResource(R.string.rotate_p_stopped))
            ProgressStep(st.secondsLeft == 0, stringResource(R.string.rotate_p_pause, st.secondsLeft))
            ProgressStep(done, stringResource(R.string.rotate_p_up))
            ProgressStep(done, stringResource(R.string.rotate_p_told, st.told))
        }
        if (done) {
            SurfaceCard {
                Column {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(Icons.Filled.CheckCircle, null, tint = HoloOnline, modifier = Modifier.size(20.dp))
                        Spacer(Modifier.width(8.dp))
                        Text(stringResource(R.string.rotate_done), color = HoloOnline, fontSize = 15.sp, fontWeight = FontWeight.Medium)
                    }
                    Text(
                        st.newAddress,
                        color = Color.White,
                        fontSize = 13.sp,
                        fontFamily = FontFamily.Monospace,
                        modifier = Modifier.padding(top = 6.dp),
                    )
                }
            }
            AccentButton(stringResource(R.string.common_done)) {
                model.dismissRotation()
                onFinish()
            }
            Text(stringResource(R.string.rotate_if_continues), color = HoloSecondary, fontSize = 12.sp)
        } else {
            Text(stringResource(R.string.rotate_wait_footer), color = HoloSecondary, fontSize = 12.sp)
        }
    }
}

@Composable
private fun ProgressStep(done: Boolean, text: String) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Icon(
            if (done) Icons.Filled.CheckCircle else Icons.Filled.RadioButtonUnchecked,
            null,
            tint = if (done) HoloOnline else HoloIconMuted,
            modifier = Modifier.size(18.dp),
        )
        Spacer(Modifier.width(12.dp))
        Text(text, color = if (done) Color.White else HoloSecondary, fontSize = 13.sp)
    }
}
