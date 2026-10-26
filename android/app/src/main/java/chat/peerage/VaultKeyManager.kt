package chat.peerage

import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.File
import java.security.KeyStore
import java.security.SecureRandom
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Уровень защиты ключа хранилища (at-rest, M4).
 *
 * Разница не в силе шифра — он один и тот же, — а в том, ЧЕМ отпирается ключ:
 *   * [KEYSTORE] — тем, что есть у устройства. Ключ не покидает защищённое
 *     железо, снятый дамп файловой системы бесполезен. Но отдаётся приложению по
 *     запросу, то есть тому, кто держит разблокированный телефон;
 *   * [PASSWORD] — тем, что человек ЗНАЕТ. Знание не отдаётся автоматически:
 *     данные остаются шифрованными, пока пароль не введён. Цена — вводить при
 *     каждом холодном старте, и забытый пароль означает потерю переписки;
 *   * [BOTH] — нужно и устройство, и знание.
 */
enum class VaultLevel { KEYSTORE, PASSWORD, BOTH }

/**
 * Управляет 32-байтовым vault-ключом.
 *
 * Схема: случайный 32-байтовый vault key оборачивается в зависимости от уровня —
 * Keystore-ключом (AES-256-GCM, non-extractable), ключом из пароля (Argon2id в
 * ядре, [Native.deriveVaultKey]) или обоими последовательно. Обёрнутый ключ лежит
 * в vault.key.enc: [12 байт IV][ciphertext+tag]; уровень, соль и номер профиля
 * стоимости KDF — в vault.meta.
 *
 * **Смена уровня не перешифровывает данные:** vault key остаётся прежним, меняется
 * только его обёртка. Иначе каждое переключение переписывало бы всю базу и блобы.
 *
 * Оба файла пишутся только атомарно, а смена уровня меняет их разом
 * ([StagedFiles]): обрезанная или наполовину сменённая обёртка — потерянный ключ.
 */
object VaultKeyManager {
    private const val META_FILE = "vault.meta"
    private const val KEYSTORE_ALIAS = "vault-key"
    private const val ENC_FILE = "vault.key.enc"
    private const val GCM_IV_LEN = 12
    private const val GCM_TAG_BITS = 128
    private const val KEY_LEN = 32
    private val PAIR = listOf(META_FILE, ENC_FILE)

    /** Текущий уровень (по умолчанию — Keystore, как было до появления выбора). */
    fun level(filesDir: File): VaultLevel {
        // Сюда приходят первым делом на старте: прерванную смену уровня доводим
        // здесь, до того как читать любой из двух файлов.
        StagedFiles.finish(filesDir, PAIR)
        val meta = File(filesDir, META_FILE)
        if (!meta.exists()) return VaultLevel.KEYSTORE
        return runCatching { VaultLevel.valueOf(meta.readText().lineSequence().first().trim()) }
            .getOrDefault(VaultLevel.KEYSTORE)
    }

    /** Требуется ли пароль для старта (уровень PASSWORD или BOTH). */
    fun needsPassword(filesDir: File): Boolean = level(filesDir) != VaultLevel.KEYSTORE

    private fun salt(filesDir: File): ByteArray {
        val meta = File(filesDir, META_FILE)
        val line = meta.readText().lineSequence().drop(1).firstOrNull().orEmpty().trim()
        return line.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
    }

    /**
     * Профиль стоимости KDF ЭТОГО хранилища. Файл без третьей строки — установка,
     * сделанная до появления номера, то есть профиль 1.
     *
     * Читать профиль из файла обязательно: параметры Argon2id должны расти вслед за
     * железом, но ключ, выведенный другими параметрами, — другой ключ. Возьми мы
     * текущий номер вместо записанного, после повышения стоимости все существующие
     * хранилища перестали бы открываться, показывая «неверный пароль» на верном.
     */
    private fun profile(filesDir: File): Int {
        val meta = File(filesDir, META_FILE)
        if (!meta.exists()) return 1
        return meta.readText().lineSequence().drop(2).firstOrNull()?.trim()?.toIntOrNull() ?: 1
    }

    private fun metaBytes(lvl: VaultLevel, salt: ByteArray, profile: Int): ByteArray =
        (lvl.name + "\n" + salt.joinToString("") { "%02x".format(it) } + "\n" + profile).toByteArray()

    /**
     * Ключ хранилища. `password` нужен на уровнях PASSWORD/BOTH; при неверном
     * пароле бросает — вызывающий обязан показать «неверный пароль», а не молча
     * стартовать с мусорным ключом (иначе данные выглядели бы повреждёнными).
     */
    fun getOrCreate(filesDir: File, password: String? = null): ByteArray {
        val encFile = File(filesDir, ENC_FILE)
        val lvl = level(filesDir)

        return when {
            encFile.exists() -> unwrapAll(encFile.readBytes(), filesDir, lvl, password)
            else -> {
                val raw = ByteArray(KEY_LEN).also { SecureRandom().nextBytes(it) }
                val salt = if (lvl == VaultLevel.KEYSTORE) ByteArray(0) else salt(filesDir)
                StagedFiles.writeAtomic(encFile, wrapAll(raw, lvl, password, salt, profile(filesDir)))
                raw
            }
        }
    }

    /**
     * Сменить уровень защиты. Пере-оборачивает ТОТ ЖЕ vault key, поэтому данные
     * не трогаются. `current` — пароль текущего уровня (если он требуется),
     * `new` — пароль нового. Возвращает false, если текущий пароль не подошёл.
     */
    fun changeLevel(filesDir: File, target: VaultLevel, current: String?, new: String?): Boolean {
        val raw = runCatching { getOrCreate(filesDir, current) }.getOrNull() ?: return false
        if (target != VaultLevel.KEYSTORE && new.isNullOrEmpty()) return false
        val salt = if (target == VaultLevel.KEYSTORE) ByteArray(0) else Native.newVaultSalt()
        // Смена уровня = новая соль, значит и профиль берём текущий: файл всё равно
        // перезаписывается, и старую стоимость тащить незачем.
        val profile = Native.currentKdfProfile()
        StagedFiles.replaceAll(
            filesDir,
            mapOf(
                META_FILE to metaBytes(target, salt, profile),
                ENC_FILE to wrapAll(raw, target, new, salt, profile),
            ),
        )
        return true
    }

    /**
     * Забыть ключ хранилища целиком: обёртку, параметры уровня и ключ Keystore.
     * Только для восстановления (R-029), когда Keystore свой ключ потерял и
     * развернуть обёртку уже нечем. Следующий [getOrCreate] заведёт новый ключ.
     */
    fun reset(filesDir: File) {
        listOf(ENC_FILE, META_FILE).forEach { File(filesDir, it).delete() }
        runCatching {
            KeyStore.getInstance("AndroidKeyStore").apply { load(null) }.deleteEntry(KEYSTORE_ALIAS)
        }
    }

    /** Обернуть ключ по уровню: пароль внутри, Keystore снаружи. Соль и профиль —
     *  явно: при смене уровня это параметры НОВОЙ обёртки, на диске их ещё нет. */
    private fun wrapAll(
        raw: ByteArray,
        lvl: VaultLevel,
        password: String?,
        salt: ByteArray,
        profile: Int,
    ): ByteArray {
        var data = raw
        if (lvl != VaultLevel.KEYSTORE) {
            val kek = Native.deriveVaultKey(password.orEmpty(), salt, profile)
            require(kek.size == KEY_LEN) { "kdf failed" }
            data = wrap(data, javax.crypto.spec.SecretKeySpec(kek, "AES"))
        }
        if (lvl != VaultLevel.PASSWORD) {
            data = wrap(data, getOrCreateKeystoreKey())
        }
        return data
    }

    /** Развернуть в обратном порядке: сначала Keystore, потом пароль. */
    private fun unwrapAll(data: ByteArray, filesDir: File, lvl: VaultLevel, password: String?): ByteArray {
        var out = data
        if (lvl != VaultLevel.PASSWORD) {
            out = unwrap(out, getOrCreateKeystoreKey())
        }
        if (lvl != VaultLevel.KEYSTORE) {
            val kek = Native.deriveVaultKey(password.orEmpty(), salt(filesDir), profile(filesDir))
            require(kek.size == KEY_LEN) { "kdf failed" }
            out = unwrap(out, javax.crypto.spec.SecretKeySpec(kek, "AES"))
        }
        return out
    }

    private fun getOrCreateKeystoreKey(): SecretKey {
        val ks = KeyStore.getInstance("AndroidKeyStore").apply { load(null) }
        if (!ks.containsAlias(KEYSTORE_ALIAS)) {
            KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore").apply {
                init(
                    KeyGenParameterSpec.Builder(
                        KEYSTORE_ALIAS,
                        KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
                    )
                        .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                        .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                        .setKeySize(256)
                        // Не требуем аутентификацию пользователя: ключ нужен
                        // при старте в фоне (до разблокировки экрана не нужен,
                        // но биометрия блокирует фоновую работу).
                        .setUserAuthenticationRequired(false)
                        .build(),
                )
                generateKey()
            }
        }
        return ks.getKey(KEYSTORE_ALIAS, null) as SecretKey
    }

    private fun wrap(raw: ByteArray, key: SecretKey): ByteArray {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key)
        val iv = cipher.iv // 12 байт, генерируется Keystore
        val ct = cipher.doFinal(raw)
        return iv + ct
    }

    private fun unwrap(data: ByteArray, key: SecretKey): ByteArray {
        require(data.size > GCM_IV_LEN) { "vault.key.enc: file corrupted" }
        val iv = data.copyOf(GCM_IV_LEN)
        val ct = data.copyOfRange(GCM_IV_LEN, data.size)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(GCM_TAG_BITS, iv))
        return cipher.doFinal(ct)
    }
}
