package dev.p2p.messenger

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
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
object Tokens {
    object Colors {
        // ── Бренд/акцент ──────────────────────────────────────────────────────
        val accent = Color(0xFF33B5E5)
        val accentDark = Color(0xFF04222E) // текст/иконка НА акценте
        val accentMuted = Color(0xFF13313D) // выбранный чип/таб (приглушённый акцент)
        val accentBadge = Color(0x2233B5E5) // фон бейджа (акцент @13%)

        // ── Поверхности ───────────────────────────────────────────────────────
        // Три слоя с ЗАМЕТНЫМ шагом: фон → карточка → элемент на карточке. Раньше
        // bg/surface различались на 4% яркости, и экран читался плоским.
        val bg = Color(0xFF0F0F11) // фон экрана (под карточками)
        val surface = Color(0xFF1A1A1D) // карточка-контейнер: список, панели
        val surfaceHi = Color(0xFF232327) // элемент НА карточке (кнопка, поле)
        val rowPinned = Color(0xFF1F1F23) // закреплённая строка списка — ступень над surface
        val divider = Color(0xFF26262B) // разделитель ВНУТРИ карточки
        val border = Color(0xFF333333)
        val callBg = Color(0xFF131418) // фон экрана звонка (темнее и тайлов, и фона приложения)
        val tile = Color(0xFF1A1D21) // тайлы звонка, поля ввода, невыбранные чипы
        val avatar = Color(0xFF3A3F45) // фон аватара-заглушки

        // ── Текст/иконки ──────────────────────────────────────────────────────
        val onDark = Color.White
        val secondary = Color(0xFF82828C) // вторичный текст: превью чата, подписи
        val tertiary = Color(0xFF6F6F78) // время, счётчики — тише вторичного
        val iconMuted = Color(0xFF5A5F66) // иконка-заглушка (превью)
        val iconChrome = Color(0xFFE6E7EA) // иконки chrome-чипов
        val onOnline = Color(0xFF0A1F0A) // текст на зелёном (мини-аватар)

        // ── Статусы ───────────────────────────────────────────────────────────
        val online = Color(0xFF8BC34A)
        val verified = Color(0xFF4CAF50)
        val error = Color(0xFFE53935)
        val warn = Color(0xFFE6A23C)
        val offlineDot = Color(0xFF555555)

        // ── Кнопки звонка ─────────────────────────────────────────────────────
        val btnNeutral = Color(0xFF3A3D44) // mic on / invite / camera
        val btnDangerMuted = Color(0xFF3A2A2C) // выключенное (mic off / video off)
        val iconDangerMuted = Color(0xFFFF8A8A) // иконка выключенного

        // ── Чат ───────────────────────────────────────────────────────────────
        val bubbleIn = Color(0xFF1E1E22) // входящий — тот же слой, что карточки списка
        val bubbleOut = Color(0xFF1F4657) // исходящий — приглушённый акцент
        val metaOnOut = Color(0xFF9FC4D4) // время/«изм.» на исходящем пузыре
        val tickOnOut = Color(0xFF8FD0EA) // галочки доставки
        val labelScrim = Color(0x0FFFFFFF) // подложка ярлыка даты/системного события
        val folderSelected = Color(0xFF1C3A47)
        val unreadMuted = Color(0xFF3A3A42) // счётчик чата без звука

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
        val destructive = Color(0xFFE0563B) // «удалить участника», «выйти из группы»

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

        // Семантические (используются Theme.kt / списками).
        val screen = l // 16 — отступ контента экрана
        val formGap = m // 12 — зазор в формах
        val rowV = 9.dp // намеренная плотность ряда списка (Telegram-density), вне сетки
        val series = 3.dp // зазор между сообщениями ОДНОЙ серии (между сериями — s)
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
        val card = 20.dp // карточка-контейнер (список чатов, секции, панели)
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
        val iconS = 18.dp
        val iconM = 22.dp
        val iconL = 25.dp

        // ── Список чатов / чат ────────────────────────────────────────────────
        val chatRow = 66.dp // высота строки списка чатов
        val avatarRow = 46.dp // аватар в строке списка
        val avatarHead = 38.dp // аватар в шапке чата
        val avatarSeries = 26.dp // аватар у последнего сообщения серии
        val tabBar = 40.dp // высота полосы табов-папок
        val inputField = 42.dp // поле ввода и кнопка отправки
        val metaSlot = 46.dp // ширина правой колонки строки (время + значок)
    }

    /** Размеры шрифта (sp). */
    object Font {
        val caption = 11.sp
        val small = 12.sp
        val body = 13.sp
        val label = 15.sp
        val title = 19.sp
        val big = 26.sp
        val huge = 28.sp
    }
}
