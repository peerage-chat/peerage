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

android {
    namespace = "chat.peerage"
    compileSdk = 34

    defaultConfig {
        applicationId = "chat.peerage"
        minSdk = 24
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
        // Набор ABI задаётся ниже через splits (arm64 — устройства, x86_64 — эмулятор);
        // abiFilters здесь конфликтует со splits, поэтому не используется.
    }

    buildTypes {
        release {
            isMinifyEnabled = false
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
    implementation(platform("androidx.compose:compose-bom:2024.06.00"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.compose.material3:material3")
    // Классы размера окна (адаптив, ADR-005) — читаются только в MainActivity.
    implementation("androidx.compose.material3:material3-window-size-class")
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
