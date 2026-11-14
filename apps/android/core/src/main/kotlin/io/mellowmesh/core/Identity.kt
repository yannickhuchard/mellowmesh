package io.mellowmesh.core

import java.time.Instant
import java.time.OffsetDateTime
import java.time.format.DateTimeParseException

/** Kind of a principal, from its URI scheme (PROTOCOL.md §3.1). */
enum class PrincipalKind {
    HUMAN, AGENT, NODE, INTERFACE;

    companion object {
        fun of(uri: String): PrincipalKind = when (uri.substringBefore("://", "")) {
            "human" -> HUMAN
            "agent" -> AGENT
            "node" -> NODE
            else -> INTERFACE
        }
    }
}

/** Helper colors (design language: color = identity). Grey means asleep. */
enum class Accent { INDIGO, SKY, PINK, AMBER, MINT, VIOLET }

object Identity {
    private val ACCENTS = listOf(Accent.INDIGO, Accent.SKY, Accent.PINK, Accent.AMBER, Accent.MINT)

    /** 64-bit FNV-1a over the lowercased UTF-8 id; same value as app-core's `stable_hash`. */
    fun stableHash(id: String): ULong {
        var h = 0xcbf29ce484222325UL
        for (b in id.lowercase().toByteArray(Charsets.UTF_8)) {
            h = h xor (b.toUByte().toULong())
            h *= 0x00000100000001b3UL
        }
        return h
    }

    /** Deterministic helper color: same helper, same color on every device. */
    fun accentOf(id: String): Accent {
        val h = stableHash(id)
        val idx = ((h xor (h shr 29)) % ACCENTS.size.toULong()).toInt()
        return ACCENTS[idx]
    }

    /** `agent://yannick/coder` → `coder`, `human://yannick` → `yannick`. */
    fun shortName(uri: String): String {
        val rest = if ("://" in uri) uri.substringAfter("://") else uri
        val name = rest.trimEnd('/').substringAfterLast('/')
        return Sanitize.line(name.ifEmpty { uri })
    }

    /** Initials for people's round avatars ("alice-martin" → "AM"). */
    fun initials(name: String): String {
        val parts = name.split(' ', '-', '_', '.').filter { it.isNotBlank() }
        val letters = when {
            parts.isEmpty() -> "?"
            parts.size == 1 -> parts[0].take(2)
            else -> "${parts[0].first()}${parts[1].first()}"
        }
        return letters.uppercase()
    }

    /** `agent://owner/name` → `owner.name` (the inbox topic path). */
    fun agentTopicPath(agentId: String): String? {
        if (!agentId.startsWith("agent://")) return null
        val segs = agentId.removePrefix("agent://").split('/').filter { it.isNotEmpty() }
        if (segs.isEmpty()) return null
        return segs.joinToString(".").lowercase()
    }

    fun agentIdFromPath(segs: List<String>): String = "agent://" + segs.joinToString("/")

    fun samePrincipal(a: String?, b: String?): Boolean =
        a != null && b != null && a.equals(b, ignoreCase = true)
}

/** Strips terminal escapes, control and bidi characters from fabric strings. */
object Sanitize {
    private const val ESC = '\u001b'

    private fun isBidi(c: Char): Boolean =
        c in '‪'..'‮' || c in '⁦'..'⁩' || c == '‎' || c == '‏' || c == '؜'

    /** Keeps `\n` and `\t`; removes escape sequences, other controls and bidi overrides. */
    fun text(input: String): String {
        if (input.none { (it.isISOControl() && it != '\n' && it != '\t') || isBidi(it) }) return input
        val out = StringBuilder(input.length)
        var i = 0
        while (i < input.length) {
            val c = input[i]
            when {
                c == ESC -> {
                    i++
                    if (i >= input.length) break
                    when (input[i]) {
                        '[' -> { // CSI: parameters/intermediates then a final byte 0x40..0x7e
                            i++
                            while (i < input.length && input[i] !in '@'..'~') i++
                        }
                        ']', 'P', 'X', '^', '_' -> { // string terminated by BEL or ST
                            i++
                            while (i < input.length) {
                                if (input[i] == '\u0007') break
                                if (input[i] == ESC && i + 1 < input.length && input[i + 1] == '\\') { i++; break }
                                i++
                            }
                        }
                        in ' '..'/' -> {
                            while (i < input.length && input[i] in ' '..'/') i++
                        }
                        else -> Unit // two-character escape: drop both
                    }
                }
                c == '\u009b' -> { // 8-bit CSI
                    i++
                    while (i < input.length && input[i] !in '@'..'~') i++
                }
                c == '\n' || c == '\t' -> out.append(c)
                c.isISOControl() || isBidi(c) -> Unit
                else -> out.append(c)
            }
            i++
        }
        return out.toString()
    }

    /** Like [text] but on one line. */
    fun line(input: String): String = text(input).replace('\n', ' ').replace('\t', ' ')
}

object Time {
    /** Parse an RFC 3339 timestamp to epoch milliseconds. */
    fun parseMs(s: String?): Long? {
        if (s.isNullOrBlank()) return null
        return try {
            OffsetDateTime.parse(s).toInstant().toEpochMilli()
        } catch (_: DateTimeParseException) {
            try {
                Instant.parse(s).toEpochMilli()
            } catch (_: DateTimeParseException) {
                null
            }
        }
    }

    fun formatMs(ms: Long): String = Instant.ofEpochMilli(ms).toString()

    /** "just now", "5 min", "2 h", "3 d" — short, plain durations for the UI. */
    fun shortDuration(seconds: Long): String {
        val s = seconds.coerceAtLeast(0)
        return when {
            s < 60 -> "${s}s"
            s < 3600 -> "${s / 60} min"
            s < 86_400 -> {
                val h = s / 3600
                val m = (s % 3600) / 60
                if (m == 0L) "$h h" else "$h h $m min"
            }
            else -> "${s / 86_400} d"
        }
    }

    fun ago(nowMs: Long, thenMs: Long): String {
        val s = (nowMs - thenMs) / 1000
        return if (s < 45) "just now" else shortDuration(s) + " ago"
    }
}
