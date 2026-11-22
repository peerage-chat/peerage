package chat.peerage.ui

import android.os.Build
import android.provider.MediaStore
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import chat.peerage.AppModel
import chat.peerage.ChatTarget
import chat.peerage.R
import chat.peerage.ShareDraft
import chat.peerage.SharedItem
import chat.peerage.Tokens
import chat.peerage.isSaved
import chat.peerage.sendShared
import chat.peerage.shareTargets
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Шапка «Отправить в Peerage»: крестик — отказ от присланного целиком. */
@Composable
internal fun ShareTopBar(onClose: () -> Unit) {
    TopBar(
        stringResource(R.string.share_in_title, stringResource(R.string.app_name)),
        leading = { BarIcon(TablerIcons.X, stringResource(R.string.common_cancel), onClick = onClose) },
    )
}

/**
 * «Поделиться» в Peerage (макет `android/Main`, app-ui.md → «Встраивание в Android»): что пришло,
 * «Кому», чаты с отметками, подпись, «Отправить в N чатов». Один чат — [onOpenOne]: вложение
 * ждёт в его поле ввода. Несколько — отправка сразу и [onSent].
 */
@Composable
internal fun ShareScreen(model: AppModel, onOpenOne: (ChatTarget) -> Unit, onSent: () -> Unit) {
    val draft = model.shareDraft ?: return
    val ctx = LocalContext.current
    var query by remember { mutableStateOf("") }
    val picked = remember(draft) { mutableStateListOf<ChatTarget>() }
    // Текст, пришедший вместе с файлами (тема письма, ссылка), — готовая подпись.
    var caption by remember(draft) { mutableStateOf(if (draft.items.isEmpty()) "" else draft.text) }
    val all = model.shareTargets()
    val shown = remember(all, query) { all.filter { matches(model, it, query) } }
    Column(Modifier.fillMaxSize()) {
        LazyColumn(Modifier.weight(1f), contentPadding = PaddingValues(top = Tokens.Space.l)) {
            item(key = "what") { SharedSummary(draft) }
            item(key = "search") { ContactSearch(query, hint = stringResource(R.string.share_in_to)) { query = it } }
            itemsIndexed(shown, key = { _, t -> (if (t.group) "g" else "d") + t.id }) { i, t ->
                CardBlock(first = i == 0, last = i == shown.lastIndex) {
                    ShareChatRow(model, t, divider = i > 0, on = t in picked) {
                        if (!picked.remove(t)) picked.add(t)
                    }
                }
            }
            if (draft.items.isNotEmpty()) {
                item(key = "caption") {
                    FormInput(
                        caption,
                        stringResource(R.string.share_in_caption),
                        Modifier.fillMaxWidth().padding(start = Tokens.Space.panel, end = Tokens.Space.panel, top = Tokens.Space.l),
                    ) { caption = it }
                }
            }
        }
        val n = picked.size
        ScreenFooter(
            if (n == 0) stringResource(R.string.share_in_pick) else pluralStringResource(R.plurals.share_in_send_to, n, n),
            enabled = n > 0,
        ) {
            if (n == 1) {
                // Один чат — открыть его: вложение ждёт в поле, подпись уходит туда же.
                model.shareDraft = draft.copy(text = if (draft.items.isEmpty()) draft.text else caption.trim(), target = picked[0])
                onOpenOne(picked[0])
            } else {
                model.sendShared(ctx, draft, picked.toList(), caption.trim())
                onSent()
            }
        }
    }
}

/** Что пришло: миниатюры (до трёх) и «2 фото · 4,1 МБ»; только текст — сам текст. */
@Composable
private fun SharedSummary(draft: ShareDraft) {
    Row(
        Modifier.fillMaxWidth().padding(start = Tokens.Space.panel, end = Tokens.Space.panel, bottom = Tokens.Space.l),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s),
    ) {
        if (draft.items.isEmpty()) {
            Text(
                draft.text,
                color = Color.White,
                fontSize = Tokens.Font.label,
                lineHeight = Tokens.Font.lineHeight,
                maxLines = 4,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(Tokens.Radius.field))
                    .background(Tokens.Colors.surface).padding(Tokens.Space.l),
            )
            return@Row
        }
        draft.items.take(SHARE_THUMBS).forEach { SharedThumb(it) }
        Spacer(Modifier.size(Tokens.Space.xs))
        Text(sharedLabel(draft), color = Tokens.Colors.secondary, fontSize = Tokens.Font.body, maxLines = 2)
    }
}

private const val SHARE_THUMBS = 3

/** «2 фото · 4,1 МБ», «1 видео · 12 МБ», «3 файла · 1,2 МБ»; размер неизвестен — без него. */
@Composable
internal fun sharedLabel(draft: ShareDraft): String {
    val n = draft.items.size
    val kind = when {
        draft.items.all { it.isImage } -> pluralStringResource(R.plurals.share_in_photos, n, n)
        draft.items.all { it.isVideo } -> pluralStringResource(R.plurals.share_in_videos, n, n)
        else -> pluralStringResource(R.plurals.share_in_files, n, n)
    }
    val known = draft.items.all { it.size >= 0 }
    return if (known) "$kind · ${fmtSizeLong(draft.totalBytes)}" else kind
}

/** Миниатюра присланного: картинка/кадр видео или значок файла на плашке. */
@Composable
private fun SharedThumb(item: SharedItem) {
    val ctx = LocalContext.current
    var bmp by remember(item.uri) { mutableStateOf<ImageBitmap?>(null) }
    if (item.isImage || item.isVideo) {
        LaunchedEffect(item.uri) {
            bmp = withContext(Dispatchers.IO) {
                runCatching {
                    if (Build.VERSION.SDK_INT >= 29) {
                        ctx.contentResolver.loadThumbnail(item.uri, android.util.Size(THUMB_PX, THUMB_PX), null)
                    } else {
                        @Suppress("DEPRECATION")
                        MediaStore.Images.Media.getBitmap(ctx.contentResolver, item.uri)
                    }
                }.getOrNull()?.asImageBitmap()
            }
        }
    }
    val shape = RoundedCornerShape(Tokens.Radius.field)
    Box(
        Modifier.size(Tokens.Size.shareThumb).clip(shape).background(Tokens.Colors.raised),
        contentAlignment = Alignment.Center,
    ) {
        val b = bmp
        if (b != null) {
            Image(b, null, Modifier.fillMaxSize().imageOutline(shape), contentScale = ContentScale.Crop)
        } else {
            Icon(TablerIcons.File, null, tint = Tokens.Colors.secondary, modifier = Modifier.size(Tokens.Size.iconM))
        }
    }
}

private const val THUMB_PX = 256

/** Строка выбора: «Сохранённое» — «заметки себе», контакт — сеть, группа — участники. */
@Composable
private fun ShareChatRow(model: AppModel, t: ChatTarget, divider: Boolean, on: Boolean, onClick: () -> Unit) {
    val saved = !t.group && model.isSaved(t.id)
    val (name, avatar) = if (saved) stringResource(R.string.menu_saved) to null else chatFace(model, t)
    val sub = when {
        saved -> stringResource(R.string.share_in_saved_sub)
        t.group -> {
            val n = model.groups.firstOrNull { it.gid == t.id }?.members ?: 0
            pluralStringResource(R.plurals.search_group_subtitle, n, n)
        }
        else -> stringResource(if (model.online[t.id] == true) R.string.status_in_network else R.string.status_offline)
    }
    SearchRow(divider, onClick, avatar = { RowAvatar(name, avatar, t.group, saved) }) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(name, color = Color.White, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(sub, color = Tokens.Colors.tertiary, fontSize = Tokens.Font.body, maxLines = 1, modifier = Modifier.padding(top = Tokens.Space.xxs))
            }
            SelectCheck(on)
        }
    }
}

private fun matches(model: AppModel, t: ChatTarget, query: String): Boolean {
    val q = query.trim().lowercase()
    if (q.isEmpty()) return true
    val name = when {
        t.group -> model.groups.firstOrNull { it.gid == t.id }?.name.orEmpty()
        model.isSaved(t.id) -> model.appContext?.getString(R.string.menu_saved).orEmpty()
        else -> model.contacts.firstOrNull { it.key == t.id }?.displayName.orEmpty()
    }
    return name.lowercase().contains(q)
}
