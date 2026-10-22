package dev.p2p.messenger.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import dev.p2p.messenger.*
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.sin

/**
 * Визуальный отпечаток ключа: 12 знаков (сетка 6×2), 96 бит.
 *
 * **Почему свой набор, а не эмодзи из шрифта.** Сверка идёт ГЛАЗАМИ по двум экранам:
 * человек сравнивает картинку у себя с картинкой у собеседника. Системные эмодзи
 * рисует шрифт устройства — на разных вендорах и версиях Android один и тот же символ
 * выглядит по-разному, и сверка перестаёт работать ровно там, где она нужнее всего.
 * Поэтому знаки рисует само приложение: одинаковые пиксели на любом устройстве, без
 * зависимости от шрифта.
 *
 * **Из чего знак.** Байт → форма (младшие 4 бита) + цвет (старшие 4). 16×16 = 256
 * вариантов, то есть ровно 8 бит на знак и 96 бит на все 12. Форма — основной
 * различитель (её видно и в чёрно-белом), цвет — вторичный.
 *
 * Само правило вывода байтов живёт в ядре (`p2p_core::identity::visual_fingerprint`)
 * и версионировано: сменим набор — бампнём версию, и старые сверки честно
 * инвалидируются, а не начнут молча расходиться.
 */

/** Палитра знаков — [Tokens.Colors.fingerprintGlyphs]. */
private val GlyphColors = Tokens.Colors.fingerprintGlyphs

/** Разобрать hex-отпечаток движка в байты. Битый вход — пустой список. */
internal fun parseFingerprint(hex: String): List<Int> {
    val clean = hex.trim()
    if (clean.length % 2 != 0) return emptyList()
    return runCatching {
        clean.chunked(2).map { it.toInt(16) }
    }.getOrDefault(emptyList())
}

/** Сетка 6×2 знаков. */
@Composable
internal fun FingerprintGrid(hex: String, modifier: Modifier = Modifier) {
    val bytes = parseFingerprint(hex)
    if (bytes.isEmpty()) return
    Column(modifier, verticalArrangement = Arrangement.spacedBy(Tokens.Space.s)) {
        bytes.chunked(6).forEach { row ->
            Row(horizontalArrangement = Arrangement.spacedBy(Tokens.Space.s)) {
                row.forEach { b -> GlyphCell(b, Modifier.weight(1f)) }
                repeat(6 - row.size) { Spacer(Modifier.weight(1f)) }
            }
        }
    }
}

@Composable
private fun GlyphCell(byte: Int, modifier: Modifier = Modifier) {
    Box(
        modifier.aspectRatio(1f).clip(RoundedCornerShape(Tokens.Radius.chip)).background(HoloBg),
        contentAlignment = Alignment.Center,
    ) {
        Canvas(Modifier.fillMaxSize(0.62f)) { drawGlyph(byte) }
    }
}

/** Нарисовать знак: форма из младших 4 бит, цвет — из старших. */
private fun DrawScope.drawGlyph(byte: Int) {
    val color = GlyphColors[(byte shr 4) and 0x0F]
    val w = size.minDimension
    val c = Offset(size.width / 2, size.height / 2)
    val r = w / 2
    val stroke = Stroke(width = w * 0.16f)
    when (byte and 0x0F) {
        0 -> drawCircle(color, r, c)
        1 -> drawCircle(color, r, c, style = stroke) // кольцо
        2 -> drawRect(color, Offset(c.x - r, c.y - r), Size(w, w))
        3 -> drawRect(color, Offset(c.x - r, c.y - r), Size(w, w), style = stroke)
        4 -> drawPath(polygon(c, r, 3, -90f), color) // треугольник вверх
        5 -> drawPath(polygon(c, r, 3, 90f), color) // треугольник вниз
        6 -> drawPath(polygon(c, r, 4, -90f), color) // ромб
        7 -> drawPath(polygon(c, r, 5, -90f), color)
        8 -> drawPath(polygon(c, r, 6, -90f), color)
        9 -> drawPath(star(c, r, 5), color)
        10 -> drawPath(star(c, r, 6), color)
        11 -> drawPath(cross(c, r, 0f), color)
        12 -> drawPath(cross(c, r, 45f), color)
        13 -> drawArc( // разомкнутое кольцо
            color = color,
            startAngle = 40f,
            sweepAngle = 280f,
            useCenter = false,
            topLeft = Offset(c.x - r * 0.85f, c.y - r * 0.85f),
            size = Size(r * 1.7f, r * 1.7f),
            style = stroke,
        )
        14 -> { // капля: круг + треугольник сверху
            drawCircle(color, r * 0.72f, Offset(c.x, c.y + r * 0.22f))
            drawPath(polygon(Offset(c.x, c.y - r * 0.28f), r * 0.62f, 3, -90f), color)
        }
        else -> { // мишень: кольцо + точка
            drawCircle(color, r, c, style = Stroke(width = w * 0.13f))
            drawCircle(color, r * 0.34f, c)
        }
    }
}

/** Правильный многоугольник с [n] вершинами, повёрнутый на [rotationDeg]. */
private fun polygon(center: Offset, radius: Float, n: Int, rotationDeg: Float): Path {
    val p = Path()
    for (i in 0 until n) {
        val a = (rotationDeg + i * 360f / n) * PI.toFloat() / 180f
        val x = center.x + radius * cos(a)
        val y = center.y + radius * sin(a)
        if (i == 0) p.moveTo(x, y) else p.lineTo(x, y)
    }
    p.close()
    return p
}

/** Звезда с [points] лучами (внутренний радиус — 0.45 внешнего). */
private fun star(center: Offset, radius: Float, points: Int): Path {
    val p = Path()
    val steps = points * 2
    for (i in 0 until steps) {
        val rr = if (i % 2 == 0) radius else radius * 0.45f
        val a = (-90f + i * 360f / steps) * PI.toFloat() / 180f
        val x = center.x + rr * cos(a)
        val y = center.y + rr * sin(a)
        if (i == 0) p.moveTo(x, y) else p.lineTo(x, y)
    }
    p.close()
    return p
}

/** Крест («плюс» или «икс» при повороте на 45°). */
private fun cross(center: Offset, radius: Float, rotationDeg: Float): Path {
    val arm = radius
    val half = radius * 0.30f
    val pts = listOf(
        Offset(-half, -arm), Offset(half, -arm), Offset(half, -half), Offset(arm, -half),
        Offset(arm, half), Offset(half, half), Offset(half, arm), Offset(-half, arm),
        Offset(-half, half), Offset(-arm, half), Offset(-arm, -half), Offset(-half, -half),
    )
    val rad = rotationDeg * PI.toFloat() / 180f
    val p = Path()
    pts.forEachIndexed { i, o ->
        val x = center.x + o.x * cos(rad) - o.y * sin(rad)
        val y = center.y + o.x * sin(rad) + o.y * cos(rad)
        if (i == 0) p.moveTo(x, y) else p.lineTo(x, y)
    }
    p.close()
    return p
}
