package chat.peerage.ui

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.PathParser
import androidx.compose.ui.unit.dp

/**
 * Курируемый набор иконок Tabler (MIT-лицензия) как Compose [ImageVector].
 *
 * app-ui.md требует «иконки — Tabler-стиль (обводка, не эмодзи)»; в приложении раньше
 * стояли Material-иконки как приближение. Здесь — только реально используемые иконки
 * (философия «свой курируемый набор», как fingerprint-эмодзи), без внешней зависимости
 * и без нужды в R8-tree-shaking → нулевой вес-оверхед.
 *
 * ДОБАВЛЕНИЕ иконки: взять path-данные из tabler-icons `icons/outline/<name>.svg`
 * (обводка 1.5 при 20 dp — app-ui.md, круглые концы — [tabler] это проставляет) и завести `val`.
 * Тинт задаёт вызывающий через `Icon(tint = …)` — цвет обводки здесь неважен.
 */
object TablerIcons

/** Собрать ImageVector из одной или нескольких SVG-path строк Tabler (24×24, обводка 1.5). */
private fun tabler(name: String, vararg paths: String): ImageVector =
    ImageVector.Builder(
        name = "ti_$name",
        defaultWidth = 24.dp,
        defaultHeight = 24.dp,
        viewportWidth = 24f,
        viewportHeight = 24f,
    ).apply {
        for (d in paths) {
            addPath(
                pathData = PathParser().parsePathString(d).toNodes(),
                stroke = SolidColor(Color.Black), // перекрывается Icon(tint)
                strokeLineWidth = 1.5f,
                strokeLineCap = StrokeCap.Round,
                strokeLineJoin = StrokeJoin.Round,
            )
        }
    }.build()

val TablerIcons.Menu2: ImageVector by lazy {
    tabler("menu-2", "M4 6l16 0", "M4 12l16 0", "M4 18l16 0")
}

val TablerIcons.Search: ImageVector by lazy {
    tabler("search", "M3 10a7 7 0 1 0 14 0a7 7 0 1 0 -14 0", "M21 21l-6 -6")
}

val TablerIcons.Messages: ImageVector by lazy {
    tabler(
        "messages",
        "M21 14l-3 -3h-7a1 1 0 0 1 -1 -1v-6a1 1 0 0 1 1 -1h9a1 1 0 0 1 1 1v10",
        "M14 15v2a1 1 0 0 1 -1 1h-7l-3 3v-10a1 1 0 0 1 1 -1h2",
    )
}

val TablerIcons.Message: ImageVector by lazy {
    tabler(
        "message",
        "M8 9h8",
        "M8 13h6",
        "M18 4a3 3 0 0 1 3 3v8a3 3 0 0 1 -3 3h-5l-5 3v-3h-2a3 3 0 0 1 -3 -3v-8a3 3 0 0 1 3 -3h12",
    )
}

val TablerIcons.Headphones: ImageVector by lazy {
    tabler(
        "headphones",
        "M4 15a2 2 0 0 1 2 -2h1a2 2 0 0 1 2 2v3a2 2 0 0 1 -2 2h-1a2 2 0 0 1 -2 -2l0 -3",
        "M15 15a2 2 0 0 1 2 -2h1a2 2 0 0 1 2 2v3a2 2 0 0 1 -2 2h-1a2 2 0 0 1 -2 -2l0 -3",
        "M4 15v-3a8 8 0 0 1 16 0v3",
    )
}

val TablerIcons.Speakerphone: ImageVector by lazy {
    tabler(
        "speakerphone",
        "M18 8a3 3 0 0 1 0 6",
        "M10 8v11a1 1 0 0 1 -1 1h-1a1 1 0 0 1 -1 -1v-5",
        "M12 8l4.524 -3.77a.9 .9 0 0 1 1.476 .692v12.156a.9 .9 0 0 1 -1.476 .692l-4.524 -3.77h-8a1 1 0 0 1 -1 -1v-4a1 1 0 0 1 1 -1h8",
    )
}

val TablerIcons.Users: ImageVector by lazy {
    tabler(
        "users",
        "M5 7a4 4 0 1 0 8 0a4 4 0 1 0 -8 0",
        "M3 21v-2a4 4 0 0 1 4 -4h4a4 4 0 0 1 4 4v2",
        "M16 3.13a4 4 0 0 1 0 7.75",
        "M21 21v-2a4 4 0 0 0 -3 -3.85",
    )
}

/** Верификация (телеграм-стиль): галочка в фигурной розетке — tabler rosette-discount-check. */
val TablerIcons.Verified: ImageVector by lazy {
    tabler(
        "rosette-discount-check",
        "M5 7.2a2.2 2.2 0 0 1 2.2 -2.2h1a2.2 2.2 0 0 0 1.55 -.64l.7 -.7a2.2 2.2 0 0 1 3.12 0l.7 .7c.412 .41 .97 .64 1.55 .64h1a2.2 2.2 0 0 1 2.2 2.2v1c0 .58 .23 1.138 .64 1.55l.7 .7a2.2 2.2 0 0 1 0 3.12l-.7 .7a2.2 2.2 0 0 0 -.64 1.55v1a2.2 2.2 0 0 1 -2.2 2.2h-1a2.2 2.2 0 0 0 -1.55 .64l-.7 .7a2.2 2.2 0 0 1 -3.12 0l-.7 -.7a2.2 2.2 0 0 0 -1.55 -.64h-1a2.2 2.2 0 0 1 -2.2 -2.2v-1a2.2 2.2 0 0 0 -.64 -1.55l-.7 -.7a2.2 2.2 0 0 1 0 -3.12l.7 -.7a2.2 2.2 0 0 0 .64 -1.55v-1",
        "M9 12l2 2l4 -4",
    )
}

val TablerIcons.Microphone: ImageVector by lazy {
    tabler(
        "microphone",
        "M9 5a3 3 0 0 1 3 -3a3 3 0 0 1 3 3v5a3 3 0 0 1 -3 3a3 3 0 0 1 -3 -3l0 -5",
        "M5 10a7 7 0 0 0 14 0",
        "M8 21l8 0",
        "M12 17l0 4",
    )
}

val TablerIcons.BellOff: ImageVector by lazy {
    tabler(
        "bell-off",
        "M9.346 5.353c.21 -.129 .428 -.246 .654 -.353a2 2 0 1 1 4 0a7 7 0 0 1 4 6v3m-1 3h-13a4 4 0 0 0 2 -3v-3a6.996 6.996 0 0 1 1.273 -3.707",
        "M9 17v1a3 3 0 0 0 6 0v-1",
        "M3 3l18 18",
    )
}

val TablerIcons.Pencil: ImageVector by lazy {
    tabler(
        "pencil",
        "M4 20h4l10.5 -10.5a2.828 2.828 0 1 0 -4 -4l-10.5 10.5v4",
        "M13.5 6.5l4 4",
    )
}

/** Канцелярская кнопка — закреплённое сообщение. */
val TablerIcons.Pin: ImageVector by lazy {
    tabler(
        "pin",
        "M15 4.5l-4 4l-4 1.5l-1.5 1.5l7 7l1.5 -1.5l1.5 -4l4 -4",
        "M9 15l-4.5 4.5",
        "M14.5 4l5.5 5.5",
    )
}

val TablerIcons.Plus: ImageVector by lazy {
    tabler("plus", "M12 5l0 14", "M5 12l14 0")
}

// ── Онбординг (M4) ──
val TablerIcons.ShieldLock: ImageVector by lazy {
    tabler(
        "shield-lock",
        "M12 3a12 12 0 0 0 8.5 3a12 12 0 0 1 -8.5 15a12 12 0 0 1 -8.5 -15a12 12 0 0 0 8.5 -3",
        "M11 11a1 1 0 1 0 2 0a1 1 0 1 0 -2 0",
        "M12 12l0 2.5",
    )
}

val TablerIcons.PhoneOff: ImageVector by lazy {
    tabler(
        "phone-off",
        "M3 21l18 -18",
        "M5.831 14.161a15.946 15.946 0 0 1 -2.831 -8.161a2 2 0 0 1 2 -2h4l2 5l-2.5 1.5c.108 .22 .223 .435 .345 .645m1.751 2.277c.843 .84 1.822 1.544 2.904 2.078l1.5 -2.5l5 2v4a2 2 0 0 1 -2 2a15.963 15.963 0 0 1 -10.344 -4.657",
    )
}

val TablerIcons.AddressBookOff: ImageVector by lazy {
    tabler(
        "address-book-off",
        "M8 4h10a2 2 0 0 1 2 2v10m-.57 3.399c-.363 .37 -.87 .601 -1.43 .601h-10a2 2 0 0 1 -2 -2v-12",
        "M10 16h6",
        "M11 11a2 2 0 0 0 2 2m2 -2a2 2 0 0 0 -2 -2",
        "M4 8h3",
        "M4 12h3",
        "M4 16h3",
        "M3 3l18 18",
    )
}

val TablerIcons.MapPinOff: ImageVector by lazy {
    tabler(
        "map-pin-off",
        "M9.442 9.432a3 3 0 0 0 4.113 4.134m1.445 -2.566a3 3 0 0 0 -3 -3",
        "M17.152 17.162l-3.738 3.738a2 2 0 0 1 -2.827 0l-4.244 -4.243a8 8 0 0 1 -.476 -10.794m2.18 -1.82a8.003 8.003 0 0 1 10.91 10.912",
        "M3 3l18 18",
    )
}

val TablerIcons.ServerOff: ImageVector by lazy {
    tabler(
        "server-off",
        "M12 12h-6a3 3 0 0 1 -3 -3v-2c0 -1.083 .574 -2.033 1.435 -2.56m3.565 -.44h10a3 3 0 0 1 3 3v2a3 3 0 0 1 -3 3h-2",
        "M16 12h2a3 3 0 0 1 3 3v2m-1.448 2.568a2.986 2.986 0 0 1 -1.552 .432h-12a3 3 0 0 1 -3 -3v-2a3 3 0 0 1 3 -3h6",
        "M7 8v.01",
        "M7 16v.01",
        "M3 3l18 18",
    )
}

val TablerIcons.User: ImageVector by lazy {
    tabler(
        "user",
        "M8 7a4 4 0 1 0 8 0a4 4 0 0 0 -8 0",
        "M6 21v-2a4 4 0 0 1 4 -4h4a4 4 0 0 1 4 4v2",
    )
}

val TablerIcons.Camera: ImageVector by lazy {
    tabler(
        "camera",
        "M5 7h1a2 2 0 0 0 2 -2a1 1 0 0 1 1 -1h6a1 1 0 0 1 1 1a2 2 0 0 0 2 2h1a2 2 0 0 1 2 2v9a2 2 0 0 1 -2 2h-14a2 2 0 0 1 -2 -2v-9a2 2 0 0 1 2 -2",
        "M9 13a3 3 0 1 0 6 0a3 3 0 0 0 -6 0",
    )
}

val TablerIcons.AlertTriangle: ImageVector by lazy {
    tabler(
        "alert-triangle",
        "M12 9v4",
        "M10.363 3.591l-8.106 13.534a1.914 1.914 0 0 0 1.636 2.871h16.214a1.914 1.914 0 0 0 1.636 -2.87l-8.106 -13.536a1.914 1.914 0 0 0 -3.274 0",
        "M12 16h.01",
    )
}

val TablerIcons.FileDownload: ImageVector by lazy {
    tabler(
        "file-download",
        "M14 3v4a1 1 0 0 0 1 1h4",
        "M17 21h-10a2 2 0 0 1 -2 -2v-14a2 2 0 0 1 2 -2h7l5 5v11a2 2 0 0 1 -2 2",
        "M12 17v-6",
        "M9.5 14.5l2.5 2.5l2.5 -2.5",
    )
}

val TablerIcons.ListNumbers: ImageVector by lazy {
    tabler(
        "list-numbers",
        "M11 6h9",
        "M11 12h9",
        "M12 18h8",
        "M4 16a2 2 0 1 1 4 0c0 .591 -.5 1 -1 1.5l-3 2.5h4",
        "M6 10v-6l-2 2",
    )
}

val TablerIcons.ChevronRight: ImageVector by lazy {
    tabler("chevron-right", "M9 6l6 6l-6 6")
}

val TablerIcons.ShieldCheck: ImageVector by lazy {
    tabler(
        "shield-check",
        "M11.46 20.846a12 12 0 0 1 -7.96 -14.846a12 12 0 0 0 8.5 -3a12 12 0 0 0 8.5 3a12 12 0 0 1 -.09 7.06",
        "M15 19l2 2l4 -4",
    )
}

// Щиты экрана сверки (макет `security/`): галочка и вопрос ВНУТРИ щита.
val TablerIcons.ShieldTick: ImageVector by lazy {
    tabler("shield-tick", "M12 3l7 3v6c0 4.5 -3 7.5 -7 9c-4 -1.5 -7 -4.5 -7 -9V6z", "M9 12l2 2l4 -4")
}

val TablerIcons.ShieldQuestion: ImageVector by lazy {
    tabler(
        "shield-question",
        "M12 3l7 3v6c0 4.5 -3 7.5 -7 9c-4 -1.5 -7 -4.5 -7 -9V6z",
        "M10 9.5a2 2 0 1 1 2.5 2c-.4 .2 -.5 .6 -.5 1",
        "M12 15.5h.01",
    )
}

val TablerIcons.Check: ImageVector by lazy {
    tabler(
        "check",
        "M5 12l5 5l10 -10",
    )
}

val TablerIcons.Bell: ImageVector by lazy {
    tabler(
        "bell",
        "M10 5a2 2 0 1 1 4 0a7 7 0 0 1 4 6v3a4 4 0 0 0 2 3h-16a4 4 0 0 0 2 -3v-3a7 7 0 0 1 4 -6",
        "M9 17v1a3 3 0 0 0 6 0v-1",
    )
}

val TablerIcons.BatteryCharging: ImageVector by lazy {
    tabler(
        "battery-charging",
        "M16 7h1a2 2 0 0 1 2 2v.5a.5 .5 0 0 0 .5 .5a.5 .5 0 0 1 .5 .5v3a.5 .5 0 0 1 -.5 .5a.5 .5 0 0 0 -.5 .5v.5a2 2 0 0 1 -2 2h-2",
        "M8 7h-2a2 2 0 0 0 -2 2v6a2 2 0 0 0 2 2h1",
        "M12 8l-2 4h3l-2 4",
    )
}

val TablerIcons.Phone: ImageVector by lazy {
    tabler(
        "phone",
        "M5 4h4l2 5l-2.5 1.5a11 11 0 0 0 5 5l1.5 -2.5l5 2v4a2 2 0 0 1 -2 2a16 16 0 0 1 -15 -15a2 2 0 0 1 2 -2",
    )
}

val TablerIcons.Bookmark: ImageVector by lazy {
    tabler(
        "bookmark",
        "M18 7v14l-6 -4l-6 4v-14a4 4 0 0 1 4 -4h4a4 4 0 0 1 4 4z",
    )
}

val TablerIcons.Topology: ImageVector by lazy {
    tabler(
        "topology-star-3",
        "M10 19a2 2 0 1 0 -4 0a2 2 0 0 0 4 0z",
        "M18 5a2 2 0 1 0 -4 0a2 2 0 0 0 4 0z",
        "M10 5a2 2 0 1 0 -4 0a2 2 0 0 0 4 0z",
        "M6 12a2 2 0 1 0 -4 0a2 2 0 0 0 4 0z",
        "M18 19a2 2 0 1 0 -4 0a2 2 0 0 0 4 0z",
        "M14 12a2 2 0 1 0 -4 0a2 2 0 0 0 4 0z",
        "M22 12a2 2 0 1 0 -4 0a2 2 0 0 0 4 0z",
        "M6 12h4",
        "M14 12h4",
        "M15 7l-2 3",
        "M9 7l2 3",
        "M11 14l-2 3",
        "M13 14l2 3",
    )
}

val TablerIcons.Database: ImageVector by lazy {
    tabler(
        "database",
        "M12 6m-8 0a8 3 0 1 0 16 0a8 3 0 1 0 -16 0",
        "M4 6v6a8 3 0 0 0 16 0v-6",
        "M4 12v6a8 3 0 0 0 16 0v-6",
    )
}

val TablerIcons.EyeOff: ImageVector by lazy {
    tabler(
        "eye-off",
        "M10.585 10.587a2 2 0 0 0 2.829 2.828",
        "M16.681 16.673a8.717 8.717 0 0 1 -4.681 1.327c-3.6 0 -6.6 -2 -9 -6c1.272 -2.12 2.712 -3.678 4.32 -4.674m2.86 -1.146a9.055 9.055 0 0 1 1.82 -.18c3.6 0 6.6 2 9 6c-.666 1.11 -1.379 2.067 -2.138 2.87",
        "M3 3l18 18",
    )
}

val TablerIcons.Video: ImageVector by lazy {
    tabler(
        "video",
        "M15 10l4.553 -2.276a1 1 0 0 1 1.447 .894v6.764a1 1 0 0 1 -1.447 .894l-4.553 -2.276v-4z",
        "M3 6m0 2a2 2 0 0 1 2 -2h8a2 2 0 0 1 2 2v8a2 2 0 0 1 -2 2h-8a2 2 0 0 1 -2 -2z",
    )
}

val TablerIcons.InfoCircle: ImageVector by lazy {
    tabler(
        "info-circle",
        "M3 12a9 9 0 1 0 18 0a9 9 0 0 0 -18 0",
        "M12 9h.01",
        "M11 12h1v4h1",
    )
}

val TablerIcons.Qrcode: ImageVector by lazy {
    tabler(
        "qrcode",
        "M4 4m0 1a1 1 0 0 1 1 -1h4a1 1 0 0 1 1 1v4a1 1 0 0 1 -1 1h-4a1 1 0 0 1 -1 -1z",
        "M7 17l0 .01",
        "M14 4m0 1a1 1 0 0 1 1 -1h4a1 1 0 0 1 1 1v4a1 1 0 0 1 -1 1h-4a1 1 0 0 1 -1 -1z",
        "M7 7l0 .01",
        "M4 14m0 1a1 1 0 0 1 1 -1h4a1 1 0 0 1 1 1v4a1 1 0 0 1 -1 1h-4a1 1 0 0 1 -1 -1z",
        "M17 7l0 .01",
        "M14 14l3 0",
        "M20 14l0 .01",
        "M14 14l0 3",
        "M14 20l3 0",
        "M17 17l3 0",
        "M20 17l0 3",
    )
}

val TablerIcons.Copy: ImageVector by lazy {
    tabler(
        "copy",
        "M7 7m0 2.667a2.667 2.667 0 0 1 2.667 -2.667h8.666a2.667 2.667 0 0 1 2.667 2.667v8.666a2.667 2.667 0 0 1 -2.667 2.667h-8.666a2.667 2.667 0 0 1 -2.667 -2.667z",
        "M4.012 16.737a2.005 2.005 0 0 1 -1.012 -1.737v-10c0 -1.1 .9 -2 2 -2h10c.75 0 1.158 .385 1.5 1",
    )
}

val TablerIcons.Share: ImageVector by lazy {
    tabler(
        "share",
        "M6 12m-3 0a3 3 0 1 0 6 0a3 3 0 1 0 -6 0",
        "M18 6m-3 0a3 3 0 1 0 6 0a3 3 0 1 0 -6 0",
        "M18 18m-3 0a3 3 0 1 0 6 0a3 3 0 1 0 -6 0",
        "M8.7 10.7l6.6 -3.4",
        "M8.7 13.3l6.6 3.4",
    )
}

val TablerIcons.Scan: ImageVector by lazy {
    tabler(
        "scan",
        "M4 7v-1a2 2 0 0 1 2 -2h2",
        "M4 17v1a2 2 0 0 0 2 2h2",
        "M16 4h2a2 2 0 0 1 2 2v1",
        "M16 20h2a2 2 0 0 0 2 -2v-1",
        "M5 12l14 0",
    )
}

val TablerIcons.Clock: ImageVector by lazy {
    tabler(
        "clock",
        "M3 12a9 9 0 1 0 18 0a9 9 0 0 0 -18 0",
        "M12 7v5l3 3",
    )
}

/** Ключ — полосы и экраны копии ключа (контур как в макете `empty/BackupStrips`). */
val TablerIcons.Key: ImageVector by lazy {
    tabler("key", "M8 15m-4 0a4 4 0 1 0 8 0a4 4 0 1 0 -8 0", "M11 12l9 -9", "M17 6l3 3", "M14 9l2 2")
}

/** Меню «⋮» и шапки (макеты `shared/Main`, `shared/GroupMenu`). */
val TablerIcons.DotsVertical: ImageVector by lazy {
    tabler(
        "dots-vertical",
        "M11 12a1 1 0 1 0 2 0a1 1 0 1 0 -2 0",
        "M11 19a1 1 0 1 0 2 0a1 1 0 1 0 -2 0",
        "M11 5a1 1 0 1 0 2 0a1 1 0 1 0 -2 0",
    )
}

val TablerIcons.Trash: ImageVector by lazy {
    tabler(
        "trash",
        "M4 7l16 0",
        "M10 11l0 6",
        "M14 11l0 6",
        "M5 7l1 12a2 2 0 0 0 2 2h8a2 2 0 0 0 2 -2l1 -12",
        "M9 7v-3a1 1 0 0 1 1 -1h4a1 1 0 0 1 1 1v3",
    )
}

val TablerIcons.Ban: ImageVector by lazy {
    tabler("ban", "M3 12a9 9 0 1 0 18 0a9 9 0 1 0 -18 0", "M5.7 5.7l12.6 12.6")
}

val TablerIcons.Logout: ImageVector by lazy {
    tabler(
        "logout",
        "M14 8v-2a2 2 0 0 0 -2 -2h-7a2 2 0 0 0 -2 2v12a2 2 0 0 0 2 2h7a2 2 0 0 0 2 -2v-2",
        "M9 12h12l-3 -3",
        "M18 15l3 -3",
    )
}

val TablerIcons.Reply: ImageVector by lazy {
    tabler("arrow-back-up", "M9 14l-4 -4l4 -4", "M5 10h11a4 4 0 1 1 0 8h-1")
}

val TablerIcons.Forward: ImageVector by lazy {
    tabler("arrow-forward-up", "M15 14l4 -4l-4 -4", "M19 10h-11a4 4 0 1 0 0 8h1")
}

val TablerIcons.CircleCheck: ImageVector by lazy {
    tabler("circle-check", "M3 12a9 9 0 1 0 18 0a9 9 0 1 0 -18 0", "M9 12l2 2l4 -4")
}

val TablerIcons.AlertCircle: ImageVector by lazy {
    tabler("alert-circle", "M3 12a9 9 0 1 0 18 0a9 9 0 0 0 -18 0", "M12 8v4", "M12 16h.01")
}

val TablerIcons.Paperclip: ImageVector by lazy {
    tabler("paperclip", "M20 11l-8 8a5 5 0 0 1 -7 -7l8 -8a3.5 3.5 0 0 1 5 5l-8 8a2 2 0 0 1 -3 -3l7 -7")
}

val TablerIcons.MoodSmile: ImageVector by lazy {
    tabler("mood-smile", "M3 12a9 9 0 1 0 18 0a9 9 0 1 0 -18 0", "M8.5 14.5a4.5 4.5 0 0 0 7 0", "M9 9.5h.01", "M15 9.5h.01")
}

val TablerIcons.Send: ImageVector by lazy {
    tabler("send", "M4 12l16 -8l-6 16l-3 -7z", "M11 13l9 -9")
}

val TablerIcons.Lock: ImageVector by lazy {
    tabler("lock", "M8 11h8a2 2 0 0 1 2 2v5a2 2 0 0 1 -2 2h-8a2 2 0 0 1 -2 -2v-5a2 2 0 0 1 2 -2z", "M9 11v-3a3 3 0 0 1 6 0v3")
}

val TablerIcons.ChevronUp: ImageVector by lazy { tabler("chevron-up", "M7 14l5 -5l5 5") }

val TablerIcons.ChevronDown: ImageVector by lazy { tabler("chevron-down", "M7 10l5 5l5 -5") }

val TablerIcons.Calendar: ImageVector by lazy {
    tabler(
        "calendar",
        "M4 7a2 2 0 0 1 2 -2h12a2 2 0 0 1 2 2v12a2 2 0 0 1 -2 2h-12a2 2 0 0 1 -2 -2v-12z",
        "M16 3v4",
        "M8 3v4",
        "M4 11h16",
    )
}

/** «Списком» в поиске: строки с маркерами. */
val TablerIcons.ListLines: ImageVector by lazy {
    tabler("list", "M9 6l11 0", "M9 12l11 0", "M9 18l11 0", "M5 6l0 .01", "M5 12l0 .01", "M5 18l0 .01")
}

/** «В ленте» в поиске: строки вразбежку, как пузыри (макет `chatsearch/List`). */
val TablerIcons.FeedLines: ImageVector by lazy { tabler("feed", "M4 6h10", "M10 12h10", "M4 18h10") }

val TablerIcons.ChevronLeft: ImageVector by lazy { tabler("chevron-left", "M15 6l-6 6l6 6") }

val TablerIcons.PlayerStop: ImageVector by lazy {
    tabler("player-stop", "M9 7h6a2 2 0 0 1 2 2v6a2 2 0 0 1 -2 2h-6a2 2 0 0 1 -2 -2v-6a2 2 0 0 1 2 -2z")
}

val TablerIcons.File: ImageVector by lazy {
    tabler("file", "M14 3h-7a2 2 0 0 0 -2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2 -2v-11z", "M14 3v5h5")
}

val TablerIcons.Music: ImageVector by lazy {
    tabler(
        "music",
        "M4.5 17a2.5 2.5 0 1 0 5 0a2.5 2.5 0 1 0 -5 0",
        "M14.5 15a2.5 2.5 0 1 0 5 0a2.5 2.5 0 1 0 -5 0",
        "M9.5 17v-11l10 -2v11",
    )
}

val TablerIcons.X: ImageVector by lazy {
    tabler("x", "M18 6l-12 12", "M6 6l12 12")
}

val TablerIcons.UserPlus: ImageVector by lazy {
    tabler("user-plus", "M8 7a4 4 0 1 0 8 0a4 4 0 0 0 -8 0", "M16 19h6", "M19 16v6", "M6 21v-2a4 4 0 0 1 4 -4h4")
}

/** «Медиа и файлы» в профиле. */
val TablerIcons.Photo: ImageVector by lazy {
    tabler(
        "photo",
        "M15 8h.01",
        "M3 6a3 3 0 0 1 3 -3h12a3 3 0 0 1 3 3v12a3 3 0 0 1 -3 3h-12a3 3 0 0 1 -3 -3v-12z",
        "M3 16l5 -5c.928 -.893 2.072 -.893 3 0l5 5",
        "M14 14l1 -1c.928 -.893 2.072 -.893 3 0l3 3",
    )
}

val TablerIcons.Download: ImageVector by lazy { tabler("download", "M12 4v11", "M7 10l5 5l5 -5", "M5 20h14") }

val TablerIcons.LayoutGrid: ImageVector by lazy {
    tabler(
        "layout-grid",
        "M4 5a1 1 0 0 1 1 -1h4a1 1 0 0 1 1 1v4a1 1 0 0 1 -1 1h-4a1 1 0 0 1 -1 -1z",
        "M14 5a1 1 0 0 1 1 -1h4a1 1 0 0 1 1 1v4a1 1 0 0 1 -1 1h-4a1 1 0 0 1 -1 -1z",
        "M4 15a1 1 0 0 1 1 -1h4a1 1 0 0 1 1 1v4a1 1 0 0 1 -1 1h-4a1 1 0 0 1 -1 -1z",
        "M14 15a1 1 0 0 1 1 -1h4a1 1 0 0 1 1 1v4a1 1 0 0 1 -1 1h-4a1 1 0 0 1 -1 -1z",
    )
}

val TablerIcons.Link: ImageVector by lazy {
    tabler(
        "link",
        "M9 15l6 -6",
        "M11 6l.463 -.536a5 5 0 0 1 7.071 7.072l-.534 .464",
        "M13 18l-.397 .534a5.068 5.068 0 0 1 -7.127 0a4.972 4.972 0 0 1 0 -7.071l.524 -.463",
    )
}

val TablerIcons.PauseBars: ImageVector by lazy { tabler("pause-bars", "M8 5v14", "M16 5v14") }

val TablerIcons.PlayOutline: ImageVector by lazy { tabler("play-outline", "M7 4v16l13 -8z") }

val TablerIcons.Volume: ImageVector by lazy {
    tabler("volume", "M4 10v4h4l5 4v-12l-5 4z", "M16.5 9a4 4 0 0 1 0 6", "M19 6.5a7.5 7.5 0 0 1 0 11")
}

val TablerIcons.VolumeOff: ImageVector by lazy { tabler("volume-off", "M4 10v4h4l5 4v-12l-5 4z", "M16 9l6 6", "M22 9l-6 6") }

val TablerIcons.Crown: ImageVector by lazy { tabler("crown", "M12 6l4 6l5 -4l-2 10h-14l-2 -10l5 4z") }
