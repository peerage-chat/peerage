@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package chat.peerage.ui

import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Campaign
import androidx.compose.material.icons.filled.GroupAdd
import androidx.compose.material.icons.filled.PersonAdd
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import chat.peerage.AppModel
import chat.peerage.HoloAccent
import chat.peerage.R
import chat.peerage.Tokens

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
    LazyColumn(Modifier.fillMaxSize().padding(top = Tokens.Space.s)) {
        item {
            // Действия — своей карточкой: это не «первые строки списка контактов»,
            // а отдельный набор, и разрыв между ними должен читаться.
            CardSection {
                ActionRow(
                    Icons.Filled.GroupAdd,
                    stringResource(R.string.title_new_group),
                    HoloAccent,
                    divider = false,
                    onClick = onNewGroup,
                )
                ActionRow(
                    Icons.Filled.Campaign,
                    stringResource(R.string.newchat_channel),
                    Tokens.Colors.secondary,
                    badge = stringResource(R.string.badge_soon),
                ) {
                    android.widget.Toast.makeText(ctx, ctx.getString(R.string.channels_soon), android.widget.Toast.LENGTH_SHORT).show()
                }
                ActionRow(
                    Icons.Filled.PersonAdd,
                    stringResource(R.string.title_add_contact),
                    HoloAccent,
                    onClick = onAddContact,
                )
            }
            SectionTitle(stringResource(R.string.newchat_contacts_header))
        }
        itemsIndexed(model.contacts, key = { _, c -> c.key }) { i, c ->
            ContactCardRow(model, c, first = i == 0, last = i == model.contacts.lastIndex) { onOpenDm(c.key) }
        }
        item { Spacer(Modifier.height(Tokens.Space.l)) }
    }
}

@Composable
private fun ActionRow(
    icon: ImageVector,
    title: String,
    tint: Color,
    badge: String? = null,
    divider: Boolean = true,
    onClick: () -> Unit,
) {
    CardRow(onClick = onClick, divider = divider) {
        Icon(icon, null, tint = tint, modifier = Modifier.size(Tokens.Size.iconM))
        Spacer(Modifier.width(Tokens.Space.m))
        Text(title, color = if (tint == HoloAccent) Color.White else tint, fontSize = Tokens.Font.label, modifier = Modifier.weight(1f))
        if (badge != null) TagBadge(badge)
    }
}
