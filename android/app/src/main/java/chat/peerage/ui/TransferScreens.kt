package chat.peerage.ui

import android.content.Context
import android.net.Uri
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import chat.peerage.AppModel
import chat.peerage.BackupStatsDto
import chat.peerage.Native
import chat.peerage.R
import chat.peerage.Tokens
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File

/** Выбранный файл копии: уже скопирован во временный, заголовок прочитан. */
internal data class PickedBackup(val name: String, val size: Long, val needsPassword: Boolean)

/**
 * Скопировать копию из `content://` во временный файл (движок читает только по пути) и
 * прочитать заголовок: закрыта ли паролем (шаг 7k). null — файл не прочесть или это не копия.
 */
internal fun pickBackup(ctx: Context, uri: Uri, tmp: File): PickedBackup? {
    val copied = runCatching {
        ctx.contentResolver.openInputStream(uri)?.use { inp -> tmp.outputStream().use { inp.copyTo(it) } } != null
    }.getOrDefault(false)
    if (!copied) return null
    val pw = Native.backupNeedsPassword(tmp.absolutePath)
    if (pw < 0) return null
    return PickedBackup(displayName(ctx, uri) ?: tmp.name, tmp.length(), pw == 1)
}

private enum class TransferStep { Offer, Password, Working, Done, Name }

/**
 * Перенос переписки после восстановления по 24 словам (макеты `onboard/Restored`,
 * `onboard/BackupPass`): файл копии → пароль, только если копия им закрыта → итог.
 * Копию открывает только та же личность, а она здесь уже восстановлена.
 */
@Composable
internal fun TransferFlow(model: AppModel, onDone: () -> Unit) {
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    var step by remember { mutableStateOf(TransferStep.Offer) }
    var file by remember { mutableStateOf<PickedBackup?>(null) }
    var password by remember { mutableStateOf("") }
    var error by remember { mutableStateOf<String?>(null) }
    var stats by remember { mutableStateOf<BackupStatsDto?>(null) }
    val tmp = remember { File(ctx.cacheDir, "transfer.tmp") }
    // Копия переписки во временном файле не должна пережить экран.
    DisposableEffect(Unit) { onDispose { tmp.delete() } }

    val restore: () -> Unit = {
        val back = if (file?.needsPassword == true) TransferStep.Password else TransferStep.Offer
        step = TransferStep.Working
        scope.launch(Dispatchers.IO) {
            val r = model.importBackupBlocking(tmp.absolutePath, password)
            withContext(Dispatchers.Main) {
                if (r.error.isEmpty()) {
                    tmp.delete()
                    stats = r
                    step = TransferStep.Done
                } else {
                    error = r.error
                    step = back
                }
            }
        }
    }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch(Dispatchers.IO) {
            val picked = pickBackup(ctx, uri, tmp)
            withContext(Dispatchers.Main) {
                error = null
                password = ""
                file = picked
                when {
                    picked == null -> {
                        error = ctx.getString(R.string.transfer_not_backup)
                        step = TransferStep.Offer
                    }
                    picked.needsPassword -> step = TransferStep.Password
                    else -> restore()
                }
            }
        }
    }
    val pickFile = { pick.launch(arrayOf("*/*")) }
    // «Как подписаться» — только если имени не вернула копия (шаг 7g).
    val finish = { if (model.selfName.isBlank()) step = TransferStep.Name else onDone() }

    when (step) {
        TransferStep.Offer -> OnbPage(
            bar = 0,
            onBack = null,
            title = stringResource(R.string.transfer_title),
            sub = stringResource(R.string.transfer_sub),
            primary = stringResource(R.string.transfer_pick),
            onPrimary = pickFile,
            icon = TablerIcons.Check,
            iconTint = Tokens.Colors.online,
            secondary = stringResource(R.string.transfer_skip),
            onSecondary = finish,
        ) {
            Spacer(Modifier.height(Tokens.Space.xxl))
            OnbGroup {
                OnbRow(TablerIcons.Archive, stringResource(R.string.transfer_row), stringResource(R.string.transfer_row_sub), divider = false)
            }
            Spacer(Modifier.height(Tokens.Space.m))
            OnbCaption(stringResource(R.string.transfer_caption))
            error?.let { OnbError(it) }
        }

        TransferStep.Password -> OnbPage(
            bar = 0,
            onBack = { step = TransferStep.Offer },
            title = stringResource(R.string.transfer_pw_title),
            sub = stringResource(R.string.transfer_pw_sub),
            primary = stringResource(R.string.transfer_restore),
            onPrimary = restore,
            primaryEnabled = password.isNotEmpty(),
            secondary = stringResource(R.string.common_back),
            onSecondary = { step = TransferStep.Offer },
        ) {
            Spacer(Modifier.height(Tokens.Space.xxl))
            val f = file
            if (f != null) {
                OnbGroup {
                    OnbRow(TablerIcons.File, f.name, fmtSizeLong(f.size), divider = false) {
                        PillButton(stringResource(R.string.transfer_other), onClick = pickFile)
                    }
                }
            }
            Spacer(Modifier.height(Tokens.Space.m))
            PasswordField(password, error = error != null, hint = stringResource(R.string.vault_ph_password)) {
                password = it
                error = null
            }
            error?.let { OnbError(it) }
        }

        TransferStep.Working -> OnbPage(
            bar = 0,
            onBack = null,
            title = stringResource(R.string.transfer_working),
            sub = null,
            primary = stringResource(R.string.transfer_restore),
            onPrimary = {},
            primaryEnabled = false,
        ) {
            Spacer(Modifier.height(Tokens.Space.xxl))
            LinearProgressIndicator(color = Tokens.Colors.accent, trackColor = Tokens.Colors.raised, modifier = Modifier.fillMaxWidth())
        }

        TransferStep.Done -> OnbPage(
            bar = 0,
            onBack = null,
            title = stringResource(R.string.transfer_done),
            sub = null,
            primary = stringResource(R.string.common_done),
            onPrimary = finish,
            icon = TablerIcons.Check,
            iconTint = Tokens.Colors.online,
        ) {
            Spacer(Modifier.height(Tokens.Space.xxl))
            RestoredCounts(stats ?: BackupStatsDto())
            Spacer(Modifier.height(Tokens.Space.m))
            OnbCaption(stringResource(R.string.transfer_done_caption))
        }

        TransferStep.Name -> ProfileStep(model, onBack = null, bar = 0, onNext = onDone)
    }
}

/** Что добавилось из копии: части с нулём не показываются. */
@Composable
internal fun RestoredCounts(s: BackupStatsDto) {
    val rows = listOf(
        R.string.restored_contacts to s.contacts,
        R.string.restored_messages to s.messages,
        R.string.restored_groups to s.groupMessages,
        R.string.restored_notes to s.notes,
        R.string.restored_attachments to s.attachments,
    ).filter { it.second > 0 }
    if (rows.isEmpty()) return
    OnbGroup {
        rows.forEachIndexed { i, (label, n) ->
            if (i > 0) HorizontalDivider(thickness = 1.dp, color = Tokens.Colors.raised)
            androidx.compose.foundation.layout.Row(
                Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.m + Tokens.Space.xxs),
            ) {
                Text(stringResource(label), color = Color.White, fontSize = Tokens.Font.label, modifier = Modifier.weight(1f))
                Text(fmtCount(n), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.option)
            }
        }
    }
    Spacer(Modifier.height(Tokens.Space.xs))
}

/** «4 812» — пробел-разделитель тысяч, как в макетах. */
private fun fmtCount(n: Long): String = String.format(java.util.Locale.getDefault(), "%,d", n)
