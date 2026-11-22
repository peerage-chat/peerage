package chat.peerage.ui

import androidx.compose.runtime.Composable
import androidx.compose.ui.res.stringResource
import chat.peerage.*
import chat.peerage.R

/**
 * Шапка «Сохранённого» (макет `misc/Main`): «назад», заголовок, поиск. Лента и ввод —
 * общий [ChatScreen]: «Сохранённое» — личный чат с собой ([isSaved]), поэтому дни,
 * пересланное, вложения, голосовые, правка, закреп и поиск в нём те же, что в чате.
 * Выбор и поиск забирают шапку целиком — как в любом чате.
 */
@Composable
internal fun SavedTopBar(model: AppModel, onBack: () -> Unit) {
    val me = model.me?.pubkey ?: return
    if (model.selection.isNotEmpty()) {
        SelectionTopBar(model)
        return
    }
    model.chatSearch?.takeIf { !it.chat.group && it.chat.id == me }?.let {
        ChatSearchTopBar(model, it)
        return
    }
    TopBar(stringResource(R.string.menu_saved), onBack = onBack) {
        BarIcon(TablerIcons.Search, stringResource(R.string.chat_menu_search)) {
            model.openChatSearch(ChatTarget(me, group = false))
        }
    }
}
