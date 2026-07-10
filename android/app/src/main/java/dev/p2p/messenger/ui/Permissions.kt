package dev.p2p.messenger.ui

import android.content.pm.PackageManager
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.platform.LocalContext

/** Разрешение уже выдано? */
internal fun android.content.Context.hasPermission(perm: String): Boolean =
    checkSelfPermission(perm) == PackageManager.PERMISSION_GRANTED

/**
 * Контекстный запрос разрешений (M4): действие выполняется ТОЛЬКО после гранта,
 * а сам системный диалог показывается ровно в момент, когда разрешение нужно —
 * перед звонком, перед включением камеры, перед сканером. Никаких запросов
 * «авансом» на старте приложения: пользователь не понимает, за что его спросили,
 * и отказ там навсегда ломает функцию.
 *
 * Возвращает `run(perms, denyMsg, action)`. При отказе — короткий тост с причиной,
 * действие не выполняется. Повторный вызов снова покажет диалог (или системные
 * настройки, если пользователь выбрал «больше не спрашивать» — это уже поведение ОС).
 */
@Composable
internal fun rememberPermissionRunner(): (Array<String>, Int, () -> Unit) -> Unit {
    val ctx = LocalContext.current
    var pending by remember { mutableStateOf<(() -> Unit)?>(null) }
    var denyMsg by remember { mutableStateOf(0) }
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) { result ->
        val action = pending
        pending = null
        if (result.values.all { it }) {
            action?.invoke()
        } else if (denyMsg != 0) {
            Toast.makeText(ctx, ctx.getString(denyMsg), Toast.LENGTH_SHORT).show()
        }
    }
    return { perms, msg, action ->
        val need = perms.filterNot { ctx.hasPermission(it) }
        if (need.isEmpty()) {
            action()
        } else {
            pending = action
            denyMsg = msg
            launcher.launch(need.toTypedArray())
        }
    }
}
