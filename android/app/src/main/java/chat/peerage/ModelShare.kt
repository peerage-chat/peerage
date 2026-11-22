package chat.peerage

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.OpenableColumns

/**
 * «Поделиться» в Peerage (app-ui.md → «Встраивание в Android», шаг 7d): то, что прислала
 * другая программа через `ACTION_SEND` / `ACTION_SEND_MULTIPLE`, ждёт здесь, пока человек не
 * выберет, кому. Байты не копируются заранее: читаются при отправке, пока жив доступ к uri.
 */

/** Присланный файл: uri с выданным доступом и то, что о нём известно до чтения. */
data class SharedItem(val uri: Uri, val mime: String, val name: String, val size: Long) {
    val isImage: Boolean get() = mime.startsWith("image/")
    val isVideo: Boolean get() = mime.startsWith("video/")
}

/**
 * Присланное целиком. [target] — чат, выбранный ещё в системном меню (ярлык чата), или
 * выбранный на экране «Отправить в Peerage», когда чат один: тогда вложение ждёт в его поле.
 */
data class ShareDraft(val items: List<SharedItem>, val text: String, val target: ChatTarget? = null) {
    val totalBytes: Long get() = items.sumOf { it.size.coerceAtLeast(0) }
}

/** Ярлыки на значке и ярлыки чатов: что открыть, когда приложение откроется. */
sealed interface LaunchTarget {
    data object AddContact : LaunchTarget
    data object MyCode : LaunchTarget
    data class Chat(val chat: ChatTarget) : LaunchTarget
}

internal const val ACTION_ADD_CONTACT = "chat.peerage.ADD_CONTACT"
internal const val ACTION_MY_CODE = "chat.peerage.MY_CODE"
internal const val ACTION_OPEN_CHAT = "chat.peerage.OPEN_CHAT"

/**
 * Разобрать входящий интент: «Поделиться» → [AppModel.shareDraft], ярлык → [AppModel.launchTarget].
 * Активити экспортирована, поэтому всё — только ДАННЫЕ, а не команды: худшее, что может
 * чужой интент, — открыть экран выбора чата или экран своего кода.
 */
internal fun AppModel.acceptIntent(ctx: Context, i: Intent?) {
    when (i?.action) {
        Intent.ACTION_SEND, Intent.ACTION_SEND_MULTIPLE -> {
            val draft = parseShare(ctx, i) ?: return
            val target = i.getStringExtra(EXTRA_SHORTCUT_ID)?.let(::chatOfShortcut)
            shareDraft = draft.copy(target = target)
        }
        ACTION_ADD_CONTACT -> launchTarget = LaunchTarget.AddContact
        ACTION_MY_CODE -> launchTarget = LaunchTarget.MyCode
        ACTION_OPEN_CHAT -> i.getStringExtra(EXTRA_SHORTCUT_CHAT)?.let(::chatOfShortcut)?.let { launchTarget = LaunchTarget.Chat(it) }
        else -> return
    }
    // Пересоздание активити не должно принять то же самое второй раз.
    i.action = null
}

private const val EXTRA_SHORTCUT_ID = "android.intent.extra.shortcut.ID" // Intent.EXTRA_SHORTCUT_ID (API 29)
internal const val EXTRA_SHORTCUT_CHAT = "chat.peerage.CHAT"

/** Id ярлыка чата: «dm:<ключ>» / «g:<gid>». */
internal fun shortcutIdOf(chat: ChatTarget): String = (if (chat.group) "g:" else "dm:") + chat.id

internal fun chatOfShortcut(id: String): ChatTarget? = when {
    id.startsWith("dm:") -> ChatTarget(id.removePrefix("dm:"), group = false)
    id.startsWith("g:") -> ChatTarget(id.removePrefix("g:"), group = true)
    else -> null
}?.takeIf { it.id.matches(HEX) }

private val HEX = Regex("[0-9a-f]{16,128}")

private fun parseShare(ctx: Context, i: Intent): ShareDraft? {
    val uris: List<Uri> = if (i.action == Intent.ACTION_SEND_MULTIPLE) {
        streamExtras(i)
    } else {
        listOfNotNull(streamExtra(i))
    }
    // Ссылку на свои файлы приложения мы не принимаем: «поделиться» не должно выводить
    // наружу то, что лежит в его собственной песочнице.
    val own = "content://${ctx.packageName}."
    val items = uris.filterNot { it.toString().startsWith(own) || it.scheme == "file" }.map { describe(ctx, it, i.type) }
    val text = listOfNotNull(i.getStringExtra(Intent.EXTRA_SUBJECT), i.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString())
        .filter { it.isNotBlank() }
        .distinct()
        .joinToString("\n")
    if (items.isEmpty() && text.isBlank()) return null
    return ShareDraft(items, text)
}

@Suppress("DEPRECATION")
private fun streamExtra(i: Intent): Uri? =
    if (Build.VERSION.SDK_INT >= 33) i.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java) else i.getParcelableExtra(Intent.EXTRA_STREAM)

@Suppress("DEPRECATION")
private fun streamExtras(i: Intent): List<Uri> =
    (if (Build.VERSION.SDK_INT >= 33) i.getParcelableArrayListExtra(Intent.EXTRA_STREAM, Uri::class.java) else i.getParcelableArrayListExtra(Intent.EXTRA_STREAM))
        .orEmpty()

private fun describe(ctx: Context, uri: Uri, fallbackMime: String?): SharedItem {
    val cr = ctx.contentResolver
    val mime = runCatching { cr.getType(uri) }.getOrNull() ?: fallbackMime?.takeUnless { it.endsWith("/*") } ?: "application/octet-stream"
    var name = ""
    var size = -1L
    runCatching {
        cr.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE), null, null, null)?.use { c ->
            if (c.moveToFirst()) {
                name = c.getString(0).orEmpty()
                if (!c.isNull(1)) size = c.getLong(1)
            }
        }
    }
    return SharedItem(uri, mime, name.substringAfterLast('/').ifBlank { "file" }, size)
}

/**
 * Кому можно отправить присланное: «Сохранённое», потом чаты с перепиской (свежие выше),
 * потом остальные контакты и группы по имени. Архивные группы — нет: писать в них нельзя.
 */
internal fun AppModel.shareTargets(): List<ChatTarget> {
    val me = me?.pubkey
    val open = groups.filterNot { it.archived }.map { it.gid }.toSet()
    val ok = { t: ChatTarget -> if (t.group) t.id in open else contacts.any { it.key == t.id } }
    val recent = chatSummaries.values
        .filter { it.lastTime > 0 && !(it.id == me && !it.group) }
        .sortedByDescending { it.lastTime }
        .map { ChatTarget(it.id, it.group) }
        .filter(ok)
    val rest = (contacts.map { ChatTarget(it.key, false) to it.displayName } + groups.filterNot { it.archived }.map { ChatTarget(it.gid, true) to it.name })
        .filter { (t, _) -> t !in recent }
        .sortedBy { (_, name) -> name.lowercase() }
        .map { it.first }
    return listOfNotNull(me?.let { ChatTarget(it, false) }) + recent + rest
}

/** Чат ещё существует: ярлык мог пережить удалённый контакт или покинутую группу. */
internal fun AppModel.hasChat(t: ChatTarget): Boolean = when {
    t.group -> groups.any { it.gid == t.id && !it.archived }
    else -> isSaved(t.id) || contacts.any { it.key == t.id }
}

/** Вложения, ждущие в поле ввода чата [chat] (один выбранный чат на «Отправить в Peerage»). */
internal fun AppModel.stagedShare(chat: ChatTarget): ShareDraft? =
    shareDraft?.takeIf { it.target == chat && it.items.isNotEmpty() }

/**
 * Текст присланного — в поле ввода чата, один раз: дальше это обычный набор. Без вложений
 * присланное на этом исчерпано.
 */
internal fun AppModel.takeSharedText(chat: ChatTarget): String? {
    val d = shareDraft?.takeIf { it.target == chat } ?: return null
    shareDraft = if (d.items.isEmpty()) null else d.copy(text = "")
    return d.text.takeIf { it.isNotBlank() }
}

/** Отправить ждущее в поле [chat] с подписью [caption]; присланное исчерпано. */
internal fun AppModel.sendStaged(ctx: Context, chat: ChatTarget, caption: String) {
    val d = stagedShare(chat) ?: return
    shareDraft = null
    sendShared(ctx, d, listOf(chat), caption)
}

/**
 * Отправить присланное в [targets]. Подпись — у первого вложения каждого чата (как у серии из
 * «Прикрепить»); без вложений текст уходит сообщением. Свой поток на всё присланное: вложения
 * одного чата уходят по порядку, а чтение uri не держит ни [AppModel.ops], ни ленту.
 */
internal fun AppModel.sendShared(ctx: Context, draft: ShareDraft, targets: List<ChatTarget>, caption: String) {
    shareDraft = null
    // Ответ, начатый в каком-то чате, к рассылке не относится; в одном чате — относится.
    if (targets.size > 1) cancelReply()
    if (draft.items.isEmpty()) {
        targets.forEach { send(it.id, draft.text, group = it.group) }
        return
    }
    val app = ctx.applicationContext
    kotlin.concurrent.thread(name = "share-send", isDaemon = true) {
        targets.forEach { t ->
            draft.items.forEachIndexed { i, item ->
                runCatching {
                    chat.peerage.ui.sendUriNow(app, this, t.id, item.uri, item.name, if (i == 0) caption else "", t.group)
                }
            }
        }
    }
}
