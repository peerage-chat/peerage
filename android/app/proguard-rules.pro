# Сохранить JNI-точки входа (нативные методы вызываются по имени из Rust).
-keepclasseswithmembernames class * {
    native <methods>;
}
-keep class chat.peerage.Native { *; }

# Ужимать и оптимизировать, но не переименовывать: трассы падений из «Отправить журнал»
# бета-тестеров должны читаться как есть, без mapping-файла под каждую сборку.
-dontobfuscate

# Воспроизводимая сборка (п. 36): R8 из AGP 9.5 иногда по-разному сужает тип параметра этого
# метода kotlinx-coroutines и его синтетического аксессора (ContinuationImpl ↔ Continuation) —
# две чистые сборки одного коммита давали разный classes.dex. Других расхождений сравнение
# дампов dex не нашло. Сигнатуры закреплены, удалить неиспользуемое — можно.
-keepclassmembers,allowshrinking class kotlinx.coroutines.flow.FlowKt__LimitKt {
    *** *emitAbort*(...);
}
