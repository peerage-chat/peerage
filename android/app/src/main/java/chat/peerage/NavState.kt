package chat.peerage

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
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

    /** Последний шаг был вглубь (новый экран) или назад (вернулись к бывшему) — для
     *  направления перехода, когда прежнего экрана в истории уже нет. */
    var lastForward by mutableStateOf(true)
        private set

    /** Куда ведёт «назад»; null — на главной. Для предиктивного жеста: его тянут заранее. */
    val previous: Screen? get() = stack.getOrNull(stack.lastIndex - 1)

    /** Место экрана в истории; -1 — его там нет. По нему переход решает «вглубь» или «назад». */
    fun indexOf(s: Screen): Int = stack.indexOf(s)

    fun go(dest: Screen) {
        val i = stack.indexOf(dest)
        if (i >= 0) {
            while (stack.size > i + 1) stack.removeAt(stack.lastIndex)
        } else {
            stack.add(dest)
        }
        lastForward = i < 0
    }

    /** Шаг назад. На главной не делает ничего: выход из приложения решает система. */
    fun back() {
        if (stack.size > 1) stack.removeAt(stack.lastIndex)
        lastForward = false
    }

    /**
     * Начать историю заново с [dest] поверх главной. Две панели (от 720 dp): главная всегда
     * видна слева, и выбранное в ней открывается справа С ЧИСТОГО ЛИСТА — «назад» из чата,
     * выбранного в списке, не должна вести в прошлый чат и его профиль.
     */
    fun resetTo(dest: Screen) {
        while (stack.size > 1) stack.removeAt(stack.lastIndex)
        if (dest != root) stack.add(dest)
        lastForward = true
    }

    /** Заменить текущий экран на [dest]. */
    fun replace(dest: Screen) {
        back()
        go(dest)
    }

    /** Уйти со всех экранов из [gone] (например, нас исключили из открытой группы). */
    fun leave(gone: Set<Screen>) {
        while (stack.size > 1 && current in gone) stack.removeAt(stack.lastIndex)
        lastForward = false
    }
}
