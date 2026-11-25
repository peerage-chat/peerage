package chat.peerage

import kotlin.math.roundToInt

/**
 * Быстрая прокрутка галереи в панели вложений (п. 70): доля высоты дорожки ↔ номер снимка.
 * Дорожка — вся галерея, а не загруженная её часть: число снимков известно заранее.
 */
object FastScroll {
    /** Снимок под долей [f] (0 — верх, 1 — низ) при [total] снимках. */
    fun indexAt(f: Float, total: Int): Int =
        if (total <= 0) 0 else (f.coerceIn(0f, 1f) * (total - 1)).roundToInt()

    /** Где на дорожке ползунок, когда сверху снимок [index]. */
    fun fractionOf(index: Int, total: Int): Float =
        if (total <= 1) 0f else (index.toFloat() / (total - 1)).coerceIn(0f, 1f)
}
