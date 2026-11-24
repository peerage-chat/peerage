// Корневой build-скрипт. Версии плагинов — совместимая связка AGP 9.5 + Kotlin 2.4.
// Kotlin в AGP 9 встроен (плагин kotlin-android не применяется); строка ниже лишь
// закрепляет версию Kotlin-плагина, иначе AGP взял бы свою минимальную.
plugins {
    id("com.android.application") version "9.5.2" apply false
    id("org.jetbrains.kotlin.android") version "2.4.21" apply false
    // Компилятор Compose — с Kotlin 2.0 плагин Kotlin той же версии (не composeOptions).
    id("org.jetbrains.kotlin.plugin.compose") version "2.4.21" apply false
    // Схема снимков/событий движка описана типами (см. Dto.kt): разбор по строковым
    // ключам заменён на @Serializable, чтобы расхождение с Rust ловил компилятор.
    id("org.jetbrains.kotlin.plugin.serialization") version "2.4.21" apply false
    // Линтер/форматтер Kotlin. Задачи: ktlintCheck (проверка), ktlintFormat (автофикс).
    id("org.jlleitschuh.gradle.ktlint") version "14.2.0" apply false
}
