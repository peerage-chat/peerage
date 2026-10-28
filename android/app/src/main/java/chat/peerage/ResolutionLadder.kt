package chat.peerage

/**
 * Ступени разрешения видео под оценку полосы (ADR-014, шаг 7b). Битрейт кодер меняет
 * на лету, но на малом битрейте большой кадр превращается в кашу из блоков —
 * честнее уменьшить кадр. Смена разрешения дороже (новый кодер, ключевой кадр,
 * мгновение без картинки), поэтому с гистерезисом: вниз — когда цели не хватает
 * пару секунд, вверх — когда её с запасом хватает дольше; между сменами — пауза.
 *
 * Чистая логика без Android: [steps] — размеры по убыванию, первый — выбранный в
 * настройках (потолок); [update] зовут раз в ~секунду с целью из оценки полосы.
 */
class ResolutionLadder(
    val steps: List<Size>,
    private val fps: Int,
    /** Потолок битрейта из настроек: выше цель не поднимется, и пороги не выше него —
     *  иначе на «низком» битрейте в настройках к выбранному разрешению не вернуться. */
    private val maxBitrate: Int,
) {
    data class Size(val width: Int, val height: Int) {
        val pixels: Long get() = width.toLong() * height
    }

    /** Текущая ступень (индекс в [steps]). */
    var current = 0
        private set

    private var lastSwitchMs: Long? = null
    private var belowSinceMs: Long? = null
    private var aboveSinceMs: Long? = null

    /** С какой цели (бит/с) ступени [i] достаточно, чтобы на неё подняться. */
    fun upBitrate(i: Int): Int = (startBitrate(steps[i].pixels) * fpsFactor(fps))
        .toInt()
        .coerceAtMost(maxBitrate * 9 / 10)

    /** Ниже этой цели ступень [i] уже не тянет — уходим вниз. */
    fun downBitrate(i: Int): Int = upBitrate(i) * 6 / 10

    /** Новая цель в момент [nowMs]; возвращает ступень (может не измениться). */
    fun update(targetBps: Int, nowMs: Long): Int {
        if (targetBps <= 0 || steps.size < 2) return current
        val settled = lastSwitchMs.let { it == null || nowMs - it >= MIN_SWITCH_GAP_MS }

        if (current < steps.lastIndex && targetBps < downBitrate(current)) {
            val since = belowSinceMs ?: nowMs.also { belowSinceMs = it }
            if (settled && nowMs - since >= DOWN_HOLD_MS) {
                // Сразу на ступень, которой цели хватает с запасом, — не по одной с
                // паузами, пока картинка рассыпается.
                val to = (current + 1..steps.lastIndex).firstOrNull { upBitrate(it) <= targetBps }
                    ?: steps.lastIndex
                switch(to, nowMs)
            }
        } else {
            belowSinceMs = null
        }

        if (current > 0 && targetBps >= upBitrate(current - 1)) {
            val since = aboveSinceMs ?: nowMs.also { aboveSinceMs = it }
            if (settled && nowMs - since >= UP_HOLD_MS) switch(current - 1, nowMs)
        } else {
            aboveSinceMs = null
        }
        return current
    }

    private fun switch(to: Int, nowMs: Long) {
        current = to
        lastSwitchMs = nowMs
        belowSinceMs = null
        aboveSinceMs = null
    }

    companion object {
        const val DOWN_HOLD_MS = 2_000L
        const val UP_HOLD_MS = 8_000L
        const val MIN_SWITCH_GAP_MS = 5_000L

        /** Пиксели кадра → битрейт (30 fps), с которого размер выглядит прилично. По
         *  таблице стартовых битрейтов WebRTC (270p 300к · 360p 500к · 540p 800к ·
         *  720p 1,5М), выше — по той же плотности. Между точками — линейно. */
        private val table = listOf(
            0L to 0.0,
            480L * 270 to 300_000.0,
            640L * 360 to 500_000.0,
            960L * 540 to 800_000.0,
            1280L * 720 to 1_500_000.0,
            1920L * 1080 to 2_500_000.0,
            3840L * 2160 to 8_000_000.0,
        )

        fun startBitrate(pixels: Long): Double {
            val hi = table.indexOfFirst { it.first >= pixels }
            if (hi < 0) {
                val (p, b) = table.last()
                return b * pixels / p
            }
            if (hi == 0) return 0.0
            val (p0, b0) = table[hi - 1]
            val (p1, b1) = table[hi]
            return b0 + (b1 - b0) * (pixels - p0) / (p1 - p0)
        }

        /** 60 fps не вдвое дороже 30: соседние кадры похожи. */
        private fun fpsFactor(fps: Int): Double = 0.5 + 0.5 * fps.coerceIn(10, 120) / 30.0

        /**
         * Ступени из размеров, которые умеет камера: выбранный [top] и меньшие того
         * же соотношения сторон, каждая не больше ~60 % площади предыдущей (мельче
         * шаг — лишние переключения без заметной разницы), не ниже 144 строк.
         */
        fun stepsFrom(top: Size, available: List<Size>): List<Size> {
            val aspect = top.width.toDouble() / top.height
            val candidates = available
                .filter { kotlin.math.abs(it.width.toDouble() / it.height - aspect) / aspect < 0.02 }
                .filter { it.pixels < top.pixels && it.height >= MIN_HEIGHT }
                .distinct()
                .sortedByDescending { it.pixels }
            val steps = mutableListOf(top)
            for (s in candidates) {
                if (s.pixels * 10 <= steps.last().pixels * 6) steps += s
            }
            return steps
        }

        private const val MIN_HEIGHT = 144
    }
}
