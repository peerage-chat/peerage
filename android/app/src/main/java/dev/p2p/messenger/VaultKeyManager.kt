package dev.p2p.messenger

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
 * Управляет 32-байтовым vault-ключом, защищённым Android Keystore.
 *
 * Схема: AES-256-GCM ключ в Keystore (non-extractable, hardware-backed где
 * доступно) оборачивает случайный 32-байтовый vault key. Обёрнутый ключ
 * хранится в vault.key.enc: [12 байт IV][ciphertext+tag].
 *
 * Миграция: если существует старый vault.key (plaintext), он автоматически
 * оборачивается и удаляется. Данные перешифровывать не нужно — vault key те же.
 */
object VaultKeyManager {
    private const val KEYSTORE_ALIAS = "p2p-vault"
    private const val ENC_FILE = "vault.key.enc"
    private const val OLD_FILE = "vault.key"
    private const val GCM_IV_LEN = 12
    private const val GCM_TAG_BITS = 128
    private const val KEY_LEN = 32

    fun getOrCreate(filesDir: File): ByteArray {
        val encFile = File(filesDir, ENC_FILE)
        val oldFile = File(filesDir, OLD_FILE)
        val keystoreKey = getOrCreateKeystoreKey()

        return when {
            encFile.exists() -> unwrap(encFile.readBytes(), keystoreKey)
            oldFile.exists() -> {
                // Миграция: plaintext vault.key → защищённый vault.key.enc
                val raw = oldFile.readBytes()
                require(raw.size == KEY_LEN) { "vault.key: неверный размер ${raw.size}" }
                encFile.writeBytes(wrap(raw, keystoreKey))
                oldFile.delete()
                raw
            }
            else -> {
                val raw = ByteArray(KEY_LEN).also { SecureRandom().nextBytes(it) }
                encFile.writeBytes(wrap(raw, keystoreKey))
                raw
            }
        }
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
        require(data.size > GCM_IV_LEN) { "vault.key.enc: файл повреждён" }
        val iv = data.copyOf(GCM_IV_LEN)
        val ct = data.copyOfRange(GCM_IV_LEN, data.size)
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(GCM_TAG_BITS, iv))
        return cipher.doFinal(ct)
    }
}
