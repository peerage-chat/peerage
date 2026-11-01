package chat.peerage

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.LineHeightStyle
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp

/**
 * ЕДИНЫЙ источник истины по дизайн-токенам: цвета, отступы, радиусы, размеры,
 * типографика. Документация и обоснование значений — `docs/design/tokens.md`.
 *
 * ПРАВИЛО: новый UI-код ссылается на `Tokens.*`, а НЕ на инлайн-хекс/магические dp.
 * Если нужного токена нет — добавь сюда (и в tokens.md), а не пиши `Color(0x…)` в экране.
 * Семантические псевдонимы из [Theme.kt] (HoloAccent/ScreenPad/…) теперь бэкаются
 * отсюда — старые экраны не трогаем, мигрируем по ходу.
 */
/**
 * Golos Text (OFL, `docs/licenses/golos-text-OFL.txt`) — шрифт всего приложения: рисовался под
 * кириллицу. Три начертания файлами, а не вариативный шрифт: minSdk 24, оси до API 26 не работают.
 */
val Golos = FontFamily(
    androidx.compose.ui.text.font.Font(R.font.golos_regular, FontWeight.Normal),
    androidx.compose.ui.text.font.Font(R.font.golos_medium, FontWeight.Medium),
    androidx.compose.ui.text.font.Font(R.font.golos_bold, FontWeight.Bold),
)

object Tokens {
    object Colors {
        // ── Бренд/акцент ──────────────────────────────────────────────────────
        val accent = Color(0xFF33B5E5)
        val accentDark = Color(0xFF04222E) // текст/иконка НА акценте
        val accentMuted = Color(0xFF13313D) // выбранный чип/таб (приглушённый акцент)
        val accentSoft = Color(0xFF7FD0EE) // текст на accentMuted: метка, пилюля
        val accentBadge = Color(0x2233B5E5) // фон бейджа (акцент @13%)

        // ── Поверхности ───────────────────────────────────────────────────────
        // Язык «лист» (app-ui.md): фон → панель со скруглённым верхом → группа строк на
        // панели. Шаг яркости заметный — иначе экран читается одним фоном.
        val bg = Color(0xFF08090B) // фон экрана, шапки
        val panel = Color(0xFF16171B) // лист под шапкой: список, лента чата
        val group = Color(0xFF1C1D22) // группа строк, поле, кнопка — ступень на панели

        // Карточки, плашки и поля экранов стоят НА панели — значит, на ступени group.
        val surface = group
        val raised = Color(0xFF26272D) // плитка иконки в группе, кнопка в шторке — ступень над group
        val surfaceHi = raised
        val rowPinned = group // закреплённая строка — ступень над панелью
        val divider = Color(0xFF222328) // разделитель внутри группы, от аватара
        val border = Color(0xFF333333)
        val callBg = Color(0xFF131418) // фон экрана звонка (темнее и тайлов, и фона приложения)
        val tile = Color(0xFF1A1D21) // тайлы звонка, поля ввода, невыбранные чипы
        val avatar = Color(0xFF3A3F45) // фон аватара-заглушки

        // ── Текст/иконки ──────────────────────────────────────────────────────
        val onDark = Color.White

        // Оба серых держат 4.5:1 и на ступени group (#1C1D22): прежние #82828C/#6F6F78
        // проваливали контраст на новых слоях (проверка 2026-11-01).
        val secondary = Color(0xFF9A9AA4) // вторичный текст: превью чата, подписи
        val tertiary = Color(0xFF8E8E98) // время, подсказки в полях — тише вторичного
        val iconMuted = Color(0xFF5A5F66) // иконка-заглушка (превью)
        val iconChrome = Color(0xFFE6E7EA) // иконки chrome-чипов
        val onOnline = Color(0xFF0A1F0A) // текст на зелёном (мини-аватар)

        // ── Статусы ───────────────────────────────────────────────────────────
        val online = Color(0xFF8BC34A)
        val verified = Color(0xFF4CAF50)
        val error = Color(0xFFFF6B6B) // ТЕКСТ и знак ошибки/опасного действия (6:1 на group)
        val errorFill = Color(0xFFE53935) // ЗАЛИВКА: отклонить звонок, запись голосового
        val warn = Color(0xFFE6A23C)
        val offlineDot = Color(0xFF555555)

        // ── Кнопки звонка ─────────────────────────────────────────────────────
        val btnNeutral = Color(0xFF3A3D44) // mic on / invite / camera
        val btnDangerMuted = Color(0xFF3A2A2C) // выключенное (mic off / video off)
        val dangerPlate = Color(0xFF3A1E22) // красная кнопка в шторке (текст iconDangerMuted)
        val handle = Color(0xFF3A3B42) // ручка шторки
        val rowRequest = Color(0xFF1A1B20) // строка запроса/приглашения на главной — чуть другой фон
        val iconDangerMuted = Color(0xFFFF8A8A) // иконка выключенного

        // ── Чат ───────────────────────────────────────────────────────────────
        val bubbleIn = Color(0xFF23242A) // входящий — на ступень светлее панели
        val bubbleOut = Color(0xFF1F4657) // исходящий — приглушённый акцент
        val labelScrim = Color(0x0FFFFFFF) // подложка ярлыка даты/системного события
        val folderSelected = Color(0xFF1C3A47)
        val unreadMuted = Color(0xFF3A3A42) // счётчик чата без звука
        val selectedRow = Color(0x2933B5E5) // строка выделенного сообщения (акцент @16%)

        // Цитата ВНУТРИ цветного пузыря: полупрозрачный белый, а не наш серый — серый
        // на синем исходящем читается пятном чужого слоя.
        val quoteOnBubble = Color(0x1AFFFFFF) // подложка (белый @10%)
        val quoteTextOnBubble = Color(0xB3FFFFFF) // текст цитаты (белый @70%)
        val metaOnBubble = Color(0x99FFFFFF) // время и «изм.» (белый @60%)
        val mediaScrim = Color(0x8C000000) // плашка поверх кадра: длительность, время (чёрный @55%)
        val playScrim = Color(0x73000000) // круг «play» поверх кадра (чёрный @45%)
        val thumbScrim = Color(0x40000000) // квадрат превью в карточке вложения (чёрный @25%)
        val trackOnMedia = Color(0x40FFFFFF) // дорожка таймлайна поверх кадра (белый @25%)

        // Упоминание на исходящем пузыре: чистый акцент на bubbleOut почти сливается.
        val mentionOnOut = Color(0xFFB5E6F7)
        val mentionSelfBg = accentBadge // упомянули МЕНЯ — подложка под участком

        // ── Скримы/оверлеи ────────────────────────────────────────────────────
        val overlay = Color(0xF2000000) // полноэкранный оверлей (входящее/настройки)
        val chipScrim = Color(0xB8141518) // chrome-чипы (имя/трафик/камера/⚙)
        val barScrim = Color(0xBC141518) // нижняя панель управления
        val tileScrim = Color(0xAA000000) // плашка имени на тайле
        val scrimStrong = Color(0xCC181A1F) // кнопка фронт/тыл на превью

        // ── Поверх видео ──────────────────────────────────────────────────────
        // Полупрозрачный БЕЛЫЙ, а не наш серый: под этими плашками кадр, и любой
        // непрозрачный цвет читался бы дырой в картинке.
        val glass = Color(0x33FFFFFF) // кнопка/чип поверх кадра
        val glassDim = Color(0x22FFFFFF) // то же, но для второстепенного
        val glassDanger = Color(0x33E53935) // «микрофон выключен» и подобное
        val tileBorder = Color(0xFF4A4F55) // обводка тайла участника
        val tileIdle = Color(0xFF2A2F35) // тайл участника без видео

        // Группы и комнаты
        val groupAvatar = Color(0xFF0D2A0D) // фон аватара группы/комнаты
        val roomBanner = Color(0xFF12331A) // баннер «в комнате идёт звонок»
        val destructive = error // «удалить участника», «выйти из группы» — тот же красный текста

        // Полоса очереди над списком главной
        val queueStrip = Color(0xFF2A2418)
        val queueText = Color(0xFFF2E3C8)
        val queueSub = Color(0xFFBFAE90)

        // Предупреждения
        val noticeBg = Color(0xFF3A2F12) // плашка-предупреждение (очередь разрослась)
        val noticeText = Color(0xFFF6C453) // текст и знак на ней
        val onboardWarnText = Color(0xFFE8C98A) // текст предупреждения в онбординге

        // Прочее
        val iconBar = Color(0xFFD6D6DD) // иконки верхней панели
        val iconPlaceholder = Color(0xFF9AA0A6) // заглушка своего аватара
        val unreadMutedText = Color(0xFFB9B9C2) // цифра на счётчике чата без звука
        val scanScrim = Color(0x99000000) // затемнение вокруг рамки сканера QR
        val kindVideo = Color(0xFF7E57C2) // видео в разбивке «Хранилища»
        val trackIdle = Color(0xFF2A2D31) // неактивная дорожка слайдера, погасшая ячейка индикатора

        // Палитры: цвет выбирается детерминированно по хэшу, поэтому у одного
        // человека он одинаков на всех экранах (и у отпечатка — на всех устройствах).
        /** Имя отправителя в группе (Discord-style). */
        val senderNames = listOf(
            Color(0xFF5BC0EB),
            Color(0xFF9BC53D),
            Color(0xFFE5793A),
            Color(0xFFEF6FB3),
            Color(0xFF7C5CFC),
            Color(0xFF20C997),
            Color(0xFFF6C453),
        )

        /** Фон аватара-инициалов контакта. */
        val avatarPalette = listOf(
            Color(0xFF1E3A5A),
            Color(0xFF2A1A3A),
            Color(0xFF1A2A1A),
            Color(0xFF2A2A1A),
            Color(0xFF3A1A1A),
            Color(0xFF1A3A3A),
        )

        /** Знаки визуального отпечатка: различимые оттенки на тёмном фоне. Порядок
         *  — часть сверки: поменять его значит, что старые скриншоты отпечатка
         *  перестанут совпадать. */
        val fingerprintGlyphs = listOf(
            Color(0xFF4FC3F7), Color(0xFF4DB6AC), Color(0xFF81C784), Color(0xFFAED581),
            Color(0xFFFFD54F), Color(0xFFFFB74D), Color(0xFFFF8A65), Color(0xFFE57373),
            Color(0xFFF06292), Color(0xFFBA68C8), Color(0xFF9575CD), Color(0xFF7986CB),
            Color(0xFF90A4AE), Color(0xFFA1887F), Color(0xFFDCE775), Color(0xFFFFFFFF),
        )
    }

    /**
     * Шкала отступов — сетка 4dp (+ хайрлайн 2). Новый код берёт ОТСЮДА; раскладку
     * собирать из этих шагов, а не из произвольных `.dp`. Маппинг старых off-grid
     * значений (6/9/10/14/18/22 → ближайший шаг) — в `docs/design/tokens.md`.
     */
    object Space {
        val xxs = 2.dp // хайрлайн: бейджи, тонкие зазоры
        val xs = 4.dp
        val s = 8.dp
        val m = 12.dp
        val l = 16.dp
        val xl = 20.dp
        val xxl = 24.dp
        val xxxl = 32.dp // поля «пустого» экрана-заглушки
        val hero = 48.dp // ритм экранов-иллюстраций (онбординг, замок, «готово»): отступ от края до сюжета

        // Семантические (используются Theme.kt / списками).
        val screen = l // 16 — отступ контента экрана
        val panel = 10.dp // от края панели до групп (концентричность радиусов 28/18)
        val panelText = l // от края панели до подписи над группой: по вертикали с текстом в группе
        val formGap = m // 12 — зазор в формах
        val rowV = 9.dp // намеренная плотность ряда списка (Telegram-density), вне сетки
        val series = 3.dp // зазор между сообщениями ОДНОЙ серии (между сериями — s)

        // Поля пузыря — плотность, как rowV: 12×8 по сетке делали короткое сообщение
        // заметно толще, чем в любом мессенджере, к которому привыкла рука.
        val bubbleH = 10.dp
        val bubbleV = 6.dp
    }

    /**
     * ЗАКРЫТАЯ шкала скруглений: пять ступеней плюс круг, и ничего между ними.
     *
     * Разъехалась она быстро: к десяти именам добавились семь десятков инлайновых
     * `RoundedCornerShape(N.dp)` пятнадцати величин — 2, 3, 5, 6, 7, 8, 9, 10, 11,
     * 12, 14, 15, 16, 18, 26. Разница в 1-2dp не читается как намерение, она
     * читается как небрежность, поэтому промежуточные ступени схлопнуты:
     *   5-6 → badge · 7-11 → chip · 12-16 → button · 17-18 → pill · 20-26 → card.
     *
     * Несколько имён на одно значение — это НЕ дубли, а точки будущего расхождения:
     * поедут кнопки — изменится `button`, не тронув превью и тайлы.
     *
     * Исключение — тонкие риски и «ручки» (ширина 2-4dp): там скругление равно
     * половине толщины, это геометрия элемента, а не ступень шкалы.
     */
    object Radius {
        val badge = 6.dp // бейджи, счётчики, срез стыка серии
        val bubbleTail = 6.dp // срезанный угол на СТЫКЕ сообщений одной серии
        val chip = 10.dp // чипы, ярлыки даты, плашка цитаты
        val tile = 14.dp // тайлы звонка
        val preview = 14.dp // область превью шаринга
        val button = 14.dp // кнопки, поле ввода, иконка-плашка
        val avatar = 14.dp // «квадрат со скруглением» — кэп пропорции (см. SquareAvatar)
        val bubble = 18.dp // пузырь сообщения (внешние углы серии)
        val pill = 18.dp // пилюли панели управления
        val field = 16.dp // поле ввода, поиск
        val group = 18.dp // группа строк на панели: 28 = 18 + отступ панели 10 (концентрично)
        val card = group // старое имя карточки-секции
        val panel = 28.dp // верх панели «листа», шторки
        val inner = 8.dp // плитка/иконка внутри группы
        val thumb = 4.dp // миниатюра в сетке медиа
    }

    /**
     * Длительности анимаций (мс). Коротко и без пружин: это мессенджер, а не витрина —
     * движение здесь объясняет, ОТКУДА взялся элемент, и не задерживает руку.
     */
    object Anim {
        val fast = 140 // смена иконки/цвета: должно читаться как мгновенное
        val normal = 200 // выезд панели, появление пузыря
    }

    /** Размеры компонентов (dp). */
    object Size {
        val chromeChip = 34.dp // верхние чипы (имя/трафик/камера/⚙) — единая высота
        val ctlButtonW = 60.dp // кнопка нижней панели звонка
        val ctlButtonH = 48.dp
        val callButton = 56.dp // сквиркул отклонить/принять/пресет
        val avatarBig = 88.dp

        // ЗАКРЫТАЯ шкала иконок, как у шрифтов: литерал `Modifier.size(N.dp)` у Icon —
        // хардкод. Было 20 разных размеров от 11 до 56 dp.
        val iconXs = 14.dp // мета и статусы: галочки, «мик выкл» на плитке, стрелки направлений
        val iconS = 18.dp // рядом с текстом: шевроны, иконки в пилюлях и чипах
        val iconRow = 20.dp // иконка в плитке строки группы и в кнопке шапки (контур 1.5)
        val iconM = 22.dp // строка карточки
        val iconL = 24.dp // кнопки звонка, крупные действия
        val iconXl = 32.dp // знак внутри круга-кнопки (play в пузыре), иконка шага онбординга
        val iconHero = 48.dp // иллюстрация экрана: пустое состояние, замок, «готово»

        // ── Список чатов / чат ────────────────────────────────────────────────
        val chatRow = 72.dp // МИНИМАЛЬНАЯ высота строки списка чатов (крупный шрифт её растит)
        val avatarRow = 48.dp // аватар в строке списка
        val avatarTop = 40.dp // аватар-вход в «Профиль» в шапке главной
        val avatarProfile = 64.dp // аватар в карточке «Профиля»
        val rowIcon = 36.dp // плитка иконки в строке группы
        val navRow = 56.dp // строка-раздел в группе «Профиля» и категорий
        val avatarHead = 38.dp // аватар в шапке чата
        val avatarSeries = 26.dp // аватар у последнего сообщения серии
        val tabLine = 2.dp // линия акцента под выбранной папкой
        val badge = 22.dp // бейдж-счётчик: высота и минимальная ширина
        val barButton = 38.dp // кнопка-плашка шапки (BarIcon) и индикатор пиров
        val topBar = 56.dp // шапка экрана и панель выделения
        val cardRow = 52.dp // минимальная высота строки в карточке; от него же отступ разделителя

        // Риска «это ссылка на сообщение» у цитаты, закрепа, полоски ответа.
        val rule = 30.dp
        val ruleWidth = 3.dp
        val inputField = 42.dp // поле ввода и кнопка отправки
        val metaSlot = 46.dp // ширина правой колонки строки (время + значок)

        val qr = 220.dp // QR-код своего кода обмена
        val fabClearance = 88.dp // хвост списка под плавающей кнопкой: последняя строка не прячется под ней

        // ── Пузырь ────────────────────────────────────────────────────────────
        val bubbleMax = 290.dp // предельная ширина пузыря
        val photoBubble = 240.dp // снимок, который сам себе пузырь (и шапка над ним)
        val photoInBubble = 220.dp // снимок внутри пузыря с подписью
        val metaReserve = 44.dp // место справа у вложения под время в углу пузыря
        val handleW = 36.dp // ручка шторки
        val handleH = 4.dp
        val primaryButton = 52.dp // главная кнопка экрана-решения, во всю ширину
        val touch = 48.dp // минимальная зона касания (Android): плашка может быть меньше, зона — нет
    }

    /**
     * ЗАКРЫТАЯ шкала шрифтов (app-ui.md → «Шрифт»): 11 время · 12 мета · 13 подписи · 14 превью ·
     * 15 сообщения · 16 имена и строки · 20 шторка · 22 экран · 24 профиль; 26/28 — звонок, крупные
     * числа и заголовок экрана-шага. Литерал `N.sp` в экране — хардкод; межстрочный интервал
     * задаёт тема (см. [Text.base]).
     */
    object Font {
        val caption = 11.sp // подписи, caps-метки секций, время, счётчики
        val small = 12.sp // мелкие подписи, бейджи
        val body = 13.sp // вторичный текст, чипы, имена в тайлах
        val preview = 14.sp // превью последнего сообщения в списке
        val label = 15.sp // основной текст: сообщения, поля ввода, кнопки
        val name = 16.sp // имена в списке, строки групп
        val sheet = 20.sp // заголовок шторки
        val title = 22.sp // заголовок экрана
        val profile = 24.sp // имя в профиле
        val big = 26.sp // имя во входящем звонке, эмодзи в панели
        val huge = 28.sp // крупные числа (трафик, обратный отсчёт)

        /** Межстрочный интервал — доля размера, а не sp: тогда он верен для любой ступени. */
        val lineHeight = 1.3.em

        /** Для длинного чтения и ввода фразы восстановления. */
        val lineHeightLoose = 1.5.em

        /** Буква-заглушка в аватаре: 40% его стороны при любом размере аватара. */
        fun initial(avatar: Dp) = (avatar.value * 0.4f).sp
    }

    /**
     * Составные стили текста. [base] тема подкладывает под ЛЮБОЙ `Text` (см. Theme.kt);
     * остальные — для ролей, где кроме размера важно что-то ещё.
     */
    object Text {
        /**
         * Пропорциональный интервал и строка, отцентрованная по высоте. Без этого Material
         * подкладывал bodyLarge: 24sp интервала при любом размере и разрядку 0.5sp, и мелкий
         * текст в бейдже съезжал с центра.
         */
        val base = TextStyle(
            fontFamily = Golos,
            fontSize = Font.label,
            lineHeight = Font.lineHeight,
            letterSpacing = 0.sp,
            lineHeightStyle = LineHeightStyle(LineHeightStyle.Alignment.Center, LineHeightStyle.Trim.None),
        )

        /** Время, «изм.», счётчики: цифры одной ширины, чтобы колонка не дрожала. */
        val meta = base.copy(fontSize = Font.caption, fontFeatureSettings = "tnum")
    }
}
