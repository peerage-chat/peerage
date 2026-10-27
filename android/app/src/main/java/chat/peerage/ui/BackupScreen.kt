package chat.peerage.ui

import android.net.Uri
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
import androidx.compose.material.icons.filled.Lock
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
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.R
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

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
    var result by remember { mutableStateOf<BackupStatsDto?>(null) }
    var restoring by remember { mutableStateOf(false) }
    var pickedFile by remember { mutableStateOf<Uri?>(null) }

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
            val stats = model.exportBackupBlocking(tmp.absolutePath, parts, password)
            if (stats.error.isEmpty()) {
                runCatching {
                    ctx.contentResolver.openOutputStream(uri)?.use { out ->
                        tmp.inputStream().use { it.copyTo(out) }
                    }
                }
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
    ) { uri -> pickedFile = uri }

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
            password = password,
            repeat = repeat,
            onPassword = { password = it },
            onRepeat = { repeat = it },
            onGo = { saveTo.launch(defaultFileName()) },
        )

        Step.Working -> WorkingStep(restoring)

        Step.Restore -> RestoreStep(
            file = pickedFile,
            // Не lastPathSegment: у content:// это идентификатор документа («43»),
            // и человек восстанавливался бы вслепую. Имя знает провайдер.
            fileName = pickedFile?.let { displayName(ctx, it) },
            password = password,
            onPassword = { password = it },
            onPickFile = { openFrom.launch(arrayOf("*/*")) },
            onGo = {
                val uri = pickedFile ?: return@RestoreStep
                step = Step.Working
                restoring = true
                scope.launch(Dispatchers.IO) {
                    val tmp = java.io.File(ctx.cacheDir, "restore.tmp")
                    val copied = runCatching {
                        ctx.contentResolver.openInputStream(uri)?.use { inp ->
                            tmp.outputStream().use { inp.copyTo(it) }
                        } != null
                    }.getOrDefault(false)
                    result = if (copied) {
                        model.importBackupBlocking(tmp.absolutePath, password)
                    } else {
                        BackupStatsDto(error = ctx.getString(R.string.backup_read_failed))
                    }
                    tmp.delete()
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
        verticalArrangement = Arrangement.spacedBy(12.dp),
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
            stringResource(R.string.backup_n_messages, est.messages),
            null,
            parts.messages,
        ) { onParts(parts.copy(messages = it)) }

        PartRow(
            Icons.Filled.Groups,
            stringResource(R.string.backup_part_groups),
            stringResource(R.string.backup_n_group_messages, est.groupMessages),
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
            stringResource(R.string.backup_n_calls, est.calls),
            null,
            parts.calls,
        ) { onParts(parts.copy(calls = it)) }

        PartRow(
            Icons.Filled.Photo,
            stringResource(R.string.backup_part_attachments),
            if (est.skippedBig > 0) {
                stringResource(R.string.backup_n_attachments_skipped, est.attachments, est.skippedBig)
            } else {
                stringResource(R.string.backup_n_attachments, est.attachments)
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
        AccentButton(stringResource(R.string.backup_next), Modifier.fillMaxWidth()) {
            if (any) onNext()
        }
        OutlineButton(
            stringResource(R.string.backup_restore),
            Modifier.fillMaxWidth(),
            tint = Color.White,
            onClick = onRestore,
        )
        Spacer(Modifier.height(Tokens.Space.l))
    }
}

@Composable
private fun PasswordStep(
    password: String,
    repeat: String,
    onPassword: (String) -> Unit,
    onRepeat: (String) -> Unit,
    onGo: () -> Unit,
) {
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(ScreenPad),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        InfoCard(
            Icons.Filled.Lock,
            stringResource(R.string.backup_pw_title),
            stringResource(R.string.backup_pw_text),
        )

        FieldLabel(stringResource(R.string.backup_pw_label))
        TextField(
            password,
            onPassword,
            Modifier.fillMaxWidth(),
            colors = inputColors(),
            singleLine = true,
            placeholder = { Text(stringResource(R.string.vault_ph_password), color = HoloSecondary) },
            visualTransformation = PasswordVisualTransformation(),
        )
        FieldLabel(stringResource(R.string.backup_pw_repeat))
        TextField(
            repeat,
            onRepeat,
            Modifier.fillMaxWidth(),
            colors = inputColors(),
            singleLine = true,
            placeholder = { Text(stringResource(R.string.vault_ph_repeat), color = HoloSecondary) },
            isError = repeat.isNotEmpty() && repeat != password,
            visualTransformation = PasswordVisualTransformation(),
        )
        if (repeat.isNotEmpty() && repeat != password) {
            Text(stringResource(R.string.vault_mismatch), color = HoloError, fontSize = Tokens.Font.small)
        }
        Text(stringResource(R.string.backup_pw_no_recovery), color = HoloWarn, fontSize = Tokens.Font.small)

        val ready = password.isNotEmpty() && password == repeat
        AccentButton(stringResource(R.string.backup_create), Modifier.fillMaxWidth()) {
            if (ready) onGo()
        }
        Text(stringResource(R.string.backup_where_hint), color = HoloSecondary, fontSize = Tokens.Font.small)
    }
}

@Composable
private fun WorkingStep(restoring: Boolean) {
    Column(
        Modifier.fillMaxSize().padding(ScreenPad),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Spacer(Modifier.height(80.dp))
        Text(
            stringResource(if (restoring) R.string.backup_working_restore else R.string.backup_working),
            color = Color.White,
            fontSize = Tokens.Font.title,
            fontWeight = FontWeight.Medium,
        )
        Spacer(Modifier.height(20.dp))
        LinearProgressIndicator(color = HoloAccent, modifier = Modifier.fillMaxWidth())
        Spacer(Modifier.height(20.dp))
        // Про секунду сказано намеренно: без этого пауза читается как «подвисло».
        Text(stringResource(R.string.backup_working_hint), color = HoloSecondary, fontSize = Tokens.Font.small)
    }
}

@Composable
private fun RestoreStep(
    file: Uri?,
    fileName: String?,
    password: String,
    onPassword: (String) -> Unit,
    onPickFile: () -> Unit,
    onGo: () -> Unit,
) {
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(ScreenPad),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        SurfaceCard(onClick = onPickFile) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(Icons.Filled.Archive, null, tint = HoloAccent, modifier = Modifier.size(22.dp))
                Spacer(Modifier.width(12.dp))
                Column {
                    Text(
                        fileName ?: stringResource(R.string.backup_pick_file),
                        color = Color.White,
                        fontSize = Tokens.Font.label,
                    )
                    Text(
                        stringResource(R.string.backup_pick_hint),
                        color = HoloSecondary,
                        fontSize = Tokens.Font.small,
                        modifier = Modifier.padding(top = 2.dp),
                    )
                }
            }
        }

        FieldLabel(stringResource(R.string.backup_pw_label))
        TextField(
            password,
            onPassword,
            Modifier.fillMaxWidth(),
            colors = inputColors(),
            singleLine = true,
            placeholder = { Text(stringResource(R.string.vault_ph_password), color = HoloSecondary) },
            visualTransformation = PasswordVisualTransformation(),
        )

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

        AccentButton(stringResource(R.string.backup_restore_go), Modifier.fillMaxWidth()) {
            if (file != null && password.isNotEmpty()) onGo()
        }
    }
}

@Composable
private fun DoneStep(stats: BackupStatsDto?, restoring: Boolean, onDone: () -> Unit) {
    Column(
        Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(ScreenPad),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Spacer(Modifier.height(50.dp))
        val err = stats?.error.orEmpty()
        if (err.isNotEmpty()) {
            Text(stringResource(R.string.backup_failed), color = HoloError, fontSize = Tokens.Font.title, fontWeight = FontWeight.Medium)
            Text(err, color = HoloSecondary, fontSize = Tokens.Font.body)
        } else {
            Icon(Icons.Filled.CheckCircle, null, tint = HoloOnline, modifier = Modifier.size(46.dp))
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
                    Icon(Icons.Filled.Check, null, tint = HoloAccentDark, modifier = Modifier.size(15.dp))
                }
            }
            Spacer(Modifier.width(12.dp))
            Icon(icon, null, tint = HoloSecondary, modifier = Modifier.size(21.dp))
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(title, color = Color.White, fontSize = Tokens.Font.label)
                Text(sub, color = HoloSecondary, fontSize = Tokens.Font.small, modifier = Modifier.padding(top = 2.dp))
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
                Icon(icon, null, tint = tint, modifier = Modifier.size(20.dp))
                Spacer(Modifier.width(Tokens.Space.s))
                Text(title, color = tint, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium)
            }
            Text(
                text,
                color = HoloSecondary,
                fontSize = Tokens.Font.body,
                modifier = Modifier.padding(top = 6.dp),
            )
        }
    }
}
