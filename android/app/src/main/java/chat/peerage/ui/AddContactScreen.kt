package chat.peerage.ui

import android.Manifest
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import chat.peerage.AppModel
import chat.peerage.CodeInfo
import chat.peerage.QrImage
import chat.peerage.R
import chat.peerage.Tokens
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * «Добавить контакт» (макеты `people/Main`, `Found`, `Errors`, шаг 7i): камера сразу, под ней
 * «Вставить код» и «Показать свой код»; ошибки кода — карточкой под камерой. Распознанный код —
 * имя, начало ключа и приветствие: оно и есть запрос в контакты.
 */
@Composable
internal fun AddContactScreen(model: AppModel, onMyCode: () -> Unit, onOpenChat: (String) -> Unit) {
    var code by remember { mutableStateOf("") }
    var info by remember { mutableStateOf<CodeInfo?>(null) }
    // Код, который уже разобран: камера видит его в каждом кадре, а разбирать его
    // заново незачем — и после «Сканировать другой» тот же код не должен вернуться.
    var seen by remember { mutableStateOf("") }
    // Откуда код: у вставленного текста «QR распознан» в ошибке было бы неправдой.
    var pasted by remember { mutableStateOf(false) }
    val take: (String, Boolean) -> Unit = { raw, fromPaste ->
        val c = raw.trim()
        if (c.isNotEmpty() && c != seen) {
            seen = c
            code = c
            pasted = fromPaste
            model.inspectCode(c) { info = it }
        }
    }
    // Код со снимка в галерее: прислали картинкой. Разбор — не на главном потоке.
    val ctx = LocalContext.current
    val scope = rememberCoroutineScope()
    val gallery = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri != null) {
            scope.launch {
                val text = withContext(Dispatchers.Default) { QrImage.fromUri(ctx, uri) }
                if (text != null) {
                    take(text, false)
                } else {
                    seen = ""
                    pasted = false
                    info = CodeInfo(NO_QR, "", "")
                }
            }
        }
    }
    val pickImage = { gallery.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) }
    val found = info?.takeIf { it.status == "ok" }
    if (found != null) {
        FoundCode(model, code, found, onSent = onOpenChat) {
            info = null
            code = ""
        }
    } else {
        ScanCode(model, info, pasted, take, pickImage, onMyCode, onOpenChat)
    }
}

/** Статус ошибки «на снимке из галереи нет QR» — своя, не из разбора кода движком. */
private const val NO_QR = "noqr"

@Composable
private fun ScanCode(
    model: AppModel,
    error: CodeInfo?,
    pasted: Boolean,
    take: (String, Boolean) -> Unit,
    onGallery: () -> Unit,
    onMyCode: () -> Unit,
    onOpenChat: (String) -> Unit,
) {
    val clipboard = LocalClipboardManager.current
    Column(Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(top = Tokens.Space.l)) {
        Viewfinder(short = error != null) { take(it, false) }
        Spacer(Modifier.height(Tokens.Space.l))
        if (error != null) {
            CodeError(model, error, pasted, onOpenChat)
            Spacer(Modifier.height(Tokens.Space.l))
        }
        CardSection {
            NavRow(
                TablerIcons.Clipboard,
                stringResource(R.string.add_paste),
                sub = stringResource(R.string.add_paste_sub),
                divider = false,
            ) { clipboard.getText()?.text?.let { take(it, true) } }
            NavRow(TablerIcons.Photo, stringResource(R.string.add_gallery), sub = stringResource(R.string.add_gallery_sub), onClick = onGallery)
            NavRow(TablerIcons.Qrcode, stringResource(R.string.add_show_code), onClick = onMyCode)
        }
        Caption(stringResource(R.string.add_caption))
    }
}

/** Видоискатель на панели: превью камеры и уголки рамки. Разрешение спрашивается при открытии. */
@Composable
private fun Viewfinder(short: Boolean, onCode: (String) -> Unit) {
    val ctx = LocalContext.current
    var granted by remember { mutableStateOf(ctx.hasPermission(Manifest.permission.CAMERA)) }
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted = it }
    LaunchedEffect(Unit) { if (!granted) launcher.launch(Manifest.permission.CAMERA) }
    Box(
        Modifier.padding(horizontal = Tokens.Space.panel).fillMaxWidth()
            .height(if (short) Tokens.Size.addCameraShort else Tokens.Size.addCamera)
            .clip(RoundedCornerShape(Tokens.Radius.panel))
            .background(Tokens.Colors.tile),
        contentAlignment = Alignment.Center,
    ) {
        if (granted) {
            CameraPreview(onCode, Modifier.fillMaxSize(), once = false)
            // Под камерой карточка ошибки — видоискатель ниже, рамка ближе к краю.
            Corners(if (short) Tokens.Size.cameraCornerInsetShort else Tokens.Size.cameraCornerInset)
            if (!short) {
                Text(
                    stringResource(R.string.add_camera_hint),
                    color = Color.White,
                    fontSize = Tokens.Font.preview,
                    modifier = Modifier.align(Alignment.BottomCenter).padding(bottom = Tokens.Space.l),
                )
            }
        } else {
            Text(
                stringResource(R.string.perm_denied_camera),
                color = Tokens.Colors.secondary,
                fontSize = Tokens.Font.body,
                textAlign = TextAlign.Center,
                modifier = Modifier.padding(Tokens.Space.xl),
            )
        }
    }
}

/** Четыре уголка рамки: куда навести код. */
@Composable
private fun Corners(inset: Dp) {
    Canvas(Modifier.fillMaxSize()) {
        val i = inset.toPx()
        val len = Tokens.Size.cameraCorner.toPx()
        val w = Tokens.Size.cameraCornerStroke.toPx()
        fun corner(x: Float, y: Float, dx: Float, dy: Float) {
            drawLine(Color.White, Offset(x, y), Offset(x + dx * len, y), w, cap = StrokeCap.Square)
            drawLine(Color.White, Offset(x, y), Offset(x, y + dy * len), w, cap = StrokeCap.Square)
        }
        corner(i, i, 1f, 1f)
        corner(size.width - i, i, -1f, 1f)
        corner(i, size.height - i, 1f, -1f)
        corner(size.width - i, size.height - i, -1f, -1f)
    }
}

/** Что не так с кодом (макет `people/Errors`): не код, код новой версии или другой сети,
 *  свой код, уже в контактах. */
@Composable
private fun CodeError(model: AppModel, info: CodeInfo, pasted: Boolean, onOpenChat: (String) -> Unit) {
    Row(
        Modifier.padding(horizontal = Tokens.Space.panel).fillMaxWidth()
            .clip(RoundedCornerShape(Tokens.Radius.group))
            .background(Tokens.Colors.group)
            .padding(start = Tokens.Space.l, end = Tokens.Space.m, top = Tokens.Space.m, bottom = Tokens.Space.m),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        if (info.status == "known") {
            val name = model.peerName(info.key)
            SquareAvatar(name, size = Tokens.Size.avatarPick, avatarBytes = model.peerAvatar(info.key), circle = true)
            Text(
                stringResource(R.string.add_err_known, name),
                color = Tokens.Colors.onDark,
                fontSize = Tokens.Font.label,
                fontWeight = FontWeight.Medium,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            PillButton(stringResource(R.string.add_open_chat)) { onOpenChat(info.key) }
            return@Row
        }
        Icon(TablerIcons.AlertTriangle, null, tint = Tokens.Colors.stateWarnText, modifier = Modifier.size(Tokens.Size.iconRow))
        Column(Modifier.weight(1f)) {
            Text(
                when (info.status) {
                    NO_QR -> stringResource(R.string.add_err_noqr)
                    "self" -> stringResource(R.string.add_err_self)
                    "newer" -> stringResource(R.string.add_err_newer, stringResource(R.string.app_name))
                    "foreign" -> stringResource(R.string.add_err_foreign)
                    else -> stringResource(R.string.add_err_invalid, stringResource(R.string.app_name))
                },
                color = Tokens.Colors.onDark,
                fontSize = Tokens.Font.label,
                fontWeight = FontWeight.Medium,
            )
            Text(
                stringResource(
                    when {
                        info.status == NO_QR -> R.string.add_err_noqr_sub
                        info.status == "self" -> R.string.add_err_self_sub
                        info.status == "newer" -> R.string.add_err_newer_sub
                        info.status == "foreign" -> R.string.add_err_foreign_sub
                        pasted -> R.string.add_err_invalid_paste_sub
                        else -> R.string.add_err_invalid_sub
                    },
                ),
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.body,
                lineHeight = Tokens.Font.lineHeight,
                modifier = Modifier.padding(top = Tokens.Space.xxs),
            )
        }
    }
}

/**
 * Код распознан (макет `people/Found`): кто это, приветствие — оно и есть запрос (уйдёт с
 * именем этого профиля, фото — после принятия), как подписать у себя. «Отправить запрос»
 * открывает чат: там приветствие и всё дальше ждут принятия.
 */
@Composable
private fun FoundCode(model: AppModel, code: String, info: CodeInfo, onSent: (String) -> Unit, onRescan: () -> Unit) {
    val shown = info.name.ifBlank { info.key.take(8) }
    var greeting by remember(code) { mutableStateOf("") }
    var localName by remember(code) { mutableStateOf("") }
    var sending by remember(code) { mutableStateOf(false) }
    Column(Modifier.fillMaxSize()) {
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(top = Tokens.Space.l)) {
            Row(
                Modifier.padding(start = Tokens.Space.panel + Tokens.Space.xs, end = Tokens.Space.panel, bottom = Tokens.Space.s),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m + Tokens.Space.xxs),
            ) {
                SquareAvatar(shown, size = Tokens.Size.avatarFound, circle = true)
                Column {
                    Text(shown, color = Tokens.Colors.onDark, fontSize = Tokens.Font.title, fontWeight = FontWeight.Bold, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Text(
                        stringResource(R.string.add_key_start, info.key.take(8)),
                        color = Tokens.Colors.tertiary,
                        fontSize = Tokens.Font.body,
                        style = Tokens.Text.tabular,
                    )
                }
            }
            SectionTitle(stringResource(R.string.add_greeting))
            Box(
                Modifier.padding(horizontal = Tokens.Space.panel).fillMaxWidth()
                    .heightIn(min = Tokens.Size.greetingField)
                    .clip(RoundedCornerShape(Tokens.Radius.field))
                    .background(Tokens.Colors.surface)
                    .padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.m + Tokens.Space.xxs),
            ) {
                if (greeting.isEmpty()) Text(stringResource(R.string.add_greeting_hint), color = Tokens.Colors.tertiary, fontSize = Tokens.Font.name)
                BasicTextField(
                    greeting,
                    { if (it.length <= GreetingMax) greeting = it },
                    textStyle = TextStyle(color = Color.White, fontSize = Tokens.Font.name, lineHeight = Tokens.Font.lineHeightLoose),
                    cursorBrush = SolidColor(Tokens.Colors.accent),
                    keyboardOptions = TextKeyboard,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            Caption(stringResource(R.string.add_greeting_caption))
            SectionTitle(stringResource(R.string.add_local_name))
            FormInput(localName, shown, Modifier.padding(horizontal = Tokens.Space.panel).fillMaxWidth()) { localName = it }
            Caption(stringResource(R.string.add_local_name_caption))
        }
        Column(
            Modifier.fillMaxWidth().navigationBarsPadding()
                .padding(start = Tokens.Space.xl, end = Tokens.Space.xl, top = Tokens.Space.m, bottom = Tokens.Space.xl),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.xs),
        ) {
            AccentButton(stringResource(R.string.add_send), Modifier.fillMaxWidth().heightIn(min = Tokens.Size.primaryButton), enabled = !sending) {
                sending = true
                model.requestContact(code, localName, greeting) { r ->
                    sending = false
                    if (r == "sent") onSent(info.key)
                }
            }
            GhostButton(stringResource(R.string.add_rescan), modifier = Modifier.fillMaxWidth(), onClick = onRescan)
        }
    }
}

/** Предел приветствия в символах: движок режет по байтам (`MAX_GREETING`, ~500 кириллицей). */
private const val GreetingMax = 500
