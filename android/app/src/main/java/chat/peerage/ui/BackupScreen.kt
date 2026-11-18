package chat.peerage.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Archive
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Groups
import androidx.compose.material.icons.filled.Message
import androidx.compose.material.icons.filled.Notes
import androidx.compose.material.icons.filled.People
import androidx.compose.material.icons.filled.Phone
import androidx.compose.material.icons.filled.Photo
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * Резервная копия ДАННЫХ (макет `docs/design/mockups/data-backup.html`).
 *
 * Первым идёт объяснение, чем это отличается от копии КЛЮЧА: иначе человек решит,
 * что сохранил всё, и потеряет личность. Дальше — состав с цифрами: выбор «класть ли
 * вложения» должен делаться по размеру, а не вслепую.
 *
 * Файл пишется во внутренний кэш, а потом копируется в выбранное пользователем место
 * (SAF): движок умеет писать только по пути, а `content://`-адрес путём не является.
 */
private enum class Step { Pick, Password, Working, Done, Restore }

@Composable
internal fun BackupScreen(model: AppModel, onDone: () -> Unit) {
    val ctx = androidx.compose.ui.platform.LocalContext.current
    val scope = rememberCoroutineScope()

    var step by remember { mutableStateOf(Step.Pick) }
    var parts by remember { mutableStateOf(BackupPartsDto()) }
    var password by remember { mutableStateOf("") }
    var repeat by remember { mutableStateOf("") }
    var withPassword by remember { mutableStateOf(false) }
    var result by remember { mutableStateOf<BackupStatsDto?>(null) }
    var restoring by remember { mutableStateOf(false) }
    var picked by remember { mutableStateOf<PickedBackup?>(null) }
    var pickFailed by remember { mutableStateOf(false) }
    val restoreTmp = remember { java.io.File(ctx.cacheDir, "restore.tmp") }
    DisposableEffect(Unit) { onDispose { restoreTmp.delete() } }

    LaunchedEffect(Unit) { model.refreshBackupEstimate() }
    val est = model.backupEstimate

    // Куда сохранить копию — спрашивает система (SAF), приложению не нужен доступ к
    // всей памяти ради одного файла.
    val saveTo = rememberLauncherForActivityResult(
        ActivityResultContracts.CreateDocument("application/octet-stream"),
    ) { uri ->
        if (uri == null) {
            step = Step.Password
            return@rememberLauncherForActivityResult
        }
        step = Step.Working
        scope.launch(Dispatchers.IO) {
            val tmp = java.io.File(ctx.cacheDir, "backup.tmp")
            var stats = model.exportBackupBlocking(tmp.absolutePath, parts, if (withPassword) password else "")
            if (stats.error.isEmpty()) {
                // Копия собрана — но ещё не записана туда, куда выбрал человек. Сбой
                // записи (флешка вынута, провайдер отказал) раньше проглатывался, и
                // экран говорил «Копия готова» про файл, которого нет.
                val written = runCatching {
                    ctx.contentResolver.openOutputStream(uri)?.use { out ->
                        tmp.inputStream().use { it.copyTo(out) }
                    } != null
                }.getOrDefault(false)
                if (!written) stats = stats.copy(error = ctx.getString(R.string.backup_write_failed))
            }
            // Кэш чистим всегда: незашифрованного там нет, но копия переписки во
            // временном файле пережила бы экран, и это лишнее.
            tmp.delete()
            result = stats
            step = Step.Done
        }
    }

    val openFrom = rememberLauncherForActivityResult(
        ActivityResultContracts.OpenDocument(),
    ) { uri ->
        if (uri != null) {
            scope.launch(Dispatchers.IO) {
                val p = pickBackup(ctx, uri, restoreTmp)
                withContext(Dispatchers.Main) {
                    picked = p
                    pickFailed = p == null
                    password = ""
                }
            }
        }
    }

    when (step) {
        Step.Pick -> PickParts(
            est = est,
            parts = parts,
            onParts = { parts = it },
            onNext = {
                restoring = false
                step = Step.Password
            },
            onRestore = {
                restoring = true
                step = Step.Restore
            },
        )

        Step.Password -> PasswordStep(
            withPassword = withPassword,
            onWithPassword = { withPassword = it },
            password = password,
            repeat = repeat,
            onPassword = { password = it },
            onRepeat = { repeat = it },
            onGo = { saveTo.launch(defaultFileName()) },
        )

        Step.Working -> WorkingStep(restoring)

        Step.Restore -> RestoreStep(
            // Имя — от провайдера, не lastPathSegment: у content:// это id документа («43»).
            picked = picked,
            pickFailed = pickFailed,
            password = password,
            onPassword = { password = it },
            onPickFile = { openFrom.launch(arrayOf("*/*")) },
            onGo = {
                if (picked == null) return@RestoreStep
                step = Step.Working
                restoring = true
                scope.launch(Dispatchers.IO) {
                    result = model.importBackupBlocking(restoreTmp.absolutePath, password)
                    restoreTmp.delete()
                    step = Step.Done
                }
            },
        )

        Step.Done -> DoneStep(result, restoring, onDone)
    }
}

/** Имя файла с датой: копий бывает несколько, и «backup.peerage» их обезличивает. */
private fun defaultFileName(): String {
    val day = java.text.SimpleDateFormat("yyyy-MM-dd", java.util.Locale.US)
        .format(java.util.Date())
    return "backup-$day.peerage"
}

@Composable
private fun PickParts(
    est: BackupStatsDto,
    parts: BackupPartsDto,
    onParts: (BackupPartsDto) -> Unit,
    onNext: () -> Unit,
    onRestore: () -> Unit,
) {
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(ScreenPad),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        InfoCard(
            Icons.Filled.Archive,
            stringResource(R.string.backup_what_title),
            stringResource(R.string.backup_what_text),
        )

        SectionTitle(stringResource(R.string.backup_contents))

        PartRow(
            Icons.Filled.People,
            stringResource(R.string.backup_part_contacts),
            stringResource(R.string.backup_n_contacts, est.contacts),
            null,
            parts.contacts,
        ) { onParts(parts.copy(contacts = it)) }

        PartRow(
            Icons.Filled.Message,
            stringResource(R.string.backup_part_messages),
            pluralOf(R.plurals.backup_n_messages, est.messages),
            null,
            parts.messages,
        ) { onParts(parts.copy(messages = it)) }

        PartRow(
            Icons.Filled.Groups,
            stringResource(R.string.backup_part_groups),
            pluralOf(R.plurals.backup_n_group_messages, est.groupMessages),
            null,
            parts.groups,
        ) { onParts(parts.copy(groups = it)) }

        PartRow(
            Icons.Filled.Notes,
            stringResource(R.string.backup_part_notes),
            stringResource(R.string.backup_n_notes, est.notes),
            null,
            parts.notes,
        ) { onParts(parts.copy(notes = it)) }

        PartRow(
            Icons.Filled.Phone,
            stringResource(R.string.backup_part_calls),
            pluralOf(R.plurals.backup_n_calls, est.calls),
            null,
            parts.calls,
        ) { onParts(parts.copy(calls = it)) }

        PartRow(
            Icons.Filled.Photo,
            stringResource(R.string.backup_part_attachments),
            if (est.skippedBig > 0) {
                pluralOf(R.plurals.backup_n_attachments_skipped, est.attachments, est.skippedBig)
            } else {
                pluralOf(R.plurals.backup_n_attachments, est.attachments)
            },
            fmtSizeLong(est.attachmentBytes),
            parts.attachments,
        ) { onParts(parts.copy(attachments = it)) }

        if (est.skippedBig > 0) {
            Text(
                stringResource(R.string.backup_big_hint),
                color = HoloSecondary,
                fontSize = Tokens.Font.small,
            )
        }

        // Про членство молчать нельзя: иначе человек узнает об этом после потери
        // телефона — в худший возможный момент.
        InfoCard(
            Icons.Filled.Groups,
            stringResource(R.string.backup_groups_title),
            stringResource(R.string.backup_groups_text),
            warn = true,
        )

        val any = parts.contacts || parts.messages || parts.groups ||
            parts.notes || parts.calls || parts.attachments
        // Ничего не выбрано — кнопка серая, а не глухая: нажатие без ответа
        // читается как зависание.
        AccentButton(stringResource(R.string.backup_next), Modifier.fillMaxWidth(), enabled = any, onClick = onNext)
        OutlineButton(
            stringResource(R.string.backup_restore),
            Modifier.fillMaxWidth(),
            tint = Color.White,
            onClick = onRestore,
        )
        Spacer(Modifier.height(Tokens.Space.l))
    }
}

/**
 * Чем закрыта копия (макет `misc/BackupPass`, шаг 7k): ключом из 24 слов всегда, паролем —
 * по желанию. Без пароля «Создать копию» доступна сразу.
 */
@Composable
private fun PasswordStep(
    withPassword: Boolean,
    onWithPassword: (Boolean) -> Unit,
    password: String,
    repeat: String,
    onPassword: (String) -> Unit,
    onRepeat: (String) -> Unit,
    onGo: () -> Unit,
) {
    val mismatch = repeat.isNotEmpty() && repeat != password
    val ready = !withPassword || (password.isNotEmpty() && password == repeat)
    Column(Modifier.fillMaxSize()) {
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(bottom = Tokens.Space.l)) {
            SectionTitle(stringResource(R.string.backup_lock_title))
            CardSection {
                CardRow(divider = false, inset = Tokens.Space.l) {
                    Icon(TablerIcons.Key, null, tint = Tokens.Colors.fieldIcon, modifier = Modifier.size(Tokens.Size.iconM))
                    Spacer(Modifier.width(Tokens.Space.m))
                    Column(Modifier.weight(1f).padding(vertical = Tokens.Space.xs)) {
                        Text(stringResource(R.string.backup_lock_words), color = Color.White, fontSize = Tokens.Font.name)
                        Text(
                            stringResource(R.string.backup_lock_words_sub),
                            color = Tokens.Colors.tertiary,
                            fontSize = Tokens.Font.body,
                            modifier = Modifier.padding(top = Tokens.Space.xxs),
                        )
                    }
                    Icon(TablerIcons.Check, null, tint = Tokens.Colors.online, modifier = Modifier.size(Tokens.Size.iconS))
                }
                CardRow(inset = Tokens.Space.l, modifier = Modifier.switchRow(withPassword, onChange = onWithPassword)) {
                    Column(Modifier.weight(1f).padding(vertical = Tokens.Space.xs)) {
                        Text(stringResource(R.string.backup_lock_password), color = Color.White, fontSize = Tokens.Font.name)
                        Text(
                            stringResource(R.string.backup_lock_password_sub),
                            color = Tokens.Colors.tertiary,
                            fontSize = Tokens.Font.body,
                            modifier = Modifier.padding(top = Tokens.Space.xxs),
                        )
                    }
                    Spacer(Modifier.width(Tokens.Space.m))
                    Switch(checked = withPassword, onCheckedChange = null)
                }
            }
            if (withPassword) {
                Spacer(Modifier.height(Tokens.Space.m))
                Column(Modifier.padding(horizontal = Tokens.Space.panel), verticalArrangement = Arrangement.spacedBy(Tokens.Space.m)) {
                    PasswordField(password, error = false, hint = stringResource(R.string.vault_ph_password), onValue = onPassword)
                    PasswordField(repeat, error = mismatch, hint = stringResource(R.string.vault_ph_repeat), onValue = onRepeat)
                    if (mismatch) OnbError(stringResource(R.string.vault_mismatch))
                    OnbWarn(stringResource(R.string.backup_lock_no_recovery))
                }
            }
        }
        ScreenFooter(stringResource(R.string.backup_create), enabled = ready, onClick = onGo)
    }
}

@Composable
private fun WorkingStep(restoring: Boolean) {
    Column(
        Modifier.fillMaxSize().padding(ScreenPad),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Spacer(Modifier.height(Tokens.Space.hero + Tokens.Space.xxxl))
        Text(
            stringResource(if (restoring) R.string.backup_working_restore else R.string.backup_working),
            color = Color.White,
            fontSize = Tokens.Font.title,
            fontWeight = FontWeight.Medium,
        )
        Spacer(Modifier.height(Tokens.Space.xl))
        LinearProgressIndicator(color = HoloAccent, modifier = Modifier.fillMaxWidth())
        Spacer(Modifier.height(Tokens.Space.xl))
        // Про секунду сказано намеренно: без этого пауза читается как «подвисло».
        Text(stringResource(R.string.backup_working_hint), color = HoloSecondary, fontSize = Tokens.Font.small)
    }
}

@Composable
private fun RestoreStep(
    picked: PickedBackup?,
    pickFailed: Boolean,
    password: String,
    onPassword: (String) -> Unit,
    onPickFile: () -> Unit,
    onGo: () -> Unit,
) {
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(ScreenPad),
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        SurfaceCard(onClick = onPickFile) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Filled.Archive, null, tint = HoloAccent, modifier = Modifier.size(Tokens.Size.iconM))
                Spacer(Modifier.width(Tokens.Space.m))
                Column {
                    Text(
                        picked?.name ?: stringResource(R.string.backup_pick_file),
                        color = Color.White,
                        fontSize = Tokens.Font.label,
                    )
                    Text(
                        stringResource(R.string.backup_pick_hint),
                        color = HoloSecondary,
                        fontSize = Tokens.Font.small,
                        modifier = Modifier.padding(top = Tokens.Space.xxs),
                    )
                }
            }
        }

        if (pickFailed) OnbError(stringResource(R.string.transfer_not_backup))
        // Пароль — только если копия им закрыта (шаг 7k): без него она открывается ключом из 24 слов.
        if (picked?.needsPassword == true) {
            FieldLabel(stringResource(R.string.backup_pw_label))
            PasswordField(password, error = false, hint = stringResource(R.string.vault_ph_password), onValue = onPassword)
        }

        // Страх «а не затрёт ли новое» — главная причина не нажимать эту кнопку.
        InfoCard(
            Icons.Filled.Check,
            stringResource(R.string.backup_merge_title),
            stringResource(R.string.backup_merge_text),
        )

        // Сказать ДО нажатия, а не после: иначе человек решит, что верификация
        // потерялась из-за сбоя, и перестанет доверять восстановлению.
        Text(
            stringResource(R.string.backup_verify_again),
            color = HoloWarn,
            fontSize = Tokens.Font.small,
        )

        AccentButton(
            stringResource(R.string.backup_restore_go),
            Modifier.fillMaxWidth(),
            enabled = picked != null && (!picked.needsPassword || password.isNotEmpty()),
            onClick = onGo,
        )
    }
}

@Composable
private fun DoneStep(stats: BackupStatsDto?, restoring: Boolean, onDone: () -> Unit) {
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(ScreenPad),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        Spacer(Modifier.height(Tokens.Space.hero))
        val err = stats?.error.orEmpty()
        if (err.isNotEmpty()) {
            Text(stringResource(R.string.backup_failed), color = HoloError, fontSize = Tokens.Font.title, fontWeight = FontWeight.Medium)
            Text(err, color = HoloSecondary, fontSize = Tokens.Font.body)
        } else {
            Icon(Icons.Filled.CheckCircle, null, tint = HoloOnline, modifier = Modifier.size(Tokens.Size.iconHero))
            Text(
                stringResource(if (restoring) R.string.backup_done_restore else R.string.backup_done_export),
                color = Color.White,
                fontSize = Tokens.Font.title,
                fontWeight = FontWeight.Medium,
            )
            val s = stats ?: BackupStatsDto()
            Text(
                stringResource(
                    R.string.backup_done_counts,
                    s.contacts,
                    s.messages,
                    s.notes,
                    s.attachments,
                ),
                color = HoloSecondary,
                fontSize = Tokens.Font.body,
            )
            if (restoring) {
                InfoCard(
                    Icons.Filled.Groups,
                    stringResource(R.string.backup_after_groups_title),
                    stringResource(R.string.backup_after_groups_text),
                    warn = true,
                )
                Text(
                    stringResource(R.string.backup_verify_again),
                    color = HoloSecondary,
                    fontSize = Tokens.Font.small,
                )
            }
        }
        AccentButton(stringResource(R.string.common_done), Modifier.fillMaxWidth()) { onDone() }
    }
}

/** Строка состава: галка + иконка + название/цифры + размер. */
@Composable
private fun PartRow(
    icon: ImageVector,
    title: String,
    sub: String,
    size: String?,
    checked: Boolean,
    onCheck: (Boolean) -> Unit,
) {
    SurfaceCard(onClick = { onCheck(!checked) }, outer = 0.dp) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            // Невыбранное — ПУСТАЯ рамка, а не бледная галка: бледная читается как
            // «выбрано, но недоступно», и человек не поймёт, что именно уедет в копию.
            Box(
                Modifier.size(22.dp).clip(RoundedCornerShape(Tokens.Radius.badge))
                    .background(if (checked) HoloAccent else Color.Transparent)
                    .then(
                        if (checked) {
                            Modifier
                        } else {
                            Modifier.border(2.dp, HoloSecondary, RoundedCornerShape(Tokens.Radius.badge))
                        },
                    )
                    .clickable { onCheck(!checked) },
                contentAlignment = Alignment.Center,
            ) {
                if (checked) {
                    Icon(Icons.Filled.Check, null, tint = HoloAccentDark, modifier = Modifier.size(Tokens.Size.iconXs))
                }
            }
            Spacer(Modifier.width(Tokens.Space.m))
            Icon(icon, null, tint = HoloSecondary, modifier = Modifier.size(Tokens.Size.iconM))
            Spacer(Modifier.width(Tokens.Space.m))
            Column(Modifier.weight(1f)) {
                Text(title, color = Color.White, fontSize = Tokens.Font.label)
                Text(sub, color = HoloSecondary, fontSize = Tokens.Font.small, modifier = Modifier.padding(top = Tokens.Space.xxs))
            }
            if (size != null) {
                Text(size, color = HoloSecondary, fontSize = Tokens.Font.body)
            }
        }
    }
}

/** Карточка-пояснение; `warn` — жёлтая рамка для того, чего в копии НЕТ. */
@Composable
private fun InfoCard(icon: ImageVector, title: String, text: String, warn: Boolean = false) {
    val tint = if (warn) HoloWarn else HoloAccent
    Box(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.tile))
            .background(if (warn) HoloWarn.copy(alpha = 0.10f) else HoloSurface)
            .padding(Tokens.Space.l),
    ) {
        Column {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(icon, null, tint = tint, modifier = Modifier.size(Tokens.Size.iconM))
                Spacer(Modifier.width(Tokens.Space.s))
                Text(title, color = tint, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium)
            }
            Text(
                text,
                color = HoloSecondary,
                fontSize = Tokens.Font.body,
                modifier = Modifier.padding(top = Tokens.Space.s),
            )
        }
    }
}
