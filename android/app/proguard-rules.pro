# Сохранить JNI-точки входа (нативные методы вызываются по имени из Rust).
-keepclasseswithmembernames class * {
    native <methods>;
}
-keep class chat.peerage.Native { *; }

# Ужимать и оптимизировать, но не переименовывать: трассы падений из «Отправить журнал»
# бета-тестеров должны читаться как есть, без mapping-файла под каждую сборку.
-dontobfuscate
