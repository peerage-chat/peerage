package dev.p2p.messenger

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Юнит-тесты чистой логики контакта (M2): приоритет имени для показа и разбор
 * ответов движка. Без Android-фреймворка — гоняются на JVM (`./gradlew test`).
 *
 * Разбор проверяется на СЫРОМ JSON, который отдаёт Rust: так тест ловит рассинхрон
 * имён полей между `dto.rs` и `Dto.kt`, а не просто копирование структуры в себя.
 */
class ContactTest {
    private val hex = "ea4a6c63e29c520abef5507b132ec5f9954776aebebe7b92421eea691446d22c"

    // ── displayName: local → profile_name → hex ──────────────────────────────

    @Test fun displayName_prefers_local_note() {
        val c = Contact(hex, localName = "Мой Друг", fingerprint = "", verified = false, profileName = "SelfName")
        assertEquals("Мой Друг", c.displayName)
    }

    @Test fun displayName_falls_back_to_self_name_when_local_blank() {
        val c = Contact(hex, localName = "", fingerprint = "", verified = false, profileName = "SelfName")
        assertEquals("SelfName", c.displayName)
    }

    @Test fun displayName_falls_back_to_hex_when_both_blank() {
        val c = Contact(hex, localName = "", fingerprint = "", verified = false, profileName = "")
        // короткий hex — первые 8 символов ключа
        assertEquals("ea4a6c63", c.displayName)
        assertEquals(8, c.displayName.length)
    }

    @Test fun displayName_local_wins_even_without_profile() {
        val c = Contact(hex, localName = "Заметка", fingerprint = "", verified = false, profileName = "")
        assertEquals("Заметка", c.displayName)
    }

    // ── Разбор: контракт полей с Rust ────────────────────────────────────────

    @Test fun contact_json_maps_all_fields() {
        val json = """
            [{"key":"$hex","name":"Локальное","fingerprint":"write annual again",
              "added":1700000000,"verified":true,"profile_name":"Алиса",
              "avatar_id":"ab12cd","helpful":3}]
        """.trimIndent()
        val c = decodeList<ContactDto>(json).single().toModel()
        assertEquals(hex, c.key)
        assertEquals("Локальное", c.localName)
        assertEquals("write annual again", c.fingerprint)
        assertTrue(c.verified)
        assertEquals("Алиса", c.profileName)
        assertEquals("ab12cd", c.avatarId)
    }

    @Test fun contact_json_defaults_when_profile_fields_absent() {
        // Старый контакт/движок без M2-полей — не должен падать, поля пустые.
        val c = decodeList<ContactDto>("""[{"key":"$hex","name":"Боб"}]""").single().toModel()
        assertEquals("Боб", c.localName)
        assertEquals("", c.profileName)
        assertEquals("", c.avatarId)
        assertFalse(c.verified)
        // displayName при пустом профиле = локальная заметка
        assertEquals("Боб", c.displayName)
    }

    @Test fun contact_json_with_only_self_name_shows_self_name() {
        // Контакт добавлен без локальной заметки, но прислал самоназвание.
        val json = """[{"key":"$hex","name":"","profile_name":"Charlie"}]"""
        assertEquals("Charlie", decodeList<ContactDto>(json).single().toModel().displayName)
    }

    @Test fun unknown_field_from_newer_engine_does_not_break_parsing() {
        // Движок может добавить поле раньше, чем клиент про него узнает: старая
        // сборка обязана продолжать читать список, а не падать на неизвестном ключе.
        val json = """[{"key":"$hex","name":"Боб","future_field":42}]"""
        assertEquals("Боб", decodeList<ContactDto>(json).single().name)
    }

    @Test fun auto_added_contact_shows_self_name_not_placeholder() {
        // TOFU-авто-добавление (ensure_contact) даёт пустое локальное имя; когда придёт
        // самоназвание — оно и показывается (а не hex-плейсхолдер, как было в баге).
        val autoAdded = Contact(hex, localName = "", fingerprint = "", verified = false)
        assertEquals("ea4a6c63", autoAdded.displayName) // до профиля — короткий hex
        val withProfile = autoAdded.copy(profileName = "Dave")
        assertEquals("Dave", withProfile.displayName) // профиль пришёл → самоназвание
    }
}
