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
        val bg = Color(0xFF1A1A1A) // фон обычных экранов
        val surface = Color(0xFF222222)
        val border = Color(0xFF333333)
        val callBg = Color(0xFF181A1F) // фон экрана звонка/шаринга (темнее тайлов → слоистость)
        val tile = Color(0xFF1A1D21) // тайлы звонка, поля ввода, невыбранные чипы
        val avatar = Color(0xFF3A3F45) // фон аватара-заглушки

        // ── Текст/иконки ──────────────────────────────────────────────────────
        val onDark = Color.White
        val secondary = Color(0xFF888888)
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
        val bubbleIn = Color(0xFF24292F)
        val bubbleOut = Color(0xFF245266)
        val folderSelected = Color(0xFF1C3A47)

        // ── Скримы/оверлеи ────────────────────────────────────────────────────
        val overlay = Color(0xF2000000) // полноэкранный оверлей (входящее/настройки)
        val chipScrim = Color(0xB8141518) // chrome-чипы (имя/трафик/камера/⚙)
        val barScrim = Color(0xBC141518) // нижняя панель управления
        val tileScrim = Color(0xAA000000) // плашка имени на тайле
        val scrimStrong = Color(0xCC181A1F) // кнопка фронт/тыл на превью
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
    }

    /** Радиусы скруглений (dp). */
    object Radius {
        val badge = 6.dp
        val chip = 10.dp
        val tile = 12.dp
        val preview = 14.dp
        val button = 14.dp
        val pill = 18.dp
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
