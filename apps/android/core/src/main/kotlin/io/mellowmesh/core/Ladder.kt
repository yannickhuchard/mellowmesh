package io.mellowmesh.core

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/**
 * The interruption ladder (design language §3). Helpers climb only as far as
 * they need; the user picks the loudest allowed level, quiet hours and a
 * daily digest. Pure logic: the app maps the outcome to in-app strips and
 * Android notifications.
 */
enum class Level(val rank: Int, val title: String, val description: String) {
    GLANCE(1, "Glance", "A dot on the place. No sound."),
    WHISPER(2, "Whisper", "A quiet strip that fades away."),
    TAP(3, "Tap on the shoulder", "The helper peeks in and waves: Now or Later."),
    KNOCK(4, "Knock", "A phone notification with the choices."),
    ALARM(5, "Alarm", "Rings until you look. Only for money or safety."),
    ;

    companion object {
        fun parse(s: String?): Level? = entries.firstOrNull { it.name.equals(s?.trim(), ignoreCase = true) }
        fun min(a: Level, b: Level): Level = if (a.rank <= b.rank) a else b
    }
}

@Serializable
data class LadderSettings(
    /** Loudest level helpers may use (default Knock). Alarm must be chosen explicitly. */
    val maxLevel: Level = Level.KNOCK,
    val quietHoursEnabled: Boolean = true,
    /** Minutes after midnight, local time. Default 21:00 → 08:00. */
    val quietStartMinute: Int = 21 * 60,
    val quietEndMinute: Int = 8 * 60,
    val digestEnabled: Boolean = true,
    /** When the daily digest is shown (default 18:00). */
    val digestMinute: Int = 18 * 60,
    /** "Later": no interruption (except Alarm) until this time. */
    val snoozedUntilMs: Long = 0,
)

data class Interruption(
    val key: String,
    val requested: Level,
    val title: String,
    val body: String,
    /** Where tapping it leads: a conversation id or a decision id. */
    val conversationId: String? = null,
    val decisionId: String? = null,
)

data class LadderOutcome(
    /** The level actually used (may be lower than requested). */
    val level: Level,
    /** Post an Android notification (Knock and Alarm). */
    val notify: Boolean,
    /** Collect into the daily digest. */
    val toDigest: Boolean,
    val reason: String,
)

object Ladder {
    const val URGENCY_HEADER = "x-mellowmesh-urgency"

    fun isQuiet(settings: LadderSettings, minuteOfDay: Int): Boolean {
        if (!settings.quietHoursEnabled) return false
        val s = settings.quietStartMinute
        val e = settings.quietEndMinute
        return when {
            s == e -> false
            s < e -> minuteOfDay in s until e
            else -> minuteOfDay >= s || minuteOfDay < e // wraps midnight
        }
    }

    /** Apply the user's policy to one interruption. */
    fun decide(requested: Level, settings: LadderSettings, minuteOfDay: Int, nowMs: Long): LadderOutcome {
        var level = Level.min(requested, settings.maxLevel)
        var reason = if (level != requested) "limited to ${settings.maxLevel.title}" else "as requested"
        val alarm = level == Level.ALARM
        if (!alarm && nowMs < settings.snoozedUntilMs) {
            level = Level.GLANCE
            reason = "snoozed"
        } else if (!alarm && isQuiet(settings, minuteOfDay)) {
            level = Level.GLANCE
            reason = "quiet hours"
        }
        val toDigest = settings.digestEnabled && (requested.rank < Level.KNOCK.rank || level.rank < requested.rank)
        return LadderOutcome(level, notify = level.rank >= Level.KNOCK.rank, toDigest = toDigest, reason = reason)
    }

    /** Whether the daily digest should be shown now. `lastDigestDay` is an epoch day. */
    fun digestDue(settings: LadderSettings, today: Long, minuteOfDay: Int, lastDigestDay: Long?, pending: Int): Boolean =
        settings.digestEnabled && pending > 0 && minuteOfDay >= settings.digestMinute && (lastDigestDay == null || lastDigestDay < today)

    /**
     * How loud a live message wants to be. Returns null for traffic that should
     * never interrupt (heartbeats, my own messages, system chatter).
     */
    fun classify(msg: Message, me: String, dir: Directory): Interruption? {
        if (Identity.samePrincipal(msg.from, me)) return null
        val topic = msg.topic.lowercase()
        val segs = topic.split('.')
        val headerLevel = Level.parse(msg.headers?.get(URGENCY_HEADER))
        val payload = msg.payload as? JsonObject
        fun str(key: String) = (payload?.get(key) as? JsonPrimitive)?.takeIf { it.isString }?.content
        val who = dir.nameOf(msg.from)

        if (segs.size == 3 && segs[0] == "_decision") {
            if (segs[2] != "requested") return null
            val title = str("title")?.let { Sanitize.line(it) }
            val asker = str("created_by")?.let { dir.nameOf(it) } ?: "A helper"
            return Interruption(
                key = "decision:${segs[1]}",
                requested = if (headerLevel == Level.ALARM) Level.ALARM else Level.KNOCK,
                title = "$asker needs your OK",
                body = title ?: "A request is waiting at your To-do desk.",
                decisionId = segs[1],
            )
        }
        if (segs.size == 3 && segs[0] == "_task") {
            val title = str("title")?.let { Sanitize.line(it) } ?: "a task"
            val level = when (segs[2]) {
                "completed" -> Level.WHISPER
                "reclaimed", "cancelled" -> Level.WHISPER
                "updated" -> if (str("status") == TaskStatus.BLOCKED) Level.TAP else return null
                else -> return null
            }
            val text = when (segs[2]) {
                "completed" -> "Finished: $title"
                "reclaimed" -> "A task is free again: $title"
                "cancelled" -> "Cancelled: $title"
                else -> "Stuck: $title"
            }
            return Interruption("task:${segs[1]}:${segs[2]}", headerLevel?.let { Level.min(it, Level.KNOCK) } ?: level, text, "", null)
        }
        val conv = Conversations.classify(msg.topic) ?: return null
        val mentioned = msg.headers?.get("x-mentions")?.split(',', ' ')?.any { Identity.samePrincipal(it.trim(), me) } == true
        val base = when {
            conv.kind == ConversationKind.AGENT && topic.endsWith(".outbox") -> Level.WHISPER
            conv.kind == ConversationKind.PERSON || mentioned -> Level.TAP
            else -> Level.GLANCE
        }
        // Senders may ask to be quieter, or louder up to Knock; Alarm is never self-granted for chat.
        val requested = headerLevel?.let { Level.min(it, Level.KNOCK) } ?: base
        val preview = Sanitize.line(msg.body).take(140)
        return Interruption(
            key = "msg:${msg.id.ifEmpty { msg.topic + msg.timestamp }}",
            requested = requested,
            title = who,
            body = preview,
            conversationId = conv.id,
        )
    }
}
