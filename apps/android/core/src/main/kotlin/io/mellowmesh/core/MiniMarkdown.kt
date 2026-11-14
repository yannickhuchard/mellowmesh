package io.mellowmesh.core

/**
 * The small CommonMark subset MeshUI text blocks use, turned into styled
 * spans the app renders natively: `**bold**`, `*italic*` / `_italic_`,
 * `` `code` ``, `[label](https://…)`, `<https://…>`, `# headings` (bold) and
 * `- bullets`. No HTML, no images: anything else is plain text.
 */
data class MdSpan(
    val text: String,
    val bold: Boolean = false,
    val italic: Boolean = false,
    val code: Boolean = false,
    /** Only http(s) links; the app asks before opening them. */
    val url: String? = null,
)

object MiniMarkdown {
    fun parse(markdown: String): List<MdSpan> {
        val out = ArrayList<MdSpan>()
        val lines = markdown.replace("\r\n", "\n").split('\n')
        for ((i, raw) in lines.withIndex()) {
            var line = raw
            var bold = false
            val heading = Regex("^#{1,6}\\s+").find(line)
            if (heading != null) {
                line = line.substring(heading.range.last + 1)
                bold = true
            }
            val bullet = Regex("^\\s*[-*+]\\s+").find(line)
            if (bullet != null && heading == null) {
                out += MdSpan("• ")
                line = line.substring(bullet.range.last + 1)
            }
            inline(line, bold, out)
            if (i < lines.size - 1) out += MdSpan("\n")
        }
        return merge(out)
    }

    private fun isSafeUrl(u: String): Boolean =
        (u.startsWith("https://", ignoreCase = true) || u.startsWith("http://", ignoreCase = true)) &&
            u.none { it.isWhitespace() || it == '"' || it == '<' || it == '>' }

    private fun inline(s: String, baseBold: Boolean, out: MutableList<MdSpan>) {
        var i = 0
        val buf = StringBuilder()
        var bold = baseBold
        var italic = false
        fun flush() {
            if (buf.isNotEmpty()) {
                out += MdSpan(buf.toString(), bold = bold, italic = italic)
                buf.setLength(0)
            }
        }
        while (i < s.length) {
            val c = s[i]
            when {
                c == '\\' && i + 1 < s.length && s[i + 1] in "\\`*_[]()<>#-" -> {
                    buf.append(s[i + 1]); i += 2
                }
                c == '`' -> {
                    val end = s.indexOf('`', i + 1)
                    if (end > i) {
                        flush()
                        out += MdSpan(s.substring(i + 1, end), code = true)
                        i = end + 1
                    } else {
                        buf.append(c); i++
                    }
                }
                s.startsWith("**", i) || s.startsWith("__", i) -> {
                    flush(); bold = if (baseBold) true else !bold; i += 2
                }
                (c == '*' || c == '_') && !(c == '_' && i > 0 && s[i - 1].isLetterOrDigit()) -> {
                    flush(); italic = !italic; i++
                }
                c == '[' -> {
                    val close = s.indexOf("](", i + 1)
                    val end = if (close > 0) s.indexOf(')', close + 2) else -1
                    if (close > 0 && end > close) {
                        val label = s.substring(i + 1, close)
                        val url = s.substring(close + 2, end).trim()
                        flush()
                        if (isSafeUrl(url)) out += MdSpan(label, bold = bold, italic = italic, url = url)
                        else out += MdSpan(label, bold = bold, italic = italic)
                        i = end + 1
                    } else {
                        buf.append(c); i++
                    }
                }
                c == '<' -> {
                    val end = s.indexOf('>', i + 1)
                    val url = if (end > i) s.substring(i + 1, end) else ""
                    if (end > i && isSafeUrl(url)) {
                        flush()
                        out += MdSpan(url, url = url)
                        i = end + 1
                    } else {
                        buf.append(c); i++
                    }
                }
                else -> {
                    buf.append(c); i++
                }
            }
        }
        flush()
    }

    /** Join neighbours with the same style. */
    private fun merge(spans: List<MdSpan>): List<MdSpan> {
        val out = ArrayList<MdSpan>()
        for (s in spans) {
            val last = out.lastOrNull()
            if (last != null && last.url == null && s.url == null && last.bold == s.bold && last.italic == s.italic && last.code == s.code) {
                out[out.size - 1] = last.copy(text = last.text + s.text)
            } else {
                out += s
            }
        }
        return out
    }

    /** Plain text (for previews and accessibility). */
    fun plain(markdown: String): String = parse(markdown).joinToString("") { it.text }
}
