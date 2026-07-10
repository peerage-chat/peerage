package dev.p2p.messenger.ui

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
 * (обводка 2px, круглые концы — [tabler] это проставляет) и завести `val`.
 * Тинт задаёт вызывающий через `Icon(tint = …)` — цвет обводки здесь неважен.
 */
object TablerIcons

/** Собрать ImageVector из одной или нескольких SVG-path строк Tabler (24×24, stroke 2px). */
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
                strokeLineWidth = 2f,
                strokeLineCap = StrokeCap.Round,
                strokeLineJoin = StrokeJoin.Round,
            )
        }
    }.build()

val TablerIcons.Affiliate: ImageVector by lazy {
    tabler(
        "affiliate",
        "M5.931 6.936l1.275 4.249m5.607 5.609l4.251 1.275",
        "M11.683 12.317l5.759 -5.759",
        "M4 5.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0 -3 0",
        "M17 5.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0 -3 0",
        "M17 18.5a1.5 1.5 0 1 0 3 0a1.5 1.5 0 1 0 -3 0",
        "M4 15.5a4.5 4.5 0 1 0 9 0a4.5 4.5 0 1 0 -9 0",
    )
}

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
