@file:OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)

package dev.p2p.messenger.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Bookmark
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.ContentCopy
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.p2p.messenger.*
import dev.p2p.messenger.R

/**
 * «Сохранённое» — заметки себе.
 *
 * Хранятся как обычные сообщения в чате с самим собой: никуда не отправляются, и
 * статусов доставки у них нет — отправлять некому. Отдельного хранилища заводить
 * незачем, история уже умеет и шифрование at-rest, и поиск.
 */
@Composable
internal fun SavedScreen(model: AppModel) {
    var input by remember { mutableStateOf("") }
    val listState = rememberLazyListState()
    // Выделение живёт в модели, как в чатах: панель действий рисуется НАД лентой
    // (в отличие от чата, где её место занимает общая верхняя панель экрана).
    val selection = model.selection
    LaunchedEffect(Unit) { model.refreshNotes() }
    // Вниз — только на НОВУЮ заметку: по размеру списка лента прыгала бы и при
    // догрузке старых (см. ChatScreen).
    val lastNote = model.notes.lastOrNull()?.id
    LaunchedEffect(lastNote) {
        if (model.notes.isNotEmpty()) listState.animateScrollToItem(model.notes.size - 1)
    }
    PagingUp(listState, Unit) { done -> model.loadOlderNotes(done) }

    Column(Modifier.fillMaxSize()) {
        if (selection.isNotEmpty()) SavedSelectionBar(model)
        if (model.notes.isEmpty()) {
            Box(Modifier.weight(1f), contentAlignment = Alignment.Center) {
                EmptyState(
                    Icons.Filled.Bookmark,
                    stringResource(R.string.saved_empty_title),
                    stringResource(R.string.saved_empty_hint),
                )
            }
        } else {
            LazyColumn(
                state = listState,
                modifier = Modifier.weight(1f).fillMaxWidth().padding(horizontal = 10.dp),
            ) {
                items(model.notes, key = { it.id }) { n ->
                    val selected = MsgRef(n.id, n.mine) in selection
                    Row(
                        Modifier.fillMaxWidth().padding(bottom = Tokens.Space.s)
                            .background(if (selected) HoloAccent.copy(alpha = 0.16f) else Color.Transparent),
                        horizontalArrangement = Arrangement.End,
                    ) {
                        Box(
                            Modifier.widthIn(max = 290.dp).clip(RoundedCornerShape(Tokens.Radius.preview))
                                .background(BubbleOut)
                                .then(
                                    if (selected) {
                                        Modifier.border(1.dp, HoloAccent, RoundedCornerShape(Tokens.Radius.preview))
                                    } else {
                                        Modifier
                                    },
                                )
                                .combinedClickable(
                                    onLongClick = { model.toggleSelect(n) },
                                    onClick = { if (selection.isNotEmpty()) model.toggleSelect(n) },
                                )
                                .padding(horizontal = 10.dp, vertical = 6.dp),
                        ) {
                            Text(
                                n.text,
                                color = Color.White,
                                fontSize = 15.sp,
                                modifier = Modifier.padding(end = bubbleTimeReserve(false)),
                            )
                            Text(
                                fmtMsgTime(n.time),
                                color = Color.White.copy(alpha = 0.5f),
                                fontSize = 11.sp,
                                modifier = Modifier.align(Alignment.BottomEnd),
                            )
                        }
                    }
                }
                item { Spacer(Modifier.height(Tokens.Space.s)) }
            }
        }

        // Ввод как в чате, но без скрепки и микрофона: вложения себе — отдельная
        // работа (блоб надо положить и связать), а рисовать неработающее нельзя.
        Row(
            Modifier.fillMaxWidth().background(HoloBg).padding(horizontal = 8.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                Modifier.weight(1f).height(44.dp).clip(RoundedCornerShape(Tokens.Radius.pill))
                    .background(HoloSurface).padding(horizontal = Tokens.Space.l),
                contentAlignment = Alignment.CenterStart,
            ) {
                androidx.compose.foundation.text.BasicTextField(
                    value = input,
                    onValueChange = { input = it },
                    textStyle = androidx.compose.ui.text.TextStyle(color = Color.White, fontSize = 15.sp),
                    cursorBrush = androidx.compose.ui.graphics.SolidColor(HoloAccent),
                    maxLines = 4,
                    modifier = Modifier.fillMaxWidth(),
                    decorationBox = { inner ->
                        if (input.isEmpty()) {
                            Text(stringResource(R.string.saved_hint), color = HoloSecondary, fontSize = 15.sp)
                        }
                        inner()
                    },
                )
            }
            Spacer(Modifier.width(Tokens.Space.s))
            SendButton {
                if (input.isNotBlank()) {
                    model.saveNote(input.trim())
                    input = ""
                }
            }
        }
    }
}

/**
 * Действия над выделенными заметками: только копировать и удалить.
 *
 * «Удалить у всех» здесь не бывает по устройству вещи: заметка — сообщение самому
 * себе, отправлять и отзывать её не у кого.
 */
@Composable
private fun SavedSelectionBar(model: AppModel) {
    val ctx = LocalContext.current
    val sel = model.selection
    val myKey = model.me?.pubkey.orEmpty()
    var confirm by remember { mutableStateOf(false) }

    Row(
        Modifier.fillMaxWidth().background(HoloSurface).padding(horizontal = 8.dp, vertical = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        BarIcon(Icons.Filled.Close, stringResource(R.string.common_cancel)) { model.clearSelection() }
        Text(
            sel.size.toString(),
            color = Color.White,
            fontSize = 16.sp,
            fontWeight = FontWeight.Medium,
            modifier = Modifier.weight(1f).padding(start = 8.dp),
        )
        BarIcon(Icons.Filled.ContentCopy, stringResource(R.string.chat_copy)) {
            val text = model.selectedText(myKey)
            if (text.isNotEmpty()) {
                val cm = ctx.getSystemService(android.content.Context.CLIPBOARD_SERVICE)
                    as android.content.ClipboardManager
                cm.setPrimaryClip(android.content.ClipData.newPlainText("p2p", text))
                android.widget.Toast
                    .makeText(ctx, R.string.chat_copied, android.widget.Toast.LENGTH_SHORT).show()
            }
            model.clearSelection()
        }
        BarIcon(Icons.Filled.Delete, stringResource(R.string.chat_delete)) { confirm = true }
    }

    if (confirm) {
        AlertDialog(
            onDismissRequest = { confirm = false },
            title = { Text(stringResource(R.string.chat_delete_title, sel.size)) },
            text = { Text(stringResource(R.string.saved_delete_note)) },
            confirmButton = {
                TextButton(onClick = {
                    model.deleteSelectedNotes()
                    confirm = false
                }) { Text(stringResource(R.string.folder_delete_ok), color = HoloError) }
            },
            dismissButton = {
                TextButton(onClick = { confirm = false }) {
                    Text(stringResource(R.string.common_cancel), color = HoloSecondary)
                }
            },
        )
    }
}
