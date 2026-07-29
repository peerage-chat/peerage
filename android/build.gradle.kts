// Корневой build-скрипт. Версии плагинов — известная совместимая связка
// (AGP 8.5 + Kotlin 1.9.25 + Compose compiler 1.5.15).
plugins {
    id("com.android.application") version "8.5.2" apply false
    id("org.jetbrains.kotlin.android") version "1.9.25" apply false
    // Схема снимков/событий движка описана типами (см. Dto.kt): разбор по строковым
    // ключам заменён на @Serializable, чтобы расхождение с Rust ловил компилятор.
    id("org.jetbrains.kotlin.plugin.serialization") version "1.9.25" apply false
    // Линтер/форматтер Kotlin. Задачи: ktlintCheck (проверка), ktlintFormat (автофикс).
    id("org.jlleitschuh.gradle.ktlint") version "12.1.1" apply false
}
