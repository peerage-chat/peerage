@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*

// Онбординг (M4) — реализация по макету docs/design/mockups/onboarding.html.
// Единый скелет с фиксированными зонами: заголовок · описание · контент · короткая
// надпись · Готово · Пропустить — зоны совпадают по высоте, поэтому шаги не «прыгают».
// TODO(i18n): строки захардкожены (RU) — вынести в strings.xml (EN+RU) при интеграции.
// TODO(M4): подключить гейт первого запуска, реальный TextField имени/аватар, экспорт
//   сид-фразы в файл/фразу (движковый хвост), запрос разрешений по кнопкам.

private val WarnBg = WarnColor.copy(alpha = 0.12f)
private val WarnBorder = WarnColor.copy(alpha = 0.40f)
private val WarnText = Color(0xFFE8C98A)

/** Пошаговый флоу первого запуска. `onFinish` — онбординг завершён (пометить флаг). */
@Composable
internal fun OnboardingFlow(model: AppModel, onFinish: () -> Unit) {
    var step by rememberSaveable { mutableStateOf(0) }
    val next: () -> Unit = { if (step < 3) step++ else onFinish() }
    when (step) {
        0 -> WelcomeStep(onStart = next, onImport = next)
        1 -> ProfileStep(onDone = next, onSkip = next)
        2 -> KeyBackupStep(model = model, onDone = next, onSkip = next)
        else -> PermissionsStep(onDone = onFinish)
    }
}

/** Единый скелет экрана онбординга (фиксированные зоны из макета). */
@Composable
private fun OnbScaffold(
    title: String,
    desc: String,
    primary: String,
    onPrimary: () -> Unit,
    secondary: String? = null,
    onSecondary: (() -> Unit)? = null,
    contentArrangement: Arrangement.Vertical = Arrangement.Center,
    caption: (@Composable RowScope.() -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(
        Modifier.fillMaxSize().background(HoloBg).padding(horizontal = 24.dp).padding(top = 24.dp, bottom = 22.dp),
    ) {
        Box(Modifier.fillMaxWidth().height(56.dp), Alignment.Center) {
            Text(title, color = Color.White, fontSize = 22.sp, fontWeight = FontWeight.Medium, textAlign = TextAlign.Center)
        }
        Box(Modifier.fillMaxWidth().height(40.dp), Alignment.Center) {
            Text(desc, color = HoloSecondary, fontSize = 13.sp, textAlign = TextAlign.Center)
        }
        Column(
            Modifier.fillMaxWidth().weight(1f).padding(vertical = 8.dp),
            verticalArrangement = contentArrangement,
            horizontalAlignment = Alignment.CenterHorizontally,
            content = content,
        )
        Row(Modifier.fillMaxWidth().height(20.dp), horizontalArrangement = Arrangement.Center, verticalAlignment = Alignment.CenterVertically) {
            caption?.invoke(this)
        }
        Spacer(Modifier.height(12.dp))
        OnbButton(primary, filled = true, onClick = onPrimary)
        Spacer(Modifier.height(10.dp))
        // Слот secondary держим всегда (пустой резерв на последнем шаге) — чтобы primary
        // не прыгал по вертикали между шагами.
        if (secondary != null) {
            OnbButton(secondary, filled = false, onClick = onSecondary ?: {})
        } else {
            Spacer(Modifier.height(48.dp))
        }
    }
}

@Composable
private fun OnbButton(text: String, filled: Boolean, onClick: () -> Unit) {
    val shape = RoundedCornerShape(Tokens.Radius.button)
    Box(
        Modifier.fillMaxWidth().height(48.dp).clip(shape)
            .then(if (filled) Modifier.background(HoloAccent) else Modifier.border(1.dp, HoloBorder, shape))
            .clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text,
            color = if (filled) HoloAccentDark else Tokens.Colors.iconChrome,
            fontSize = 15.sp,
            fontWeight = FontWeight.Medium,
        )
    }
}

@Composable
private fun Bullet(icon: ImageVector, text: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 7.dp), verticalAlignment = Alignment.CenterVertically) {
        Icon(icon, null, tint = HoloAccent, modifier = Modifier.size(24.dp))
        Spacer(Modifier.width(12.dp))
        Text(text, color = Color.White, fontSize = 14.sp)
    }
}

@Composable
private fun WelcomeStep(onStart: () -> Unit, onImport: () -> Unit) {
    OnbScaffold(
        title = "Приватность по умолчанию",
        desc = "Серверов нет. Данные — только у вас.",
        primary = "Начать",
        onPrimary = onStart,
        secondary = "У меня уже есть ключ",
        onSecondary = onImport,
        caption = { Text("Открытый код · GPL-3.0", color = HoloSecondary, fontSize = 11.sp) },
    ) {
        Box(
            Modifier.size(62.dp).clip(RoundedCornerShape(18.dp)).background(HoloAccentDark),
            contentAlignment = Alignment.Center,
        ) { Icon(TablerIcons.ShieldLock, null, tint = HoloAccent, modifier = Modifier.size(32.dp)) }
        Spacer(Modifier.height(8.dp))
        Bullet(TablerIcons.PhoneOff, "Без номера телефона")
        Bullet(TablerIcons.AddressBookOff, "Не читаем ваши контакты")
        Bullet(TablerIcons.MapPinOff, "Не знаем, где вы")
        Bullet(TablerIcons.ServerOff, "Нет центрального сервера")
    }
}

@Composable
private fun ProfileStep(onDone: () -> Unit, onSkip: () -> Unit) {
    OnbScaffold(
        title = "Ваш профиль",
        desc = "Необязательно — можно изменить позже",
        primary = "Готово",
        onPrimary = onDone,
        secondary = "Пропустить",
        onSecondary = onSkip,
        caption = { Text("Видно только по вашему QR-коду", color = HoloSecondary, fontSize = 11.sp) },
    ) {
        Box(contentAlignment = Alignment.BottomEnd) {
            Box(
                Modifier.size(96.dp).clip(RoundedCornerShape(26.dp)).background(Tokens.Colors.avatar),
                contentAlignment = Alignment.Center,
            ) { Icon(TablerIcons.User, null, tint = Color(0xFF9AA0A6), modifier = Modifier.size(40.dp)) }
            Box(
                Modifier.size(34.dp).clip(RoundedCornerShape(12.dp)).background(HoloAccent).border(3.dp, HoloBg, RoundedCornerShape(12.dp)),
                contentAlignment = Alignment.Center,
            ) { Icon(TablerIcons.Camera, null, tint = HoloAccentDark, modifier = Modifier.size(17.dp)) }
        }
        Spacer(Modifier.height(14.dp))
        // TODO(M4): реальный TextField имени.
        Row(
            Modifier.fillMaxWidth().height(50.dp).clip(RoundedCornerShape(14.dp)).background(Tokens.Colors.tile).padding(horizontal = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(TablerIcons.User, null, tint = HoloSecondary, modifier = Modifier.size(20.dp))
            Spacer(Modifier.width(10.dp))
            Text("Имя", color = HoloSecondary, fontSize = 15.sp)
        }
    }
}

@Composable
private fun KeyBackupStep(model: AppModel, onDone: () -> Unit, onSkip: () -> Unit) {
    var revealed by remember { mutableStateOf(false) }
    if (revealed) {
        OnbScaffold(
            title = "Ваша сид-фраза",
            desc = "Запишите 24 слова по порядку и храните офлайн",
            primary = "Я записал",
            onPrimary = {
                model.clearRevealedPhrase()
                onDone()
            },
            secondary = "Назад",
            onSecondary = {
                model.clearRevealedPhrase()
                revealed = false
            },
            caption = {
                Icon(TablerIcons.ShieldCheck, null, tint = HoloOnline, modifier = Modifier.size(13.dp))
                Spacer(Modifier.width(6.dp))
                Text("Экран защищён · не фотографируйте", color = HoloOnline, fontSize = 11.sp)
            },
        ) { PhraseGrid(model.revealedPhrase) }
        return
    }
    OnbScaffold(
        title = "Резервная копия ключа",
        desc = "Ключ заменяет логин и пароль — только у вас",
        primary = "Готово",
        onPrimary = onDone,
        secondary = "Пропустить (напомним позже)",
        onSecondary = onSkip,
        contentArrangement = Arrangement.spacedBy(10.dp),
        caption = {
            Icon(TablerIcons.ShieldCheck, null, tint = HoloOnline, modifier = Modifier.size(13.dp))
            Spacer(Modifier.width(6.dp))
            Text("Экран защищён · не фотографируйте", color = HoloOnline, fontSize = 11.sp)
        },
    ) {
        // Варнинг (амбер).
        Row(
            Modifier.fillMaxWidth().weight(1f).clip(RoundedCornerShape(14.dp)).background(WarnBg)
                .border(1.dp, WarnBorder, RoundedCornerShape(14.dp)).padding(horizontal = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(TablerIcons.AlertTriangle, null, tint = WarnColor, modifier = Modifier.size(20.dp))
            Spacer(Modifier.width(12.dp))
            Text(
                "Восстановить доступ можно только этим ключом. Сервера, который «вспомнит» вас, — нет.",
                color = WarnText,
                fontSize = 12.5.sp,
                lineHeight = 17.sp,
            )
        }
        KeyOption(TablerIcons.FileDownload, "Сохранить в файл", "Зашифрованный ключ на устройство") { /* TODO(M4): экспорт в файл */ }
        KeyOption(TablerIcons.ListNumbers, "Показать фразу", "24 слова — запишите на бумаге") {
            model.revealSeedPhrase()
            revealed = true
        }
    }
}

/** Сетка 24 слов сид-фразы: два столбца по 12, каждое пронумеровано. `phrase` = null —
 *  ещё грузится (читается через ops). */
@Composable
private fun ColumnScope.PhraseGrid(phrase: String?) {
    val words = phrase?.split(" ")?.filter { it.isNotBlank() } ?: emptyList()
    if (words.size < 24) {
        Box(Modifier.fillMaxWidth().weight(1f), contentAlignment = Alignment.Center) {
            Text("…", color = HoloSecondary, fontSize = 20.sp)
        }
        return
    }
    Row(
        Modifier.fillMaxWidth().weight(1f).clip(RoundedCornerShape(14.dp)).background(Tokens.Colors.tile)
            .border(1.dp, HoloBorder, RoundedCornerShape(14.dp)).padding(14.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        for (col in 0 until 2) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.SpaceEvenly) {
                for (row in 0 until 12) {
                    val i = col * 12 + row
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text("${i + 1}", color = HoloSecondary, fontSize = 12.sp, modifier = Modifier.width(22.dp))
                        Text(words[i], color = Color.White, fontSize = 14.sp, fontWeight = FontWeight.Medium)
                    }
                }
            }
        }
    }
}

@Composable
private fun ColumnScope.KeyOption(icon: ImageVector, title: String, sub: String, onClick: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().weight(1f).clip(RoundedCornerShape(14.dp)).background(Tokens.Colors.tile)
            .border(1.dp, HoloBorder, RoundedCornerShape(14.dp)).clickable { onClick() }.padding(horizontal = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = HoloAccent, modifier = Modifier.size(24.dp))
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f)) {
            Text(title, color = Color.White, fontSize = 15.sp, fontWeight = FontWeight.Medium)
            Text(sub, color = HoloSecondary, fontSize = 12.sp)
        }
        Icon(TablerIcons.ChevronRight, null, tint = HoloSecondary, modifier = Modifier.size(18.dp))
    }
}

@Composable
private fun PermissionsStep(onDone: () -> Unit) {
    OnbScaffold(
        title = "Пара разрешений",
        desc = "Только необходимое",
        primary = "Готово",
        onPrimary = onDone,
        contentArrangement = Arrangement.spacedBy(12.dp),
    ) {
        PermCard(TablerIcons.Bell, "Уведомления", "Чтобы видеть новые сообщения и звонки.")
        PermCard(TablerIcons.BatteryCharging, "Работа в фоне", "Держать связь при погасшем экране (mesh-узел).")
    }
}

@Composable
private fun ColumnScope.PermCard(icon: ImageVector, title: String, desc: String) {
    Column(
        Modifier.fillMaxWidth().weight(1f).clip(RoundedCornerShape(14.dp)).background(Tokens.Colors.tile)
            .border(1.dp, HoloBorder, RoundedCornerShape(14.dp)).padding(14.dp),
        verticalArrangement = Arrangement.Center,
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Icon(icon, null, tint = HoloAccent, modifier = Modifier.size(22.dp))
            Spacer(Modifier.width(10.dp))
            Text(title, color = Color.White, fontSize = 15.sp, fontWeight = FontWeight.Medium)
        }
        Spacer(Modifier.height(9.dp))
        Text(desc, color = HoloSecondary, fontSize = 12.5.sp, lineHeight = 17.sp)
        Spacer(Modifier.height(11.dp))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Box(
                Modifier.weight(1f).height(38.dp).clip(RoundedCornerShape(11.dp)).background(HoloAccent).clickable { /* TODO grant */ },
                contentAlignment = Alignment.Center,
            ) { Text("Разрешить", color = HoloAccentDark, fontSize = 13.sp, fontWeight = FontWeight.Medium) }
            Box(
                Modifier.weight(1f).height(38.dp).clip(RoundedCornerShape(11.dp)).border(1.dp, HoloBorder, RoundedCornerShape(11.dp)).clickable { /* later */ },
                contentAlignment = Alignment.Center,
            ) { Text("Позже", color = HoloSecondary, fontSize = 13.sp) }
        }
    }
}
