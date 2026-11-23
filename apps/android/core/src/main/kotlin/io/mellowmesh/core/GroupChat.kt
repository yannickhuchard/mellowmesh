package io.mellowmesh.core

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.add
import kotlinx.serialization.json.addJsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject

/**
 * Talking to a group of Mellows at once, wherever each one runs.
 *
 * There is no shared room: the Mellows of a group may live on different
 * machines and networks. The phone says each line once per member, in that
 * Mellow's own conversation on its own machine, tagged `payload.mm_group`
 * (the group, the turn, who it is for, what was said lately). Each Mellow
 * answers in its conversation (`parent_id` = the line it got); the phone
 * reads them all back as one thread. When a Mellow writes `@Name` of
 * another member, the phone hands the answer over to that Mellow — at most
 * [MAX_HANDOVERS] times per turn, never twice for the same answer.
 *
 * Group lines stay out of the one-to-one conversations ([isGroupTraffic]).
 */

/** A member of a group, where it lives, and who the phone is there. */
data class GroupMember(
    val id: String,
    val name: String,
    val machineId: String,
    val conversationId: String,
    /** The phone's principal on that machine. */
    val me: String,
)

/** A message read from a member's conversation. */
data class MemberMessage(val member: GroupMember, val message: Message)

/** The group tag of a message. */
data class GroupTag(
    val groupId: String,
    val turn: String,
    /** Member ids the line is for; empty = everyone. */
    val to: List<String>,
    /** The answer this line hands over, when it is a hand-over. */
    val handoverOf: String?,
    val handoverFrom: String?,
)

sealed interface GroupItem {
    val key: String
    val tsMs: Long
}

/** Something said: by the person (once, whatever the number of members) or by a Mellow. */
data class GroupSaid(
    val message: ChatMessage,
    val turn: String,
    /** For the person's lines: the names it was for; empty = everyone. */
    val toNames: List<String>,
    /** For a Mellow's answer: the member who said it. */
    val member: GroupMember?,
) : GroupItem {
    override val key: String get() = "s-" + (member?.machineId ?: "") + "-" + message.id.ifEmpty { turn }
    override val tsMs: Long get() = message.tsMs
}

/** A Mellow passed the word to another one. */
data class GroupHandOver(
    val fromId: String,
    val fromName: String,
    val toId: String,
    val toName: String,
    val ofMessage: String,
    override val tsMs: Long,
) : GroupItem {
    override val key: String get() = "h-$ofMessage-$toId"
}

/** An answer to hand over to [to]. */
data class HandOverJob(val answer: GroupSaid, val to: GroupMember)

object GroupChat {
    const val KEY = "mm_group"
    const val MAX_HANDOVERS = 3
    const val MAX_SAID = 6
    private const val MAX_LINE = 400
    private const val MAX_ID = 64
    /** A member who has not answered after this long is no longer shown as thinking. */
    const val THINKING_FOR_MS = 5 * 60_000L

    private fun str(o: JsonObject, k: String): String? =
        (o[k] as? JsonPrimitive)?.takeIf { it.isString }?.content?.trim()?.takeIf { it.isNotEmpty() && it.length <= MAX_ID * 2 }

    /** The group tag of [m], or null when it was not said in a group. */
    fun tagOf(m: Message): GroupTag? {
        val g = (m.payload as? JsonObject)?.get(KEY) as? JsonObject ?: return null
        val id = str(g, "id")?.takeIf { it.length <= MAX_ID } ?: return null
        val turn = str(g, "turn")?.takeIf { it.length <= MAX_ID } ?: return null
        val to = (g["to"] as? JsonArray).orEmpty().mapNotNull { (it as? JsonPrimitive)?.takeIf { p -> p.isString }?.content }.take(32)
        return GroupTag(id, turn, to, str(g, "handover_of"), str(g, "handover_from"))
    }

    /** What the Mellows should know of the group with this line. */
    fun payload(
        group: MellowGroup,
        members: List<GroupMember>,
        turn: String,
        to: List<GroupMember>,
        said: List<Pair<String, String>>,
        handoverOf: String? = null,
        handoverFrom: GroupMember? = null,
    ): JsonObject = buildJsonObject {
        putJsonObject(KEY) {
            put("id", group.id)
            put("name", group.name)
            put("turn", turn)
            putJsonArray("members") {
                members.forEach { m -> addJsonObject { put("id", m.id); put("name", m.name) } }
            }
            putJsonArray("to") { if (to.size < members.size) to.forEach { add(it.id) } }
            putJsonArray("said") {
                said.takeLast(MAX_SAID).forEach { (who, text) ->
                    addJsonObject { put("who", who); put("text", text.take(MAX_LINE)) }
                }
            }
            if (handoverOf != null) put("handover_of", handoverOf)
            if (handoverFrom != null) {
                put("handover_from", handoverFrom.id)
                put("handover_from_name", handoverFrom.name)
            }
        }
    }

    /** Members named with `@Name` in [text] (whole name, any case), except [except]. */
    fun mentions(text: String, members: List<GroupMember>, except: String? = null): List<GroupMember> =
        members.filter { m ->
            !m.id.equals(except, ignoreCase = true) &&
                Regex("(^|[^\\p{L}\\p{N}_])@" + Regex.escape(m.name) + "(?![\\p{L}\\p{N}_])", RegexOption.IGNORE_CASE)
                    .containsMatchIn(text)
        }

    /** Who a line of the person is for: the members it names with `@`, else everyone. */
    fun addressees(text: String, members: List<GroupMember>): List<GroupMember> =
        mentions(text, members).ifEmpty { members }

    /**
     * Whether a message of a one-to-one conversation belongs to a group:
     * said in a group, or an answer to such a line.
     */
    fun isGroupTraffic(history: List<Message>): (Message) -> Boolean {
        val tagged = history.filter { tagOf(it) != null }.map { it.id }.filter { it.isNotEmpty() }.toSet()
        return { m -> tagOf(m) != null || (m.parentId != null && m.parentId in tagged) }
    }

    /** The thread of [group]: the person's lines once, every answer, every hand-over; oldest first. */
    fun thread(group: MellowGroup, members: List<GroupMember>, read: List<MemberMessage>, dir: Directory): List<GroupItem> {
        val byId = members.associateBy { it.id.lowercase() }
        // Lines said in this group, per machine (ids are per hub).
        val tagged = HashMap<String, GroupTag>()
        for (r in read) {
            val t = tagOf(r.message) ?: continue
            if (t.groupId == group.id && r.message.id.isNotEmpty()) tagged[r.member.machineId + "|" + r.message.id] = t
        }
        val items = ArrayList<GroupItem>()
        val turns = HashSet<String>()
        val seen = HashSet<String>()
        for (r in read.sortedWith(compareBy<MemberMessage> { it.message.timestampMs }.thenBy { it.message.id })) {
            val m = r.message
            if (m.id.isNotEmpty() && !seen.add(r.member.machineId + "|" + m.id)) continue
            val t = tagOf(m)
            if (t != null) {
                if (t.groupId != group.id) continue
                val ho = t.handoverOf
                if (ho != null) {
                    val from = t.handoverFrom?.let { byId[it.lowercase()] }
                    items += GroupHandOver(
                        fromId = t.handoverFrom ?: "",
                        fromName = from?.name ?: t.handoverFrom?.let { dir.nameOf(it) } ?: "",
                        toId = r.member.id,
                        toName = r.member.name,
                        ofMessage = ho,
                        tsMs = m.timestampMs,
                    )
                } else if (turns.add(t.turn)) {
                    items += GroupSaid(
                        message = Chat.toChat(m, r.member.me, dir),
                        turn = t.turn,
                        toNames = t.to.mapNotNull { byId[it.lowercase()]?.name },
                        member = null,
                    )
                }
                continue
            }
            val parent = m.parentId ?: continue
            val pt = tagged[r.member.machineId + "|" + parent] ?: continue
            if (!m.from.equals(r.member.id, ignoreCase = true)) continue
            items += GroupSaid(Chat.toChat(m, r.member.me, dir), pt.turn, emptyList(), r.member)
        }
        return items.sortedWith(compareBy<GroupItem> { it.tsMs }.thenBy { it.key })
    }

    /** The last lines of the thread, as `who: text`, for the Mellows' context. */
    fun said(items: List<GroupItem>, myName: String = "Owner"): List<Pair<String, String>> =
        items.filterIsInstance<GroupSaid>().takeLast(MAX_SAID).map { s ->
            val who = s.member?.name ?: myName
            who to Chat.preview(s.message)
        }

    /** Answers that name another member and were not handed over yet (bounded per turn). */
    fun handOvers(items: List<GroupItem>, members: List<GroupMember>): List<HandOverJob> {
        val done = items.filterIsInstance<GroupHandOver>().map { it.ofMessage + "|" + it.toId.lowercase() }.toMutableSet()
        val perTurn = HashMap<String, Int>()
        items.filterIsInstance<GroupHandOver>().forEach { h ->
            val turn = items.filterIsInstance<GroupSaid>().firstOrNull { it.message.id == h.ofMessage }?.turn ?: return@forEach
            perTurn[turn] = (perTurn[turn] ?: 0) + 1
        }
        val jobs = ArrayList<HandOverJob>()
        for (s in items.filterIsInstance<GroupSaid>()) {
            val from = s.member ?: continue
            if (s.message.id.isEmpty()) continue
            for (to in mentions(s.message.body, members, except = from.id)) {
                if (!done.add(s.message.id + "|" + to.id.lowercase())) continue
                val n = perTurn[s.turn] ?: 0
                if (n >= MAX_HANDOVERS) break
                perTurn[s.turn] = n + 1
                jobs += HandOverJob(s, to)
            }
        }
        return jobs
    }

    /**
     * Members asked lately who have not answered yet: those the person's
     * last line was for, and those something was just handed over to.
     */
    fun waitingOn(items: List<GroupItem>, members: List<GroupMember>, nowMs: Long): List<GroupMember> {
        val answered = { memberId: String, afterMs: Long ->
            items.any { it is GroupSaid && it.member?.id.equals(memberId, true) && it.tsMs >= afterMs }
        }
        val out = LinkedHashMap<String, GroupMember>()
        val last = items.lastOrNull { it is GroupSaid && it.member == null } as? GroupSaid
        if (last != null && nowMs - last.tsMs < THINKING_FOR_MS) {
            val ids = last.toNames.let { names -> if (names.isEmpty()) members else members.filter { it.name in names } }
            ids.filter { !answered(it.id, last.tsMs) }.forEach { out[it.id.lowercase()] = it }
        }
        for (h in items.filterIsInstance<GroupHandOver>()) {
            if (nowMs - h.tsMs >= THINKING_FOR_MS) continue
            val m = members.firstOrNull { it.id.equals(h.toId, true) } ?: continue
            if (!answered(m.id, h.tsMs)) out[m.id.lowercase()] = m
        }
        return out.values.toList()
    }

    /** The words a Mellow gets when another one hands over to it. */
    fun handOverText(answer: GroupSaid): String =
        "${answer.member?.name ?: "Another Mellow"} handed this over to you:\n\n${answer.message.body.take(4_000)}"
}
