package chat.peerage

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/** Проверка обновлений: что берётся из ответа GitHub и манифеста (docs/release.md). */
class UpdatesTest {
    private val cert = "04ca852072eb8de62c748e6edbecfb6c6005c759296166f8c013450f41a7cca2"
    private val sha = "80a895d5a225dfe2d8a668ed2f4c89be4fc005637c97985ff03053591cf467f0"

    private fun manifest(
        format: Int = 1,
        code: Long = 400,
        file: String = "peerage-0.4.0-arm64-v8a.apk",
        abi: String = "arm64-v8a",
        tag: String = "v0.4.0",
    ) = """
        {
          "format": $format, "version_name": "0.4.0", "version_code": $code, "tag": "$tag",
          "commit": "e7810bea014558510ab80ed09d709db71138a6d2", "date": "2026-11-25",
          "cert_sha256": "$cert", "future_field": {"x": 1},
          "apks": [{"abi": "$abi", "file": "$file", "size": 39016503, "sha256": "$sha"}],
          "notes": "https://github.com/peerage-chat/peerage/releases/tag/$tag"
        }
    """.trimIndent()

    @Test
    fun manifest_gives_the_apk_for_the_device_abi() {
        val u = UpdateManifest.parse(manifest(), listOf("arm64-v8a", "armeabi-v7a"))!!
        assertEquals("0.4.0", u.versionName)
        assertEquals(400L, u.versionCode)
        assertEquals("v0.4.0", u.tag)
        assertEquals("peerage-0.4.0-arm64-v8a.apk", u.file)
        assertEquals(39016503L, u.size)
        assertEquals(sha, u.sha256)
        assertEquals(cert, u.certSha256)
    }

    @Test
    fun no_apk_for_the_device_abi_means_no_update() {
        assertNull(UpdateManifest.parse(manifest(), listOf("x86_64")))
    }

    @Test
    fun unknown_format_is_silence() {
        assertNull(UpdateManifest.parse(manifest(format = 2), listOf("arm64-v8a")))
    }

    @Test
    fun file_and_tag_that_could_leave_the_release_are_rejected() {
        assertNull(UpdateManifest.parse(manifest(file = "../evil.apk"), listOf("arm64-v8a")))
        assertNull(UpdateManifest.parse(manifest(file = "peerage.zip"), listOf("arm64-v8a")))
        assertNull(UpdateManifest.parse(manifest(tag = "v0.4.0/../x"), listOf("arm64-v8a")))
    }

    @Test
    fun garbage_is_silence() {
        assertNull(UpdateManifest.parse("<html>rate limited</html>", listOf("arm64-v8a")))
        assertNull(UpdateManifest.parse("{}", listOf("arm64-v8a")))
    }

    @Test
    fun newest_release_with_a_manifest_wins_prereleases_included() {
        val releases = """
            [
              {"tag_name": "v0.4.1", "draft": false, "prerelease": true, "assets": [{"name": "SHA256SUMS"}]},
              {"tag_name": "v0.4.0", "draft": false, "prerelease": true,
               "assets": [{"name": "peerage-0.4.0-arm64-v8a.apk"}, {"name": "update.json"}]},
              {"tag_name": "v0.3.0", "draft": false, "prerelease": true, "assets": [{"name": "update.json"}]}
            ]
        """.trimIndent()
        assertEquals("v0.4.0", UpdateManifest.releaseTag(releases))
    }

    @Test
    fun highest_version_wins_over_a_later_patch_of_an_older_line() {
        // 0.4.2 выпущена после 0.5.0 — GitHub ставит её первой, но новее 0.5.0.
        val releases = """
            [
              {"tag_name": "v0.4.2", "draft": false, "assets": [{"name": "update.json"}]},
              {"tag_name": "v0.5.0", "draft": false, "assets": [{"name": "update.json"}]},
              {"tag_name": "v0.4.1", "draft": false, "assets": [{"name": "update.json"}]},
              {"tag_name": "nightly", "draft": false, "assets": [{"name": "update.json"}]}
            ]
        """.trimIndent()
        assertEquals("v0.5.0", UpdateManifest.releaseTag(releases))
    }

    @Test
    fun drafts_are_skipped() {
        val releases = """
            [
              {"tag_name": "v0.5.0", "draft": true, "assets": [{"name": "update.json"}]},
              {"tag_name": "v0.4.0", "draft": false, "assets": [{"name": "update.json"}]}
            ]
        """.trimIndent()
        assertEquals("v0.4.0", UpdateManifest.releaseTag(releases))
    }

    @Test
    fun no_releases_or_error_object_gives_nothing() {
        assertNull(UpdateManifest.releaseTag("[]"))
        assertNull(UpdateManifest.releaseTag("""{"message": "API rate limit exceeded"}"""))
    }

    private val day = 86_400L
    private val t0 = 1_000L * day

    @Test
    fun no_check_without_consent() {
        assertFalse(UpdateFacts().due(t0))
        assertFalse(UpdateFacts(consent = false).due(t0))
    }

    @Test
    fun check_once_a_day() {
        val f = UpdateFacts(consent = true, checkedAt = t0)
        assertFalse(f.due(t0 + day - 1))
        assertTrue(f.due(t0 + day))
    }

    @Test
    fun later_hides_this_version_for_a_day_but_not_the_next_one() {
        val u = UpdateManifest.parse(manifest(code = 400), listOf("arm64-v8a"))!!
        val next = UpdateManifest.parse(manifest(code = 401), listOf("arm64-v8a"))!!
        val f = UpdateFacts(consent = true).later(u, t0)
        assertFalse(f.shows(u, t0 + day - 1))
        assertTrue(f.shows(u, t0 + day))
        assertTrue(f.shows(next, t0 + 1))
    }
}
