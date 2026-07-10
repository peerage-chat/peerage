@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package dev.p2p.messenger.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Campaign
import androidx.compose.material.icons.filled.GroupAdd
import androidx.compose.material.icons.filled.PersonAdd
import androidx.compose.material.icons.filled.Verified
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.AppModel
import dev.p2p.messenger.HoloAccent
import dev.p2p.messenger.HoloOnline
import dev.p2p.messenger.HoloSecondary
import dev.p2p.messenger.HoloVerified
import dev.p2p.messenger.R
import dev.p2p.messenger.ScreenPad

/** Композер «Новый чат» (с FAB Главной): сверху действия — новая группа / канал
 *  (скоро) / добавить контакт, ниже — список контактов (тап → личный чат). */
@Composable
internal fun NewChatScreen(
    model: AppModel,
    onNewGroup: () -> Unit,
    onAddContact: () -> Unit,
    onOpenDm: (String) -> Unit,
) {
    val ctx = LocalContext.current
    LazyColumn(Modifier.fillMaxSize()) {
        item {
            ActionRow(Icons.Filled.GroupAdd, stringResource(R.string.title_new_group), HoloAccent, onClick = onNewGroup)
            ActionRow(
                Icons.Filled.Campaign,
                stringResource(R.string.newchat_channel),
                HoloSecondary,
                badge = stringResource(R.string.badge_soon),
            ) {
                android.widget.Toast.makeText(ctx, ctx.getString(R.string.channels_soon), android.widget.Toast.LENGTH_SHORT).show()
            }
            ActionRow(Icons.Filled.PersonAdd, stringResource(R.string.title_add_contact), HoloAccent, onClick = onAddContact)
            HorizontalDivider(color = dev.p2p.messenger.HoloBorder, modifier = Modifier.padding(vertical = 6.dp))
            Box(Modifier.padding(start = ScreenPad, top = 6.dp, bottom = 4.dp)) {
                FieldLabel(stringResource(R.string.newchat_contacts_header))
            }
        }
        items(model.contacts, key = { it.key }) { c ->
            val online = model.online[c.key] == true
            ListRow(onClick = { onOpenDm(c.key) }) {
                SquareAvatar(c.displayName, size = 44.dp, online = online, avatarBytes = model.contactAvatars[c.avatarId])
                Spacer(Modifier.width(12.dp))
                Column(Modifier.weight(1f)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            c.displayName,
                            color = Color.White,
                            fontSize = 15.sp,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.weight(1f, fill = false),
                        )
                        if (c.verified) {
                            Spacer(Modifier.width(4.dp))
                            Icon(Icons.Filled.Verified, null, tint = HoloVerified, modifier = Modifier.size(13.dp))
                        }
                    }
                    Text(
                        stringResource(if (online) R.string.status_online else R.string.status_offline),
                        color = if (online) HoloOnline else HoloSecondary,
                        fontSize = 11.sp,
                    )
                }
            }
        }
    }
}

/** Действие-строка композера: круглая иконка-акцент + подпись (+ опц. бейдж «скоро»). */
@Composable
private fun ActionRow(
    icon: ImageVector,
    label: String,
    tint: Color,
    badge: String? = null,
    onClick: () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth().clickable { onClick() }.padding(horizontal = ScreenPad, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.size(44.dp).clip(RoundedCornerShape(percent = 30)).background(tint.copy(alpha = 0.15f)),
            contentAlignment = Alignment.Center,
        ) {
            Icon(icon, null, tint = tint, modifier = Modifier.size(22.dp))
        }
        Spacer(Modifier.width(14.dp))
        Text(label, color = Color.White, fontSize = 15.sp, fontWeight = FontWeight.Medium, modifier = Modifier.weight(1f))
        if (badge != null) {
            Text(badge, color = HoloSecondary, fontSize = 11.sp)
        }
    }
}
