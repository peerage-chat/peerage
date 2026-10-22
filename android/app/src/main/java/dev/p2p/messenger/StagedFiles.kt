package dev.p2p.messenger

import java.io.File
import java.io.FileOutputStream
import java.io.IOException

/**
 * Атомарная запись файлов ключа хранилища (инвариант 6) — по одному и парой.
 *
 * Android убивает процесс в любой момент. Обрезанный `vault.key.enc` — это
 * потерянный ключ, а с ним и все данные, поэтому файл пишется рядом и
 * переименовывается поверх. Смена уровня защиты меняет ДВА файла (обёртку и её
 * параметры), и половина смены так же фатальна: новая соль при старой обёртке
 * не разворачивает ключ. Поэтому пара идёт через журнал — новые версии лежат
 * рядом как `.new`, точка фиксации — маркер, и [finish] на старте доводит
 * прерванную смену до конца либо отбрасывает её целиком.
 */
object StagedFiles {
    private const val NEW = ".new"
    private const val MARKER = "vault.commit"

    /** Записать файл целиком или не трогать вовсе. */
    fun writeAtomic(file: File, data: ByteArray) {
        val tmp = File(file.parentFile, file.name + ".tmp")
        FileOutputStream(tmp).use {
            it.write(data)
            it.fd.sync()
        }
        if (!tmp.renameTo(file)) {
            tmp.delete()
            throw IOException("rename ${file.name}")
        }
    }

    /** Заменить несколько файлов в `dir` разом: после сбоя на старте будут либо все
     *  старые версии, либо все новые. */
    fun replaceAll(dir: File, files: Map<String, ByteArray>) {
        for ((name, data) in files) writeAtomic(File(dir, name + NEW), data)
        writeAtomic(File(dir, MARKER), ByteArray(0)) // точка фиксации
        finish(dir, files.keys)
    }

    /** Довести прерванную замену: есть маркер — новые версии готовы целиком, ставим
     *  их; нет — замена не дошла до фиксации, её обрывки выбрасываем. `names` —
     *  файлы, которые меняются парой. */
    fun finish(dir: File, names: Collection<String>) {
        val staged = names.map { File(dir, it + NEW) }.filter { it.exists() }
        val marker = File(dir, MARKER)
        if (marker.exists()) {
            for (f in staged) {
                if (!f.renameTo(File(dir, f.name.removeSuffix(NEW)))) throw IOException("rename ${f.name}")
            }
            marker.delete()
        } else {
            staged.forEach { it.delete() }
        }
    }
}
