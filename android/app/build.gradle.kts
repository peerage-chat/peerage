plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
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
    namespace = "dev.p2p.messenger"
    compileSdk = 34

    defaultConfig {
        applicationId = "dev.p2p.messenger"
        minSdk = 24
        targetSdk = 34
        versionCode = 1
        versionName = "0.1.0"
        // arm64 — реальные устройства; x86_64 — эмулятор.
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
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

    // Нативные .so (libygg.so + libp2p_android.so) стейджатся скриптом
    // scripts/build-android-libs.sh в этот каталог по ABI.
    sourceSets["main"].jniLibs.srcDirs("src/main/jniLibs")

    packaging {
        // Go c-shared libygg.so не выровнена под uncompressed-загрузку — извлекаем.
        jniLibs.useLegacyPackaging = true
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
    implementation("androidx.lifecycle:lifecycle-runtime-ktx:2.8.4")
    // QR-генерация (MeScreen) — ZXing core.
    implementation("com.google.zxing:core:3.5.3")
    // QR-сканер (AddScreen) — CameraX + ZXing core, собственный Compose UI.
    implementation("androidx.camera:camera-camera2:1.3.4")
    implementation("androidx.camera:camera-lifecycle:1.3.4")
    implementation("androidx.camera:camera-view:1.3.4")
    debugImplementation("androidx.compose.ui:ui-tooling")
}
