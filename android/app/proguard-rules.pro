# Сохранить JNI-точки входа (нативные методы вызываются по имени из Rust).
-keepclasseswithmembernames class * {
    native <methods>;
}
-keep class dev.p2p.messenger.Native { *; }
