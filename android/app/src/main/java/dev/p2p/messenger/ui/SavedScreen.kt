package dev.p2p.messenger.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Bookmark
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.res.stringResource
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
    var deleting by remember { mutableStateOf<Message?>(null) }
    val listState = rememberLazyListState()
    LaunchedEffect(Unit) { model.refreshNotes() }
    LaunchedEffect(model.notes.size) {
        if (model.notes.isNotEmpty()) listState.animateScrollToItem(model.notes.size - 1)
    }

    Column(Modifier.fillMaxSize()) {
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
                    Row(
                        Modifier.fillMaxWidth().padding(bottom = Tokens.Space.s),
                        horizontalArrangement = Arrangement.End,
                    ) {
                        Box(
                            Modifier.widthIn(max = 290.dp).clip(RoundedCornerShape(Tokens.Radius.preview))
                                .background(BubbleOut)
                                .pointerInput(n.id) { detectTapGestures(onLongPress = { deleting = n }) }
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

    deleting?.let { n ->
        AlertDialog(
            onDismissRequest = { deleting = null },
            title = { Text(stringResource(R.string.saved_delete)) },
            text = { Text(n.text, maxLines = 4) },
            confirmButton = {
                TextButton(onClick = {
                    model.deleteNote(n.id)
                    deleting = null
                }) { Text(stringResource(R.string.folder_delete_ok), color = HoloError) }
            },
            dismissButton = {
                TextButton(onClick = { deleting = null }) {
                    Text(stringResource(R.string.common_cancel), color = HoloSecondary)
                }
            },
        )
    }
}
