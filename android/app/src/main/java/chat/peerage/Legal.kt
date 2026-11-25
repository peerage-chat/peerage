package chat.peerage

import android.content.Context

/** Кусок документа: подзаголовок, абзац или пункт списка. */
sealed interface DocBlock {
    data class Heading(val text: String) : DocBlock
    data class Para(val text: String) : DocBlock
    data class Bullet(val text: String) : DocBlock
}

/**
 * Политика конфиденциальности (app-ui.md → «О приложении»). Источник один —
 * `assets/legal/privacy.<язык>.md`: его же показывает GitHub по публичной ссылке (её просит
 * Play). Язык — тот же, что у журнала изменений (`changelog_lang`); нет файла — английский.
 *
 * Разметка — узкое подмножество Markdown, без внешней библиотеки: `# ` — заголовок документа
 * (его показывает шапка экрана, здесь пропускается), `## ` — подзаголовок, `- ` — пункт,
 * остальное — абзацы через пустую строку.
 */
object Legal {
    fun privacy(ctx: Context): List<DocBlock> {
        val lang = ctx.getString(R.string.changelog_lang)
        val text = runCatching { read(ctx, lang) }.getOrNull() ?: runCatching { read(ctx, "en") }.getOrNull() ?: return emptyList()
        return parse(text)
    }

    private fun read(ctx: Context, lang: String) =
        ctx.assets.open("legal/privacy.$lang.md").use { String(it.readBytes(), Charsets.UTF_8) }

    fun parse(md: String): List<DocBlock> {
        val out = mutableListOf<DocBlock>()
        val para = StringBuilder()
        fun flush() {
            if (para.isNotEmpty()) out += DocBlock.Para(para.toString())
            para.clear()
        }
        for (raw in md.lineSequence()) {
            val line = raw.trimEnd()
            when {
                line.isBlank() -> flush()
                line.startsWith("## ") -> {
                    flush()
                    out += DocBlock.Heading(line.removePrefix("## ").trim())
                }
                line.startsWith("# ") -> flush()
                line.startsWith("- ") -> {
                    flush()
                    out += DocBlock.Bullet(line.removePrefix("- ").trim())
                }
                // Продолжение пункта с отступом — тот же пункт.
                line.startsWith("  ") && para.isEmpty() && out.lastOrNull() is DocBlock.Bullet -> {
                    val b = out.removeAt(out.lastIndex) as DocBlock.Bullet
                    out += DocBlock.Bullet(b.text + " " + line.trim())
                }
                else -> {
                    if (para.isNotEmpty()) para.append(' ')
                    para.append(line.trim())
                }
            }
        }
        flush()
        return out
    }
}
