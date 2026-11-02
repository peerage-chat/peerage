@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package chat.peerage.ui

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.waitForUpOrCancellation
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyListState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.*
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.layout.SubcomposeLayout
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.TextLayoutResult
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import chat.peerage.*
import chat.peerage.HoloSurface
import chat.peerage.R
import kotlin.math.abs

// ── Общие элементы ──────────────────────────────────────────────────────────

/**
 * Иконка-кнопка топ-бара: плашка цвета панели на фоне шапки, 38dp с иконкой 20dp.
 * Фон обязателен — без него иконка на фоне экрана не отличается от подписи и не
 * читается как нажимаемая (правило «контролы отличаются от контента»).
 * Нажимается вся зона 48dp вокруг плашки: на Android меньше — промахи пальцем.
 */
@Composable
internal fun BarIcon(icon: ImageVector, desc: String, tint: Color = Tokens.Colors.iconBar, onClick: () -> Unit) {
    val press = remember { MutableInteractionSource() }
    Box(
        Modifier.size(Tokens.Size.touch)
            .pressScale(press)
            .clip(RoundedCornerShape(Tokens.Radius.button))
            .clickable(interactionSource = press, indication = null, role = Role.Button, onClickLabel = desc) { onClick() },
        contentAlignment = Alignment.Center,
    ) {
        Box(
            Modifier.size(Tokens.Size.barButton)
                .clip(RoundedCornerShape(Tokens.Radius.button))
                .background(Tokens.Colors.panel),
            contentAlignment = Alignment.Center,
        ) {
            Icon(icon, contentDescription = desc, tint = tint, modifier = Modifier.size(Tokens.Size.iconRow))
        }
    }
}

@Composable
internal fun TopBar(
    title: String,
    onBack: (() -> Unit)? = null,
    onMenu: (() -> Unit)? = null,
    /** Своё содержимое слева вместо «назад»/«меню» — например, аватар-кнопка «Я». */
    leading: (@Composable () -> Unit)? = null,
    actions: @Composable RowScope.() -> Unit = {},
) {
    // Шапка на фоне экрана, а не на акцентной плашке: цветная полоса сверху делила
    // экран пополам и спорила с карточками за внимание.
    Row(
        Modifier.fillMaxWidth().background(HoloBg).height(Tokens.Size.topBar).padding(horizontal = Tokens.Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        when {
            leading != null -> leading()
            onMenu != null -> BarIcon(TablerIcons.Menu2, stringResource(R.string.common_menu), onClick = onMenu)
            onBack != null -> BarIcon(Icons.AutoMirrored.Filled.ArrowBack, stringResource(R.string.common_back), onClick = onBack)
            else -> Spacer(Modifier.width(Tokens.Space.m))
        }
        Text(
            title,
            color = Color.White,
            fontSize = Tokens.Font.title,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.weight(1f),
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
        actions()
    }
}

/**
 * Бейдж-счётчик: непрочитанное в строке чата, на вкладке, значок «@». Один на все
 * места — раньше их было три копии с разными отступами и скруглением, и цифра в
 * каждой сидела по-своему.
 *
 * Высота фиксирована, ширина не меньше высоты: одна цифра — кружок, «99+» — капсула.
 * Текст центрируется по высоте самой строкой (`Tokens.Text.base`), а не отступами.
 */
@Composable
internal fun CountBadge(text: String, muted: Boolean = false, compact: Boolean = false) {
    // compact — счётчик у подписи вкладки: мельче строчного, чтобы не перерастать текст.
    val size = if (compact) Tokens.Size.badgeTab else Tokens.Size.badge
    Box(
        Modifier.height(size).widthIn(min = size)
            .clip(RoundedCornerShape(percent = 50))
            .background(if (muted) Tokens.Colors.unreadMuted else HoloAccent)
            .padding(horizontal = Tokens.Space.xs + Tokens.Space.xxs),
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text,
            style = Tokens.Text.meta,
            color = if (muted) Tokens.Colors.unreadMutedText else HoloAccentDark,
            fontSize = if (compact) Tokens.Font.caption else Tokens.Font.small,
            fontWeight = FontWeight.Bold,
            maxLines = 1,
        )
    }
}

/** Тег-пометка у строки: «скоро», «рекомендуется», «по умолчанию». Тише счётчика —
 *  он сообщает, а не зовёт; три копии с разными полями сведены сюда. */
@Composable
internal fun TagBadge(text: String) {
    Box(
        Modifier.clip(RoundedCornerShape(Tokens.Radius.badge))
            .background(Tokens.Colors.accentBadge)
            .padding(horizontal = Tokens.Space.s, vertical = Tokens.Space.xxs),
    ) {
        Text(text, color = HoloAccent, fontSize = Tokens.Font.caption, maxLines = 1)
    }
}

/** Битрейт словами: «1,5» или «2» — без хвоста «,0» у целых. Число в локали
 *  устройства: по-русски запятая. */
internal fun fmtMbps(bps: Int): String {
    val v = bps / 1_000_000.0
    return if (v % 1.0 == 0.0) "%d".format(v.toInt()) else "%.1f".format(v)
}

/** Число для бейджа: больше 99 — «99+», точное число в узком бейдже не читается. */
internal fun badgeCount(n: Int): String = if (n > 99) "99+" else "$n"

/** Индикатор присутствия: маленький кружок (онлайн/офлайн). */
@Composable
internal fun StatusDot(online: Boolean) {
    Box(Modifier.size(10.dp).clip(CircleShape).background(if (online) HoloOnline else OfflineDot))
}

/**
 * Аватар: буква имени на детерминированном цвете, картинка профиля или своя
 * иконка ([content]); опционально presence-точка в правом-нижнем углу.
 *
 * Форма несёт смысл: [circle] — человек, скруглённый квадрат — сущность (группа,
 * «Сохранённое», канал). Различие формы читается боковым зрением и не требует
 * подписи. [ring] — цвет обводки presence-точки: она должна совпадать с фоном ПОД
 * аватаром, иначе точка выглядит наклеенной.
 */
@Composable
internal fun SquareAvatar(
    name: String,
    size: Dp = 44.dp,
    online: Boolean? = null,
    bg: Color? = null,
    avatarBytes: ByteArray? = null,
    circle: Boolean = false,
    ring: Color = Tokens.Colors.surface,
    content: (@Composable () -> Unit)? = null,
) {
    // Аватар декодируется под свою сторону: он бывает и 38dp в шапке, и 96dp в
    // профиле, а исходник — полноразмерное фото.
    val bmp = rememberBitmap(avatarBytes, null, size)
    // «квадрат со скруглением» (app-ui.md ~14-15px): пропорция size×0.3, но с кэпом
    // на Tokens.Radius.avatar — иначе крупные аватарки «балонят» до почти-круга.
    val shape = if (circle) {
        CircleShape
    } else {
        RoundedCornerShape(minOf(size * 0.3f, Tokens.Radius.avatar))
    }
    Box(Modifier.size(size), contentAlignment = Alignment.BottomEnd) {
        Box(
            Modifier.size(size).clip(shape).background(bg ?: avatarColor(name))
                .then(if (bmp != null) Modifier.imageOutline(shape) else Modifier),
            contentAlignment = Alignment.Center,
        ) {
            when {
                bmp != null -> Image(bmp, null, Modifier.fillMaxSize(), contentScale = ContentScale.Crop)
                content != null -> content()
                else -> Text(
                    initialOf(name),
                    color = Color.White,
                    fontSize = Tokens.Font.initial(size),
                    fontWeight = FontWeight.Medium,
                )
            }
        }
        if (online != null) {
            Box(
                Modifier.size(size * 0.30f).clip(CircleShape)
                    .background(if (online) HoloOnline else OfflineDot)
                    .border(2.5.dp, ring, CircleShape),
            )
        }
    }
}

/** Буква-заглушка аватара: первая ГРАФЕМА, а не первый char. Имя, начатое с эмодзи,
 *  давало половину суррогатной пары — квадратик с крестом вместо буквы. */
internal fun initialOf(name: String): String {
    val t = name.trim()
    if (t.isEmpty()) return ""
    val it = java.text.BreakIterator.getCharacterInstance()
    it.setText(t)
    val end = it.next().takeIf { e -> e != java.text.BreakIterator.DONE } ?: t.length
    return t.substring(0, end).uppercase()
}

@Composable
internal fun ListRow(onClick: () -> Unit, content: @Composable RowScope.() -> Unit) {
    // Без разделителей — мягкий «телеграмный» список (по макету).
    Row(
        Modifier.fillMaxWidth().clickable { onClick() }.padding(horizontal = ScreenPad, vertical = RowPadV),
        verticalAlignment = Alignment.CenterVertically,
        content = content,
    )
}

@Composable
internal fun FieldLabel(text: String) {
    Text(text, color = HoloSecondary, fontSize = Tokens.Font.caption, fontWeight = FontWeight.Medium)
}

// ── Карточки-секции ─────────────────────────────────────────────────────────
// Единый контейнер для всех списочных экранов: строки живут ВНУТРИ карточки,
// заголовок секции — СНАРУЖИ, над ней. Так группа читается как один предмет, а
// заголовок не оказывается «первой строкой списка», к которой хочется прикоснуться.

/** Подпись группы — над ней, вне подложки, по одной вертикали с текстом в группе. */
@Composable
internal fun SectionTitle(text: String, modifier: Modifier = Modifier) {
    Text(
        text,
        color = Tokens.Colors.tertiary,
        fontSize = Tokens.Font.body,
        fontWeight = FontWeight.Medium,
        modifier = modifier.padding(
            start = Tokens.Space.panelText,
            end = Tokens.Space.panelText,
            top = Tokens.Space.l,
            bottom = Tokens.Space.s,
        ),
    )
}

/**
 * Группа строк на панели «листа»: ступень `group`, скругление 18 при отступе панели 10 —
 * концентрично её 28. Отступ один на все экраны: группы стоят по одной вертикали, иначе
 * переход между экранами читается как сдвиг всего макета.
 */
@Composable
internal fun CardSection(modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    Column(
        modifier.fillMaxWidth().padding(horizontal = Tokens.Space.panel)
            .clip(RoundedCornerShape(Tokens.Radius.card))
            .background(Tokens.Colors.surface),
        content = content,
    )
}

/**
 * Кусок карточки под ОДНУ строку ленивого списка: скругление появляется только у
 * первой и последней. Нужен там, где строк много и держать их в одном `Column`
 * нельзя — `LazyColumn` иначе потеряет переработку элементов.
 */
@Composable
internal fun CardBlock(
    first: Boolean,
    last: Boolean,
    bg: Color = Tokens.Colors.surface,
    content: @Composable ColumnScope.() -> Unit,
) {
    val r = Tokens.Radius.card
    Column(
        Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.panel)
            .clip(
                RoundedCornerShape(
                    topStart = if (first) r else 0.dp,
                    topEnd = if (first) r else 0.dp,
                    bottomStart = if (last) r else 0.dp,
                    bottomEnd = if (last) r else 0.dp,
                ),
            )
            .background(bg),
        content = content,
    )
}

/**
 * Строка внутри [CardSection]. [divider] — рисовать ли разделитель СВЕРХУ (у первой
 * строки не нужен); [inset] — отступ разделителя слева, чтобы он начинался за
 * иконкой и не рассекал карточку насквозь.
 */
@Composable
internal fun CardRow(
    onClick: (() -> Unit)? = null,
    divider: Boolean = true,
    inset: Dp = Tokens.Size.cardRow,
    modifier: Modifier = Modifier,
    content: @Composable RowScope.() -> Unit,
) {
    if (divider) {
        HorizontalDivider(
            thickness = 1.dp,
            color = Tokens.Colors.divider,
            modifier = Modifier.padding(start = inset),
        )
    }
    Row(
        modifier.fillMaxWidth()
            .then(if (onClick != null) Modifier.clickable { onClick() } else Modifier)
            .heightIn(min = Tokens.Size.cardRow)
            .padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.s),
        verticalAlignment = Alignment.CenterVertically,
        content = content,
    )
}

/**
 * Строка-раздел в группе: плитка с иконкой, название, справа значение («12», «1,2 ГБ»). Шеврона
 * нет — что строка ведёт дальше, говорит сама группа разделов. Разделитель — от названия.
 */
@Composable
internal fun NavRow(
    icon: ImageVector,
    label: String,
    value: String = "",
    divider: Boolean = true,
    onClick: () -> Unit,
) {
    if (divider) {
        HorizontalDivider(thickness = 1.dp, color = Tokens.Colors.divider, modifier = Modifier.padding(start = NavRowInset))
    }
    Row(
        Modifier.fillMaxWidth()
            .clickable(role = Role.Button) { onClick() }
            .heightIn(min = Tokens.Size.navRow)
            .padding(start = Tokens.Space.l, end = Tokens.Space.l, top = Tokens.Space.s, bottom = Tokens.Space.s),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        Box(
            Modifier.size(Tokens.Size.rowIcon).clip(RoundedCornerShape(Tokens.Radius.inner)).background(Tokens.Colors.raised),
            contentAlignment = Alignment.Center,
        ) {
            Icon(icon, null, tint = Tokens.Colors.iconBar, modifier = Modifier.size(Tokens.Size.iconRow))
        }
        Text(label, color = Color.White, fontSize = Tokens.Font.name, modifier = Modifier.weight(1f))
        if (value.isNotEmpty()) Text(value, color = Tokens.Colors.tertiary, fontSize = Tokens.Font.preview, maxLines = 1)
    }
}

private val NavRowInset = Tokens.Space.l + Tokens.Size.rowIcon + Tokens.Space.m

/**
 * Строка с переключателем нажимается ЦЕЛИКОМ и озвучивается как переключатель с текстом строки
 * в имени. Сам `Switch` в ней — только картинка (`onCheckedChange = null`): иначе у строки две
 * цели нажатия, а TalkBack читает «Переключатель, вкл» без названия (проверка 2026-11-01).
 */
internal fun Modifier.switchRow(checked: Boolean, enabled: Boolean = true, onChange: (Boolean) -> Unit): Modifier =
    toggleable(value = checked, enabled = enabled, role = Role.Switch, onValueChange = onChange)

/**
 * Пилюля выбора варианта (разрешение, fps, устройство звука, «авто»). Одна на все
 * экраны: в звонке и в настройках варианты выглядят одинаково — так и задумано
 * (call-ui.md, «Настройки по умолчанию»: те же элементы, что во время звонка).
 * [compact] — мелкий вариант для переключателя «авто» рядом с заголовком.
 */
@Composable
internal fun OptionChip(
    text: String,
    selected: Boolean,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    compact: Boolean = false,
    onClick: () -> Unit,
) {
    val shape = RoundedCornerShape(Tokens.Radius.chip)
    val fg = if (selected) Tokens.Colors.accent else Color.White
    Row(
        modifier.clip(shape)
            .background(if (selected) Tokens.Colors.accentMuted else Tokens.Colors.tile)
            .then(if (selected) Modifier.border(1.5.dp, Tokens.Colors.accent, shape) else Modifier)
            .clickable { onClick() }
            .padding(
                horizontal = if (compact) Tokens.Space.m else Tokens.Space.l,
                vertical = if (compact) Tokens.Space.xs else Tokens.Space.m,
            ),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s, Alignment.CenterHorizontally),
    ) {
        if (icon != null) {
            Icon(
                icon,
                null,
                tint = if (selected) Tokens.Colors.accent else Tokens.Colors.secondary,
                modifier = Modifier.size(if (compact) 13.dp else 16.dp),
            )
        }
        Text(text, color = fg, fontSize = if (compact) Tokens.Font.caption else Tokens.Font.body, maxLines = 1)
    }
}

@Composable
internal fun AccentButton(text: String, modifier: Modifier = Modifier, enabled: Boolean = true, onClick: () -> Unit) {
    val press = remember { MutableInteractionSource() }
    Button(
        onClick = onClick,
        enabled = enabled,
        interactionSource = press,
        // Высота как у поля ввода: на material-умолчании (~33dp) радиус 14 съедает
        // почти всю сторону, и кнопка читается капсулой, а не нашей плашкой.
        modifier = modifier.pressScale(press).heightIn(min = Tokens.Size.inputField),
        shape = RoundedCornerShape(Tokens.Radius.button),
        // Неактивная — плашкой с приглушённым текстом: тёмный текст акцента на material-сером
        // не читается вовсе.
        colors = ButtonDefaults.buttonColors(containerColor = HoloAccent, disabledContainerColor = Tokens.Colors.raised),
    ) {
        Text(text, color = if (enabled) HoloAccentDark else Tokens.Colors.tertiary, fontWeight = FontWeight.Medium)
    }
}

/**
 * Подтверждение — шторка снизу (app-ui.md → «Общие элементы»): вопрос, одна строка
 * последствия, красная кнопка действия и «Отмена». Для того, что уходит в сеть или не
 * возвращается; обратимое «у себя» — снекбаром «Отменить», а не шторкой.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun ConfirmSheet(title: String, text: String, ok: String, onOk: () -> Unit, onClose: () -> Unit) {
    ActionSheet(title, text, onClose) {
        PlateButton(ok, modifier = Modifier.fillMaxWidth(), danger = true) {
            onOk()
            onClose()
        }
        PlateButton(stringResource(R.string.common_cancel), modifier = Modifier.fillMaxWidth(), onClick = onClose)
    }
}

/** Шторка-решение: заголовок, пояснение и кнопки [buttons] столбиком (опасная — первой). */
@Composable
internal fun ActionSheet(title: String, text: String, onClose: () -> Unit, buttons: @Composable ColumnScope.() -> Unit) {
    ModalBottomSheet(
        onDismissRequest = onClose,
        sheetState = rememberModalBottomSheetState(skipPartiallyExpanded = true),
        containerColor = Tokens.Colors.group,
        // Без тональной подсветки Material: она подмешивает акцент, и ступень group синела.
        tonalElevation = 0.dp,
        shape = RoundedCornerShape(topStart = Tokens.Radius.panel, topEnd = Tokens.Radius.panel),
        dragHandle = { SheetHandle() },
    ) {
        Column(
            Modifier.fillMaxWidth().navigationBarsPadding()
                .padding(start = Tokens.Space.l, end = Tokens.Space.l, bottom = Tokens.Space.xl),
            verticalArrangement = Arrangement.spacedBy(Tokens.Space.m),
        ) {
            Text(
                title,
                color = Color.White,
                fontSize = Tokens.Font.sheet,
                fontWeight = FontWeight.Bold,
                modifier = Modifier.padding(horizontal = Tokens.Space.xs).semantics { heading() },
            )
            Text(
                text,
                color = Tokens.Colors.tertiary,
                fontSize = Tokens.Font.label,
                modifier = Modifier.padding(horizontal = Tokens.Space.xs),
            )
            buttons()
        }
    }
}

/** Ручка шторки: показывает, что её можно утянуть вниз. */
@Composable
internal fun SheetHandle() {
    Box(
        Modifier.padding(top = Tokens.Space.s, bottom = Tokens.Space.xs)
            .size(width = Tokens.Size.handleW, height = Tokens.Size.handleH)
            .clip(RoundedCornerShape(percent = 50))
            .background(Tokens.Colors.handle),
    )
}

/** Кнопка без подложки: второстепенное действие под главной («Отклонить», «Заблокировать»). */
@Composable
internal fun GhostButton(text: String, color: Color = Tokens.Colors.accent, modifier: Modifier = Modifier, onClick: () -> Unit) {
    val press = remember { MutableInteractionSource() }
    Box(
        modifier.heightIn(min = Tokens.Size.touch)
            .pressScale(press)
            .clip(RoundedCornerShape(Tokens.Radius.button))
            .clickable(interactionSource = press, indication = null, role = Role.Button) { onClick() }
            .padding(horizontal = Tokens.Space.l),
        contentAlignment = Alignment.Center,
    ) {
        Text(text, color = color, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium)
    }
}

/**
 * Кнопка-плашка на панели: ступень над группой, иконка + подпись, 48 dp. Нажатие сжимает её
 * до 0.96 — отклик пальцу (app-ui.md → «Отделка»); у строк вместо этого подсветка.
 */
@Composable
internal fun PlateButton(
    text: String,
    icon: ImageVector? = null,
    modifier: Modifier = Modifier,
    danger: Boolean = false,
    onClick: () -> Unit,
) {
    val fg = if (danger) Tokens.Colors.iconDangerMuted else Color.White
    val press = remember { MutableInteractionSource() }
    Row(
        modifier.pressScale(press)
            .heightIn(min = Tokens.Size.touch)
            .clip(RoundedCornerShape(Tokens.Radius.button))
            .background(if (danger) Tokens.Colors.dangerPlate else Tokens.Colors.raised)
            .clickable(interactionSource = press, indication = null, role = Role.Button) { onClick() }
            .padding(horizontal = Tokens.Space.l, vertical = Tokens.Space.s),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s, Alignment.CenterHorizontally),
    ) {
        if (icon != null) Icon(icon, null, tint = fg, modifier = Modifier.size(Tokens.Size.iconRow))
        Text(text, color = fg, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium, maxLines = 1)
    }
}

/** Сжатие кнопки под пальцем: заметно, но не «проваливается» (меньше 0.95 — уже утрировано). */
internal const val PressScale = 0.96f

/**
 * Нажатие — масштаб 0.96 (app-ui.md → «Отделка»): кнопки, плитки, пилюли. Не строки (там
 * подсветка) и не пузыри. [source] — тот же, что у `clickable`, иначе нажатия не видно.
 */
@Composable
internal fun Modifier.pressScale(source: MutableInteractionSource): Modifier {
    val pressed by source.collectIsPressedAsState()
    val scale by animateFloatAsState(if (pressed) PressScale else 1f, tween(Tokens.Anim.fast), label = "press")
    return graphicsLayer {
        scaleX = scale
        scaleY = scale
    }
}

/** Обводка картинки — 1 px белым 10 % по её форме: фото не сливается с тёмным фоном. */
internal fun Modifier.imageOutline(shape: androidx.compose.ui.graphics.Shape): Modifier =
    border(1.dp, Tokens.Colors.imageEdge, shape)

/**
 * Второстепенная кнопка: та же форма и высота, что у [AccentButton], но контуром.
 * Material по умолчанию рисует кнопки пилюлей; рядом с нашими плашками радиуса 14
 * это выглядело как элемент из другого набора.
 */
@Composable
internal fun OutlineButton(
    text: String,
    modifier: Modifier = Modifier,
    tint: Color = HoloAccent,
    onClick: () -> Unit,
) {
    OutlinedButton(
        onClick = onClick,
        modifier = modifier.heightIn(min = Tokens.Size.inputField),
        shape = RoundedCornerShape(Tokens.Radius.button),
        border = BorderStroke(1.dp, Tokens.Colors.border),
    ) {
        Text(text, color = tint, fontWeight = FontWeight.Medium)
    }
}

/**
 * Квадратик миниатюры в плашке цитаты. Возвращает `false`, если показывать нечего —
 * вызывающий тогда оставляет текстовую подпись («Фото», имя файла).
 *
 * Имя файла в цитате человеку ничего не говорит: на «v11.png» невозможно узнать
 * снимок, на который отвечают. Миниатюра узнаётся мгновенно.
 */
@Composable
internal fun RowScope.QuoteThumb(model: AppModel, blobId: String, thumbId: String, kind: String): Boolean {
    // Сначала миниатюра; сам блоб — только у фото: у видео и файла это весь файл
    // в память ради квадратика 34dp.
    val bytes = model.blob(thumbId) ?: (if (kind == "image") model.blob(blobId) else null) ?: return false
    val bmp = rememberBitmap(bytes, thumbId.ifEmpty { blobId }, 34.dp) ?: return false
    Image(
        bmp,
        null,
        Modifier.align(Alignment.CenterVertically)
            .padding(start = Tokens.Space.s, end = Tokens.Space.s)
            .size(34.dp)
            .clip(RoundedCornerShape(Tokens.Radius.badge)),
        contentScale = ContentScale.Crop,
    )
    return true
}

@Composable
internal fun SendButton(onClick: () -> Unit) {
    Box(
        // Та же форма и размер, что у кнопки отправки в чате: это одна и та же
        // кнопка по смыслу, и различаться ей незачем.
        Modifier.clip(RoundedCornerShape(Tokens.Radius.button)).background(HoloAccent)
            .size(Tokens.Size.inputField).clickable { onClick() },
        contentAlignment = Alignment.Center,
    ) { Icon(Icons.AutoMirrored.Filled.Send, stringResource(R.string.common_send), tint = HoloAccentDark, modifier = Modifier.size(Tokens.Size.iconM)) }
}

// ── Экраны ──────────────────────────────────────────────────────────────────

/** Версия приложения — из пакета, то есть из `versionName` в Gradle. Строкой в
 *  ресурсах она жила отдельно, в двух языках, и уже разошлась (в меню стояло «v0.3»). */
@Composable
internal fun appVersion(): String {
    val ctx = LocalContext.current
    return remember {
        runCatching { ctx.packageManager.getPackageInfo(ctx.packageName, 0).versionName }.getOrNull().orEmpty()
    }
}

/** Время сообщения «ЧЧ:ММ» (time — unix-секунды). */
internal fun fmtMsgTime(ts: Long): String =
    java.text.SimpleDateFormat("HH:mm", java.util.Locale.getDefault()).format(java.util.Date(ts * 1000))

/**
 * Время в строке списка чатов: сегодня — «ЧЧ:ММ», вчера — «вчера», на этой неделе —
 * короткий день, дальше — «ДД.ММ». Колонка узкая, поэтому чем старше сообщение, тем
 * грубее единица: точное время недельной давности всё равно ничего не сообщает.
 */
@Composable
internal fun fmtListTime(ts: Long): String {
    if (ts <= 0) return ""
    val nowSec = System.currentTimeMillis() / 1000
    val loc = java.util.Locale.getDefault()
    val date = java.util.Date(ts * 1000)
    return when {
        dayKey(ts) == dayKey(nowSec) -> fmtMsgTime(ts)
        dayKey(ts) == dayKey(nowSec - 86_400) -> stringResource(R.string.chats_time_yesterday)
        nowSec - ts < 6 * 86_400 -> java.text.SimpleDateFormat("EEE", loc).format(date)
        // Прошлый год — с годом: «05.03» без него выглядит как этой весной.
        yearOf(ts) == yearOf(nowSec) -> java.text.SimpleDateFormat("dd.MM", loc).format(date)
        else -> java.text.SimpleDateFormat("dd.MM.yy", loc).format(date)
    }
}

/** Ключ дня для сравнения «сменился ли день» между соседними сообщениями
 *  (не-Composable: используется в условии группировки). */
internal fun dayKey(ts: Long): Long {
    val c = java.util.Calendar.getInstance().apply { timeInMillis = ts * 1000 }
    return c.get(java.util.Calendar.YEAR) * 1000L + c.get(java.util.Calendar.DAY_OF_YEAR)
}

internal fun yearOf(ts: Long): Long = dayKey(ts) / 1000

// ── Автор сообщения ─────────────────────────────────────────────────────────
// Живёт здесь, а не в экране группы: имя и цвет автора нужны и ленте, и цитате,
// и полоске ответа — то есть общему коду чата, а не одному экрану.

/** Имя отправителя группового сообщения по hex-ключу. */
@Composable
internal fun groupSenderName(model: AppModel, hex: String): String = when {
    hex.isEmpty() -> stringResource(R.string.group_member_fallback)
    hex == model.me?.pubkey -> stringResource(R.string.common_you)
    else -> model.peerName(hex)
}

/** Яркий детерминированный цвет имени отправителя (Discord-style). */
internal val NameColors = Tokens.Colors.senderNames
internal fun nameColor(hex: String): Color =
    if (hex.isEmpty()) HoloSecondary else NameColors[abs(hex.hashCode()) % NameColors.size]

// ── Склейка сообщений в блоки (docs/design/mockups/chat-blocks.html) ────────
// Блок — подряд идущие сообщения ОДНОГО отправителя внутри окна времени, без
// разделителей между ними. Внутри блока сообщения стоят вплотную и «сшиваются»
// уменьшенным радиусом со стороны отправителя.

/** Окно склейки, секунды. Часы устройств не синхронизированы (время ставит
 *  отправитель), поэтому окно нарочно широкое: узкое рвало бы блок на расхождении. */
internal const val BLOCK_WINDOW_SEC = 300L

/** Положение сообщения в блоке. */
internal enum class BlockPos { Solo, First, Mid, Last }

/** Продолжает ли сообщение блок предыдущего: тот же отправитель, тот же день,
 *  разрыв меньше окна. Системные строки блок разрывают (передавайте `null`). */
internal fun continuesBlock(prevSender: String?, prevTime: Long, sender: String, time: Long): Boolean =
    prevSender == sender && time - prevTime < BLOCK_WINDOW_SEC && dayKey(prevTime) == dayKey(time)

internal fun blockPos(withPrev: Boolean, withNext: Boolean): BlockPos = when {
    !withPrev && !withNext -> BlockPos.Solo
    !withPrev -> BlockPos.First
    withNext -> BlockPos.Mid
    else -> BlockPos.Last
}

/** Отступ ПОД сообщением: внутри блока — хайрлайн, между блоками — обычный шаг. */
internal fun blockGap(pos: BlockPos) =
    if (pos == BlockPos.First || pos == BlockPos.Mid) Tokens.Space.xxs else Tokens.Space.s

/**
 * Текст сообщения с метой (время, «изм.», галочка) в правом нижнем углу — так,
 * что текст ОБТЕКАЕТ мету: короткая последняя строка пускает мету к себе, длинная
 * переносит её на новую строку.
 *
 * Как это устроено: мета измеряется, и в конец текста подставляется невидимая
 * вставка ровно её размера — перенос считает уже сам движок вёрстки. Раньше место
 * резервировалось «на глаз» (`bubbleTimeReserve`), и каждый новый значок в мете
 * (сначала галочка, потом «изм.») этот расчёт ломал: то дыра, то наезд на текст.
 *
 * ВАЖНО: сюда передаётся та же мета, что рисует пузырь в своём углу ([MsgMeta]) —
 * здесь она нужна только как мерка. Расходиться они не должны.
 */
@Composable
internal fun BubbleText(
    text: String,
    modifier: Modifier = Modifier,
    color: Color = Color.White,
    fontSize: TextUnit = Tokens.Font.label,
    meta: @Composable () -> Unit,
) = BubbleText(AnnotatedString(text), modifier, color, fontSize, meta)

/** Что делать по тапу на «@имя» в пузыре: получает ключ упомянутого (hex). Задаёт
 *  экран ленты — пузырю незачем знать, куда ведёт навигация. */
internal val LocalMentionClick = staticCompositionLocalOf<((String) -> Unit)?> { null }

/** То же для размеченного текста (упоминания): мерка и показ идут по ОДНОЙ строке —
 *  имя в «@» бывает длиннее или короче набранного отправителем. */
@Composable
internal fun BubbleText(
    text: AnnotatedString,
    modifier: Modifier = Modifier,
    color: Color = Color.White,
    fontSize: TextUnit = Tokens.Font.label,
    meta: @Composable () -> Unit,
) {
    val measurer = rememberTextMeasurer()
    val style = Tokens.Text.base.copy(color = color, fontSize = fontSize)
    val onMention = LocalMentionClick.current
    val tappable = onMention != null && text.getStringAnnotations(Mentions.TAG, 0, text.length).isNotEmpty()
    var layout by remember { mutableStateOf<TextLayoutResult?>(null) }
    // Тап по «@» ловим сами, а не clickable: остальной текст должен отдавать жесты
    // пузырю (долгий тап — выделение), поэтому событие съедаем, только если короткий
    // тап пришёлся точно на имя.
    val mentionTap = if (!tappable) {
        Modifier
    } else {
        Modifier.pointerInput(text) {
            awaitEachGesture {
                val down = awaitFirstDown(requireUnconsumed = false)
                val key = mentionAt(layout, text, down.position) ?: return@awaitEachGesture
                val up = waitForUpOrCancellation() ?: return@awaitEachGesture
                if (up.uptimeMillis - down.uptimeMillis < viewConfiguration.longPressTimeoutMillis) {
                    up.consume()
                    onMention?.invoke(key)
                }
            }
        }
    }
    SubcomposeLayout(modifier) { constraints ->
        // Мету здесь только ИЗМЕРЯЕМ, чтобы оставить ей место. Рисует её сам пузырь,
        // в своём правом нижнем углу: у пузыря шире текста (метка «Переслано», имя
        // автора) прижатая к концу строки мета повисла бы посередине, а растянуться
        // до ширины соседей этот layout не может.
        val metaP = subcompose("meta", meta).map { it.measure(Constraints()) }
        val metaW = metaP.maxOfOrNull { it.width } ?: 0
        val metaH = metaP.maxOfOrNull { it.height } ?: 0
        val gap = Tokens.Space.s.roundToPx() // зазор между текстом и метой

        // Стиль ОДИН на измерение и на показ: разойдись они хоть на межбуквенный
        // интервал — расчёт «влезает ли мета» пойдёт по одной ширине строки, а
        // нарисуется другая, и время слипнется с текстом.
        val textP = subcompose("text") {
            Text(text, style = style, onTextLayout = { layout = it }, modifier = mentionTap)
        }.first().measure(constraints)

        // Решаем САМИ, влезает ли мета в конец последней строки: меряем текст и
        // смотрим, где эта строка кончается. Прежний вариант — невидимая вставка
        // размера меты в конце текста — отдавал решение движку переноса, и тот
        // считал её частью последнего слова: мета уезжала на отдельную строку даже
        // когда рядом было полно места, а место под неё всё равно резервировалось
        // сбоку.
        val m = measurer.measure(
            text,
            style,
            constraints = Constraints(maxWidth = constraints.maxWidth),
        )
        val lastRight = m.getLineRight(m.lineCount - 1).toInt()
        val fits = lastRight + gap + metaW <= constraints.maxWidth

        val width = if (fits) maxOf(textP.width, lastRight + gap + metaW) else textP.width
        // Не влезла — отдаём ей строку под текстом. Ширину при этом НЕ трогаем:
        // сбоку резервировать нечего, мета уже не в строке.
        val height = textP.height + if (fits) 0 else metaH
        layout(width, height) { textP.place(0, 0) }
    }
}

/** Ключ упомянутого под точкой `pos`, если она попала в сам глиф «@имени», а не
 *  просто ближе всего к нему (тап справа от конца строки — это тап по пузырю). */
private fun mentionAt(layout: TextLayoutResult?, text: AnnotatedString, pos: androidx.compose.ui.geometry.Offset): String? {
    val l = layout ?: return null
    val o = l.getOffsetForPosition(pos)
    return listOf(o, o - 1).filter { it in text.indices }
        .firstOrNull { l.getBoundingBox(it).contains(pos) }
        ?.let { i -> text.getStringAnnotations(Mentions.TAG, i, i + 1).firstOrNull()?.item }
}

/** Форма пузыря: «шов» внутри блока — со стороны отправителя. Внешние углы серии
 *  полные, соприкасающиеся — срезанные: наоборот получаются «песочные часы». */
internal fun bubbleShape(mine: Boolean, pos: BlockPos): RoundedCornerShape {
    val r = Tokens.Radius.bubble
    val seam = Tokens.Radius.bubbleTail
    val topSeam = pos == BlockPos.Mid || pos == BlockPos.Last
    val bottomSeam = pos == BlockPos.Mid || pos == BlockPos.First
    return if (mine) {
        RoundedCornerShape(r, if (topSeam) seam else r, if (bottomSeam) seam else r, r)
    } else {
        RoundedCornerShape(if (topSeam) seam else r, r, r, if (bottomSeam) seam else r)
    }
}

/** Локализованная метка дня для чипа-разделителя: Сегодня / Вчера / «d MMMM». */
@Composable
internal fun dayLabel(ts: Long): String {
    val nowSec = System.currentTimeMillis() / 1000
    return when (dayKey(ts)) {
        dayKey(nowSec) -> stringResource(R.string.date_today)
        dayKey(nowSec - 86_400) -> stringResource(R.string.date_yesterday)
        else -> java.text.SimpleDateFormat(
            if (yearOf(ts) == yearOf(nowSec)) "d MMMM" else "d MMMM yyyy",
            java.util.Locale.getDefault(),
        ).format(java.util.Date(ts * 1000))
    }
}

/** Детерминированный цвет аватара по имени контакта. */
internal fun avatarColor(name: String): Color {
    val palette = Tokens.Colors.avatarPalette
    return palette[abs(name.hashCode()) % palette.size]
}

/** Генерирует QR-код для строки [data] размером 512×512. Возвращает null при ошибке. */
internal fun generateQrBitmap(data: String): android.graphics.Bitmap? = runCatching {
    val size = 512
    val bits = com.google.zxing.qrcode.QRCodeWriter()
        .encode(data, com.google.zxing.BarcodeFormat.QR_CODE, size, size)
    val pixels = IntArray(size * size) { i ->
        if (bits[i % size, i / size]) android.graphics.Color.BLACK else android.graphics.Color.WHITE
    }
    android.graphics.Bitmap.createBitmap(size, size, android.graphics.Bitmap.Config.ARGB_8888)
        .also { it.setPixels(pixels, 0, size, 0, 0, size, size) }
}.getOrNull()

/** Склонение по числу, когда число — Long: плюралы Android принимают только Int. */
@Composable
internal fun pluralOf(id: Int, n: Long, vararg rest: Any): String =
    androidx.compose.ui.res.pluralStringResource(id, n.coerceAtMost(Int.MAX_VALUE.toLong()).toInt(), n, *rest)

/**
 * Копирование в буфер — одно на всё приложение. С Android 13 система сама
 * показывает плашку «скопировано»; свой тост — только на старых версиях, иначе
 * их два (так было в чате и «Сохранённом»), а профиль контакта не говорил ничего.
 */
internal fun copyToClipboard(ctx: android.content.Context, text: String, label: String) {
    val cm = ctx.getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
    cm.setPrimaryClip(android.content.ClipData.newPlainText(label, text))
    if (android.os.Build.VERSION.SDK_INT < android.os.Build.VERSION_CODES.TIRAMISU) {
        android.widget.Toast.makeText(ctx, ctx.getString(R.string.common_copied), android.widget.Toast.LENGTH_SHORT).show()
    }
}

/**
 * Клавиатура для пароля: тип «пароль» и без автоисправления. Иначе клавиатура
 * учит набранное и потом подсказывает его в других приложениях — пароль от
 * хранилища или копии оказывался в её словаре.
 */
internal val PasswordKeyboard = androidx.compose.foundation.text.KeyboardOptions(
    keyboardType = androidx.compose.ui.text.input.KeyboardType.Password,
    autoCorrect = false,
)

@Composable
internal fun inputColors() = TextFieldDefaults.colors(
    focusedContainerColor = Tokens.Colors.group,
    unfocusedContainerColor = Tokens.Colors.group,
    focusedTextColor = Color.White,
    unfocusedTextColor = Color.White,
    cursorColor = HoloAccent,
    focusedIndicatorColor = HoloAccent,
    unfocusedIndicatorColor = HoloBorder,
)

/**
 * Догрузка старой истории для ленты, привязанной к НИЗУ (`reverseLayout = true`,
 * элемент 0 — самое свежее): чат открывается хвостом (движок не поднимает
 * переписку целиком), остальное приезжает страницами, когда взгляд подходит к
 * верхнему краю — к последним индексам.
 *
 * Компенсировать прокрутку не нужно: старое дописывается в конец списка, то есть
 * над видимым, и лента не сдвигается. Если история короче экрана, все элементы
 * видны сразу — условие держится, и страницы тянутся, пока не кончатся (каждая
 * меняет число элементов и будит поток снова; конец истории отмечает модель).
 *
 * Раньше лента строилась сверху вниз: чат на кадр открывался с начала и прыгал в
 * конец, а это «начало» ещё и запрашивало лишнюю страницу.
 */
@Composable
internal fun PagingOlder(listState: LazyListState, key: Any?, load: () -> Unit) {
    LaunchedEffect(key) {
        snapshotFlow {
            val info = listState.layoutInfo
            (info.visibleItemsInfo.lastOrNull()?.index ?: -1) to info.totalItemsCount
        }.collect { (last, count) -> if (count > 0 && last >= count - 4) load() }
    }
}

/**
 * Довести ленту до низа на НОВОЕ сообщение — если человек и так внизу или написал
 * сам. Читающего историю чужое входящее не утаскивает вниз: раньше любое сообщение
 * в группе выдёргивало из середины переписки.
 *
 * Ориентир — пара «автор + id» последнего, а не размер списка: догрузка старых тоже
 * меняет размер. «Внизу» — первые два элемента: новое вставляется под видимым, и на
 * момент проверки лента уже сдвинута на один.
 */
@Composable
internal fun FollowNewest(listState: LazyListState, msgs: List<Message>) {
    val last = msgs.lastOrNull()
    LaunchedEffect(last?.sender, last?.id) {
        if (last != null && (last.mine || listState.firstVisibleItemIndex <= 1)) {
            listState.animateScrollToItem(0)
        }
    }
}

/**
 * Довести ленту до сообщения [AppModel.jumpTo] — из поиска или «Показать в чате».
 *
 * Ждёт, пока сообщение окажется в ленте: из поиска чат открывается пустым, история
 * приезжает следом, и проверка в первом кадре сбрасывала цель, так ничего и не найдя.
 * Цель гасится, только когда до неё дошли (или при уходе из чата).
 */
@Composable
internal fun JumpToMessage(model: AppModel, listState: LazyListState, msgs: List<Message>) {
    val target = model.jumpTo
    LaunchedEffect(target, msgs.size) {
        if (target == 0L) return@LaunchedEffect
        val i = msgs.indexOfFirst { it.id == target }
        if (i >= 0) {
            // Сначала доехать, потом гасить: смена цели перезапускает эффект и
            // оборвала бы прокрутку на полпути.
            listState.animateScrollToItem(msgs.size - 1 - i) // лента перевёрнута: индекс от конца
            model.jumpTo = 0L
        }
    }
}

/**
 * Плашка закреплённого сообщения над лентой. Показывает ВЕРХНЕЕ (самое свежее) и
 * счётчик остальных: закреп — свойство чата, поэтому одинаков у всех участников.
 *
 * Кнопка «открепить» видна тогда же, когда доступно само действие: в личном чате —
 * всегда, в группе — только админу (иначе она обещала бы то, что движок отклонит).
 */
@Composable
internal fun PinnedBar(model: AppModel, chat: String, group: Boolean, canUnpin: Boolean) {
    val list = model.pinned[chat].orEmpty()
    val top = list.firstOrNull() ?: return
    // Полоса, а не карточка: закреплённое читается как продолжение шапки и не
    // спорит с пузырями за внимание. Слева — акцентная риска вместо иконки.
    Row(
        Modifier.fillMaxWidth().background(HoloSurface)
            .padding(horizontal = Tokens.Space.m, vertical = Tokens.Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        AccentRule()
        Spacer(Modifier.width(Tokens.Space.s))
        Column(Modifier.weight(1f)) {
            Text(
                if (list.size > 1) {
                    stringResource(R.string.pin_title_many, list.size)
                } else {
                    stringResource(R.string.pin_title)
                },
                color = HoloAccent,
                fontSize = Tokens.Font.caption,
            )
            Text(
                model.plainText(top).ifEmpty { stringResource(R.string.pin_attachment) },
                color = Color.White,
                fontSize = Tokens.Font.body,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        if (canUnpin) {
            BarIcon(Icons.Filled.Close, stringResource(R.string.pin_remove)) {
                model.setPinned(chat, group, top, false)
            }
        }
    }
}

/** Акцентная риска слева от цитаты, закрепа, полоски ответа: «это ссылка на другое
 *  сообщение». Одна на все места, чтобы толщина и высота не разъезжались. */
@Composable
internal fun AccentRule(modifier: Modifier = Modifier.height(Tokens.Size.rule)) {
    Box(
        modifier.width(Tokens.Size.ruleWidth)
            .clip(RoundedCornerShape(Tokens.Size.ruleWidth / 2)).background(HoloAccent),
    )
}

/**
 * Плашка над полем ввода (макеты `chatact/Reply`, `chatact/Edit`): значок режима, кому или
 * что, и ×. Режим ввода один за раз, поэтому и плашка одна. Низ почти прямой: плашка
 * прилегает к полю и читается его шапкой, а не отдельной карточкой.
 */
@Composable
internal fun InputStrip(icon: ImageVector, title: String, body: String, onCancel: () -> Unit) {
    val r = Tokens.Radius
    Row(
        Modifier.fillMaxWidth().padding(horizontal = Tokens.Space.m)
            .clip(RoundedCornerShape(topStart = r.field, topEnd = r.field, bottomStart = r.badge, bottomEnd = r.badge))
            .background(Tokens.Colors.group)
            .padding(start = Tokens.Space.m),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = HoloAccent, modifier = Modifier.size(Tokens.Size.iconS))
        Column(Modifier.weight(1f).padding(start = Tokens.Space.m, end = Tokens.Space.xs)) {
            Text(
                title,
                color = HoloAccent,
                fontSize = Tokens.Font.body,
                fontWeight = FontWeight.Medium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(body, color = Tokens.Colors.secondary, fontSize = Tokens.Font.body, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        Box(
            Modifier.size(Tokens.Size.touch).clip(CircleShape).clickable(role = Role.Button, onClick = onCancel),
            contentAlignment = Alignment.Center,
        ) { Icon(TablerIcons.X, stringResource(R.string.common_cancel), tint = Tokens.Colors.secondary, modifier = Modifier.size(Tokens.Size.iconS)) }
    }
}

/** Плашка «Изменение»: уйдёт правка, а не новое сообщение. */
@Composable
internal fun EditStrip(text: String, onCancel: () -> Unit) =
    InputStrip(TablerIcons.Pencil, stringResource(R.string.chat_editing), text, onCancel)

/**
 * Снекбар «Отменить» (макет `shared/Undo`): кольцо-отсчёт с секундами, что сделано, «Отменить».
 * Длительность — рекомендованная системой для текста с кнопкой: с TalkBack она дольше.
 * Вежливая живая область — появление зачитывается, не перебивая; кольцо из озвучки убрано,
 * иначе каждая секунда читалась бы вслух.
 */
@Composable
internal fun UndoSnackbar(slot: UndoSlot, modifier: Modifier = Modifier) {
    val u = slot.current ?: return
    val ctx = LocalContext.current
    val total = remember(u) { undoTimeoutMs(ctx) }
    val left = remember(u) { androidx.compose.animation.core.Animatable(1f) }
    LaunchedEffect(u) {
        left.animateTo(0f, tween(total, easing = androidx.compose.animation.core.LinearEasing))
        slot.expire(u)
    }
    val secs = kotlin.math.ceil(left.value * total / 1000f).toInt()
    Row(
        modifier.fillMaxWidth()
            .heightIn(min = Tokens.Size.primaryButton)
            .shadow(Tokens.Size.snackShadow, RoundedCornerShape(Tokens.Radius.field))
            .clip(RoundedCornerShape(Tokens.Radius.field))
            .background(Tokens.Colors.snack)
            .semantics { liveRegion = androidx.compose.ui.semantics.LiveRegionMode.Polite }
            .padding(start = Tokens.Space.l, end = Tokens.Space.s),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m),
    ) {
        Box(Modifier.size(Tokens.Size.undoRing).clearAndSetSemantics {}, contentAlignment = Alignment.Center) {
            androidx.compose.foundation.Canvas(Modifier.matchParentSize()) {
                val stroke = Tokens.Size.undoStroke.toPx()
                // Окружность r = 9 из 24 (как в макете): средняя линия обводки на 1/8 от края.
                val inset = size.minDimension / 8
                val arc = androidx.compose.ui.geometry.Size(size.width - inset * 2, size.height - inset * 2)
                val tl = androidx.compose.ui.geometry.Offset(inset, inset)
                val st = androidx.compose.ui.graphics.drawscope.Stroke(stroke)
                drawArc(Tokens.Colors.handle, 0f, 360f, false, tl, arc, style = st)
                drawArc(Tokens.Colors.accent, -90f, 360f * left.value, false, tl, arc, style = st)
            }
            Text("$secs", color = Color.White, fontSize = Tokens.Font.caption, style = Tokens.Text.meta)
        }
        Text(u.text, color = Color.White, fontSize = Tokens.Font.label, modifier = Modifier.weight(1f))
        val press = remember { MutableInteractionSource() }
        Box(
            Modifier.heightIn(min = Tokens.Size.suggest)
                .pressScale(press)
                .clip(RoundedCornerShape(Tokens.Radius.button))
                .clickable(interactionSource = press, indication = null, role = Role.Button) { slot.undo() }
                .padding(horizontal = Tokens.Space.m),
            contentAlignment = Alignment.Center,
        ) {
            Text(stringResource(R.string.undo_action), color = Tokens.Colors.accent, fontSize = Tokens.Font.label, fontWeight = FontWeight.Medium)
        }
    }
}

/** Сколько держать «Отменить»: 5 с, или сколько попросит система (TalkBack, «время на действие»). */
private fun undoTimeoutMs(ctx: android.content.Context): Int {
    val base = 5_000
    if (android.os.Build.VERSION.SDK_INT < android.os.Build.VERSION_CODES.Q) return base
    val am = ctx.getSystemService(android.content.Context.ACCESSIBILITY_SERVICE) as android.view.accessibility.AccessibilityManager
    return am.getRecommendedTimeoutMillis(
        base,
        android.view.accessibility.AccessibilityManager.FLAG_CONTENT_TEXT or
            android.view.accessibility.AccessibilityManager.FLAG_CONTENT_CONTROLS,
    )
}

/**
 * Меню одного вида (app-ui.md → «Общие элементы», макет `shared/GroupMenu`): у строки
 * чата, «⋮», в просмотре. Подложка group с тенью, строки с иконкой, опасное — после
 * разделителя, красным. Встаёт у правого верхнего угла якоря (родителя); если снизу не
 * помещается — поднимается так, чтобы низ совпал с низом якоря.
 */
@Composable
internal fun PopMenu(open: Boolean, onDismiss: () -> Unit, content: @Composable ColumnScope.() -> Unit) {
    if (!open) return
    val margin = with(androidx.compose.ui.platform.LocalDensity.current) { Tokens.Space.s.roundToPx() }
    val position = remember(margin) { AnchorEndPosition(margin) }
    androidx.compose.ui.window.Popup(
        popupPositionProvider = position,
        onDismissRequest = onDismiss,
        properties = androidx.compose.ui.window.PopupProperties(focusable = true),
    ) { MenuSurface(content = content) }
}

/** Подложка меню: ступень group с тенью и тонкой кромкой. Одна у «⋮» и у меню пузыря. */
@Composable
internal fun MenuSurface(modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    val shape = RoundedCornerShape(Tokens.Radius.group)
    Column(
        modifier.width(Tokens.Size.menu)
            .shadow(Tokens.Size.menuShadow, shape)
            .clip(shape)
            .background(Tokens.Colors.group)
            .border(1.dp, Tokens.Colors.menuEdge, shape),
        content = content,
    )
}

/** Строка меню: иконка и подпись; [trailing] — переключатель справа, если пункт им и является. */
@Composable
internal fun PopMenuItem(
    icon: ImageVector,
    label: String,
    danger: Boolean = false,
    trailing: (@Composable () -> Unit)? = null,
    onClick: () -> Unit,
) {
    Row(
        Modifier.fillMaxWidth().heightIn(min = Tokens.Size.touch)
            .clickable(role = Role.Button) { onClick() }
            .padding(horizontal = Tokens.Space.l),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(Tokens.Space.m + Tokens.Space.xxs),
    ) {
        Icon(
            icon,
            null,
            tint = if (danger) Tokens.Colors.error else Tokens.Colors.secondary,
            modifier = Modifier.size(Tokens.Size.iconRow),
        )
        Text(
            label,
            color = if (danger) Tokens.Colors.error else Color.White,
            fontSize = Tokens.Font.label,
            modifier = Modifier.weight(1f).padding(vertical = Tokens.Space.m),
        )
        trailing?.invoke()
    }
}

/** Разделитель перед опасным пунктом меню. */
@Composable
internal fun PopMenuDivider() {
    HorizontalDivider(thickness = 1.dp, color = Tokens.Colors.raised)
}

/** Правый верх меню — к правому верху якоря; не влезает вниз — вверх от низа якоря. */
private class AnchorEndPosition(private val margin: Int) : androidx.compose.ui.window.PopupPositionProvider {
    override fun calculatePosition(
        anchorBounds: androidx.compose.ui.unit.IntRect,
        windowSize: androidx.compose.ui.unit.IntSize,
        layoutDirection: androidx.compose.ui.unit.LayoutDirection,
        popupContentSize: androidx.compose.ui.unit.IntSize,
    ): androidx.compose.ui.unit.IntOffset {
        val maxX = (windowSize.width - popupContentSize.width - margin).coerceAtLeast(margin)
        val x = (anchorBounds.right - popupContentSize.width - margin).coerceIn(margin, maxX)
        val below = anchorBounds.top + popupContentSize.height <= windowSize.height - margin
        val y = if (below) anchorBounds.top else anchorBounds.bottom - popupContentSize.height
        val maxY = (windowSize.height - popupContentSize.height - margin).coerceAtLeast(margin)
        return androidx.compose.ui.unit.IntOffset(x, y.coerceIn(margin, maxY))
    }
}

/**
 * Иконка, меняющая смысл на месте (микрофон ↔ отправить, app-ui.md → «Движение»): обе в
 * дереве, уходящая гаснет и сжимается к 0.25 с размытием 4 dp, приходящая — наоборот;
 * пружина без отскока. Размытие — с Android 12, раньше только прозрачность и масштаб.
 */
@Composable
internal fun SwapIcon(second: Boolean, first: @Composable () -> Unit, other: @Composable () -> Unit) {
    val p by animateFloatAsState(
        if (second) 1f else 0f,
        androidx.compose.animation.core.spring(dampingRatio = 1f),
        label = "swap",
    )
    Box(contentAlignment = Alignment.Center) {
        SwapLayer(1f - p, first)
        SwapLayer(p, other)
    }
}

@Composable
private fun SwapLayer(shown: Float, content: @Composable () -> Unit) {
    if (shown <= 0.001f) return
    Box(
        Modifier.graphicsLayer {
            alpha = shown
            val s = 0.25f + 0.75f * shown
            scaleX = s
            scaleY = s
        }.blur(Tokens.Size.swapBlur * (1f - shown), androidx.compose.ui.draw.BlurredEdgeTreatment.Unbounded),
        contentAlignment = Alignment.Center,
    ) { content() }
}
