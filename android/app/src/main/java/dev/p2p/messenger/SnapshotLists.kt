package dev.p2p.messenger

import androidx.compose.runtime.snapshots.Snapshot
import androidx.compose.runtime.snapshots.SnapshotStateList

/**
 * Заменить содержимое списка целиком — одним шагом для всех, кто его читает.
 *
 * `clear()` + `addAll()` из фонового потока — два отдельных изменения: между ними
 * UI рисует пустой список (мигание), а тикер, идущий по тому же списку, видит
 * «ни одного контакта» и пропускает свою работу. Внутри изменяемого снимка оба
 * шага применяются разом.
 */
fun <T> SnapshotStateList<T>.replaceWith(items: List<T>) {
    Snapshot.withMutableSnapshot {
        clear()
        addAll(items)
    }
}
