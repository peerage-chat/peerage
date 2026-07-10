package dev.p2p.messenger

import org.json.JSONObject
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Юнит-тесты чистой логики контакта (M2): приоритет имени для показа и разбор JSON
 * движка. Без Android-фреймворка — гоняются на JVM (`./gradlew test`). Ловят рассинхрон
 * имён полей между Rust (`contacts_json`) и Kotlin, а также ошибки в приоритете имени.
 */
class ContactTest {
    private val hex = "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c"

    // ── displayName: local → profile_name → hex ──────────────────────────────

    @Test fun displayName_prefers_local_note() {
        val c = Contact(hex, name = "Мой Друг", fingerprint = "", verified = false, profileName = "SelfName")
        assertEquals("Мой Друг", c.displayName)
    }

    @Test fun displayName_falls_back_to_self_name_when_local_blank() {
        val c = Contact(hex, name = "", fingerprint = "", verified = false, profileName = "SelfName")
        assertEquals("SelfName", c.displayName)
    }

    @Test fun displayName_falls_back_to_hex_when_both_blank() {
        val c = Contact(hex, name = "", fingerprint = "", verified = false, profileName = "")
        // короткий hex — первые 8 символов ключа
        assertEquals("ea4a6c63", c.displayName)
        assertEquals(8, c.displayName.length)
    }

    @Test fun displayName_local_wins_even_without_profile() {
        val c = Contact(hex, name = "Заметка", fingerprint = "", verified = false, profileName = "")
        assertEquals("Заметка", c.displayName)
    }

    // ── fromJson: контракт полей с Rust ──────────────────────────────────────

    @Test fun fromJson_maps_all_fields() {
        val o = JSONObject()
            .put("key", hex)
            .put("name", "Локальное")
            .put("fingerprint", "write annual again")
            .put("verified", true)
            .put("profile_name", "Алиса")
            .put("avatar_id", "ab12cd")
        val c = Contact.fromJson(o)
        assertEquals(hex, c.key)
        assertEquals("Локальное", c.name)
        assertEquals("write annual again", c.fingerprint)
        assertTrue(c.verified)
        assertEquals("Алиса", c.profileName)
        assertEquals("ab12cd", c.avatarId)
    }

    @Test fun fromJson_defaults_when_profile_fields_absent() {
        // Старый контакт/движок без M2-полей — не должен падать, поля пустые.
        val o = JSONObject().put("key", hex).put("name", "Боб")
        val c = Contact.fromJson(o)
        assertEquals("Боб", c.name)
        assertEquals("", c.profileName)
        assertEquals("", c.avatarId)
        assertFalse(c.verified)
        // displayName при пустом профиле = локальная заметка
        assertEquals("Боб", c.displayName)
    }

    @Test fun fromJson_contact_with_only_self_name_shows_self_name() {
        // Контакт добавлен без локальной заметки, но прислал самоназвание.
        val o = JSONObject().put("key", hex).put("name", "").put("profile_name", "Charlie")
        assertEquals("Charlie", Contact.fromJson(o).displayName)
    }

    @Test fun auto_added_contact_shows_self_name_not_placeholder() {
        // TOFU-авто-добавление (ensure_contact) даёт пустое локальное имя; когда придёт
        // самоназвание — оно и показывается (а не hex-плейсхолдер, как было в баге).
        val autoAdded = Contact(hex, name = "", fingerprint = "", verified = false)
        assertEquals("ea4a6c63", autoAdded.displayName) // до профиля — короткий hex
        val withProfile = autoAdded.copy(profileName = "Dave")
        assertEquals("Dave", withProfile.displayName) // профиль пришёл → самоназвание
    }
}
