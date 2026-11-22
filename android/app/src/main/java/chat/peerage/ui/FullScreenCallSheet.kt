package chat.peerage.ui

import android.content.Intent
import android.net.Uri
import android.provider.Settings
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import chat.peerage.AppModel
import chat.peerage.R

/**
 * Подсказка Android 14+ (notifications.md, п. 7): без разрешения на полноэкранный интент
 * входящий на заблокированном экране — только уведомлением. Один раз, после первого такого
 * звонка; вид — как разрешение по одному (макет `onboard/PermsB`).
 */
@Composable
internal fun FullScreenCallSheet(model: AppModel) {
    val ctx = LocalContext.current
    ActionSheet(stringResource(R.string.fsi_title), stringResource(R.string.fsi_text), model::dismissFullScreenHint) {
        PlateButton(stringResource(R.string.onb_allow), modifier = Modifier.fillMaxWidth(), primary = true) {
            model.dismissFullScreenHint()
            runCatching {
                ctx.startActivity(Intent(Settings.ACTION_MANAGE_APP_USE_FULL_SCREEN_INTENT, Uri.parse("package:" + ctx.packageName)))
            }
        }
        PlateButton(stringResource(R.string.fsi_later), modifier = Modifier.fillMaxWidth(), onClick = model::dismissFullScreenHint)
    }
}
