plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.serialization")
    id("org.jlleitschuh.gradle.ktlint")
}

ktlint {
    version.set("1.2.1")
    android.set(true)
    // Не валим CI/сборку на каждый чих — отчёт читаемый, но check остаётся в `check`.
    ignoreFailures.set(false)
    reporters {
        reporter(org.jlleitschuh.gradle.ktlint.reporter.ReporterType.PLAIN)
    }
}

/** Версия приложения (semver без суффиксов): её же несёт тег релиза. */
val APP_VERSION = "0.2.0"

fun versionCodeOf(v: String): Int {
    val parts = v.split(".").map { it.toIntOrNull() ?: error("версия $v: только числа M.m.p") }
    require(parts.size == 3 && parts[1] < 100 && parts[2] < 100) { "версия $v: нужно M.m.p, m и p меньше 100" }
    return parts[0] * 10000 + parts[1] * 100 + parts[2]
}

android {
    namespace = "chat.peerage"
    compileSdk = 34

    defaultConfig {
        applicationId = "chat.peerage"
        minSdk = 24
        targetSdk = 34
        // Версия — одна точка: versionCode выводится из versionName (M.m.p → M·10000 + m·100 + p),
        // так новый релиз не может уйти с прежним versionCode. Перед релизом меняется
        // только APP_VERSION, тег — `v$APP_VERSION` (docs/release.md).
        versionName = APP_VERSION
        versionCode = versionCodeOf(APP_VERSION)
        // Набор ABI задаётся ниже через splits (arm64 — устройства, x86_64 — эмулятор);
        // abiFilters здесь конфликтует со splits, поэтому не используется.
    }

    buildTypes {
        release {
            // R8: без него Compose в релизе заметно медленнее — неоптимизированный dex, а
            // снимочные коллекции ART помечает «failed lock verification and will run slower»
            // (строка из лога с телефона, 0.2.0). Имена НЕ переименовываем (proguard-rules.pro:
            // -dontobfuscate): трассы падений из лога бета-тестеров читаются без mapping-файла.
            isMinifyEnabled = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }

    buildFeatures { compose = true }
    composeOptions { kotlinCompilerExtensionVersion = "1.5.15" }

    // JVM-юнит-тесты (чистая логика: displayName, парсинг контактов). org.json из
    // android.jar в юнит-тестах — заглушка; настоящий берётся из testImplementation ниже.
    testOptions { unitTests.isReturnDefaultValues = true }

    // Нативные .so (libygg.so + libpeerage_android.so) стейджатся скриптом
    // scripts/build-android-libs.sh в этот каталог по ABI.
    sourceSets["main"].jniLibs.srcDirs("src/main/jniLibs")

    packaging {
        // Нативка лежит в APK несжатой+выровненной (AGP zipalign) и грузится ПРЯМО из
        // APK — без распаковки-дубликата на диск. Проверено: и Go c-shared libygg.so,
        // и Rust libpeerage_android.so корректно мапятся (nativeloader: base.apk!/lib/…: ok).
        jniLibs.useLegacyPackaging = false
    }

    // Сплит по ABI: каждому устройству — своя одна архитектура (arm64 на телефон,
    // x86_64 на эмулятор). Иначе universal-APK несёт обе несжатыми и раздувается.
    // Вместе с useLegacyPackaging=false даёт минимальный размер на устройстве.
    splits {
        abi {
            isEnable = true
            reset()
            include("arm64-v8a", "x86_64")
            isUniversalApk = false
        }
    }
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2024.09.03"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-extended")
    implementation("androidx.activity:activity-compose:1.9.2")
    implementation("org.jetbrains.kotlinx:kotlinx-serialization-json:1.6.3")
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.4")
    // QR-генерация (MeScreen) — ZXing core.
    implementation("com.google.zxing:core:3.5.3")
    // QR-сканер (AddScreen) — CameraX + ZXing core, собственный Compose UI.
    implementation("androidx.camera:camera-camera2:1.3.4")
    implementation("androidx.camera:camera-lifecycle:1.3.4")
    implementation("androidx.camera:camera-view:1.3.4")
    debugImplementation("androidx.compose.ui:ui-tooling")

    // Юнит-тесты (чистая Kotlin-логика без Android-фреймворка).
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20240303")
}

// Хардкод стиля, который уже вычищен, не должен вернуться: размер шрифта, цвет, отступы и
// размеры иконок в экранах — только из Tokens (docs/design/tokens.md). Проверка до сборки,
// чтобы ловилось сразу, а не на ревью.
val checkTokens by tasks.registering {
    val src = fileTree("src/main/java") {
        include("**/*.kt")
        exclude("**/Tokens.kt")
    }
    inputs.files(src)
    doLast {
        // Литерал N.dp — хардкод, если это ОТСТУП (padding/Spacer/spacedBy) или РАЗМЕР
        // ИКОНКИ. 1.dp разрешён: хайрлайн бейджа, а не шаг сетки. Размеры компонентов
        // (аватар 52, круг 72) не ловим — у них своя геометрия, а не шкала.
        val dpN = """([2-9]|[1-9]\d+)(\.\d+)?\.dp"""
        val rules = listOf(
            Regex("""(?<![\w.])\d+(\.\d+)?\.sp\b""") to "размер шрифта — Tokens.Font",
            Regex("""Color\(0x""") to "цвет — Tokens.Colors",
            Regex("""padding\([^)]*\b$dpN""") to "отступ — Tokens.Space",
            Regex("""Spacer\(Modifier\.(width|height)\($dpN\)""") to "отступ — Tokens.Space",
            Regex("""spacedBy\($dpN\)""") to "отступ — Tokens.Space",
            Regex("""^\s*(horizontal|vertical|start|end|top|bottom) = $dpN""") to "отступ — Tokens.Space",
            Regex("""Icon\(.*Modifier\.size\($dpN\)""") to "размер иконки — Tokens.Size.icon*",
        )
        // Иконка, разнесённая по строкам: размер ищем в пределах её вызова.
        val iconSize = Regex("""Modifier\.size\($dpN\)""")
        val hits = src.files.flatMap { f ->
            val lines = f.readLines()
            val single = lines.withIndex().flatMap { (i, line) ->
                rules.filter { it.first.containsMatchIn(line) }
                    .map { "${f.relativeTo(projectDir)}:${i + 1}: ${it.second}: ${line.trim()}" }
            }
            val multi = lines.withIndex().filter { (_, l) -> l.trimEnd().endsWith("Icon(") }.flatMap { (i, _) ->
                lines.subList(i, minOf(i + 6, lines.size)).withIndex()
                    .filter { (_, l) -> iconSize.containsMatchIn(l) }
                    .map { (k, l) -> "${f.relativeTo(projectDir)}:${i + k + 1}: размер иконки — Tokens.Size.icon*: ${l.trim()}" }
            }
            single + multi
        }
        if (hits.isNotEmpty()) throw GradleException("Хардкод вместо токенов:\n" + hits.joinToString("\n"))
    }
}
tasks.named("preBuild") { dependsOn(checkTokens) }
