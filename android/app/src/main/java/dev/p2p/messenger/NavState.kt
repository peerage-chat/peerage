package dev.p2p.messenger

import androidx.compose.runtime.mutableStateListOf
import kotlin.reflect.KProperty

/**
 * Стек экранов. Делегат для `var screen by nav`: `screen = X` — переход вперёд ([go]).
 *
 * «Назад» ([back]) всегда ведёт туда, откуда пришли, — и системная кнопка, и стрелка
 * в шапке зовут одну эту функцию. Раньше родитель экрана был зашит таблицей, а стрелки
 * в шапках жили отдельно от неё: экран, открытый из нескольких мест («Добавить
 * контакт» — из «Нового чата» и из «Контактов»), возвращал на главную, а стрелка и
 * системная кнопка вели в разные места.
 *
 * Правила:
 * - главная ([root]) — дно стека; переход на неё начинает историю заново;
 * - переход на экран, который уже есть в стеке, — возврат к нему, а не новый слой:
 *   «Написать» из профиля возвращает в тот же чат, и циклов в истории не бывает;
 * - [replace] — переход, после которого вернуться на текущий экран незачем: выбрали
 *   собеседника в «Новом чате» — «назад» из чата ведёт на главную, а не в выбор.
 */
internal class NavState(initial: Screen, private val root: Screen = Screen.Contacts) {
    private val stack = mutableStateListOf(root).apply { if (initial != root) add(initial) }

    val current: Screen get() = stack.last()

    /** Глубина истории — для тестов и для решения «гасить ли системную назад». */
    val depth: Int get() = stack.size

    operator fun getValue(thisRef: Any?, property: KProperty<*>): Screen = current

    operator fun setValue(thisRef: Any?, property: KProperty<*>, value: Screen) = go(value)

    fun go(dest: Screen) {
        val i = stack.indexOf(dest)
        if (i >= 0) {
            while (stack.size > i + 1) stack.removeAt(stack.lastIndex)
        } else {
            stack.add(dest)
        }
    }

    /** Шаг назад. На главной не делает ничего: выход из приложения решает система. */
    fun back() {
        if (stack.size > 1) stack.removeAt(stack.lastIndex)
    }

    /** Заменить текущий экран на [dest]. */
    fun replace(dest: Screen) {
        back()
        go(dest)
    }

    /** Уйти со всех экранов из [gone] (например, нас исключили из открытой группы). */
    fun leave(gone: Set<Screen>) {
        while (stack.size > 1 && current in gone) stack.removeAt(stack.lastIndex)
    }
}
