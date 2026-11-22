package chat.peerage.ui

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Дальние углы пузыря у стыков серии (макет `bubbles/Adaptive`, UI 5.8): угол срезан, если
 * сосед по серии НЕ УЖЕ — шире или ровно такой же, без допуска «почти вровень» (правка юзера
 * 2026-11-22). Сосед уже — угол полный: под ним пустое место, и срез висел бы в воздухе.
 */
class BubbleCornersTest {
    @Test fun wider_neighbour_cuts_the_far_corner() {
        assertEquals(FarCut(top = true, bottom = false), farCut(own = 100, above = 140, below = null))
        assertEquals(FarCut(top = false, bottom = true), farCut(own = 100, above = null, below = 101))
    }

    @Test fun equal_width_cuts_too() {
        assertEquals(FarCut(top = true, bottom = true), farCut(own = 100, above = 100, below = 100))
    }

    @Test fun narrower_neighbour_leaves_it_round_even_by_a_pixel() {
        assertEquals(FarCut.None, farCut(own = 100, above = 99, below = 60))
    }

    @Test fun unknown_width_leaves_it_round() {
        // Ширина ещё не измерена (первый кадр, сосед за краем экрана) — полный угол.
        assertEquals(FarCut.None, farCut(own = null, above = 140, below = 140))
        assertEquals(FarCut.None, farCut(own = 100, above = null, below = null))
    }
}
