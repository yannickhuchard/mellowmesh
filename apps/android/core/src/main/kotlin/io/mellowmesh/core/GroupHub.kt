package io.mellowmesh.core

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.addJsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject

/**
 * A group kept by a hub the person owns (`/groups`): the hub says each line
 * to every member, keeps it for a sleeping machine, watches the answers and
 * hands over — so the group goes on while the phone sleeps. Every app reads
 * the same thread from that hub.
 */

@Serializable
data class HubNamed(val id: String, val name: String = "")

@Serializable
data class HubMember(
    val id: String,
    val name: String,
    val conversation: String = "",
    val machine: String? = null,
    val here: Boolean = true,
    val reachable: Boolean? = null,
)

@Serializable
data class HubGroup(
    val id: String,
    val name: String,
    val members: List<HubMember> = emptyList(),
    val topic: String = "",
)

@Serializable
data class HubItem(
    val id: String,
    /** `said`, `answer`, `handover`, `delivered` or `gave_up`. */
    val kind: String,
    val ts: String = "",
    val from: String = "",
    val text: String = "",
    val turn: String? = null,
    val to: List<HubNamed> = emptyList(),
    val member: HubNamed? = null,
    val of: String? = null,
)

@Serializable
data class HubPending(
    val turn: String = "",
    val member: HubNamed,
    val machine: String? = null,
    /** `thinking` (said, no answer yet) or `waiting` (its machine is out of reach). */
    val state: String,
    val since: String = "",
)

@Serializable
data class HubThread(
    val group: HubGroup,
    val items: List<HubItem> = emptyList(),
    val pending: List<HubPending> = emptyList(),
)

/** A word from the hub in the thread: delivered late, or not delivered. */
data class GroupNote(val id: String, val text: String, val late: Boolean, override val tsMs: Long) : GroupItem {
    override val key: String get() = "n-$id"
}

/** A member whose machine is asleep: the line waits for it. */
data class GroupAway(val member: GroupMember, val machine: String?)

/** A member to keep, with the pass the keeping hub reaches it with (null: it lives there). */
data class KeptMember(val member: GroupMember, val passUrl: String?, val passToken: String?, val machine: String?)

object GroupHub {
    /** The thread topic of a kept group on its hub. */
    fun topic(groupId: String): String = "_group.${groupId.lowercase()}"

    /** The machine that should keep a group: the owned one with the most members, then the one in use. */
    fun chooseKeeper(members: List<GroupMember>, owned: List<String>, active: String?): String? {
        if (owned.isEmpty()) return null
        val count = members.groupingBy { it.machineId }.eachCount()
        return owned.sortedWith(
            compareByDescending<String> { count[it] ?: 0 }.thenByDescending { it == active },
        ).first()
    }

    /** The body of `POST /groups`. */
    fun keepBody(group: MellowGroup, members: List<KeptMember>): JsonObject = buildJsonObject {
        put("id", group.id.lowercase())
        put("name", group.name)
        putJsonArray("members") {
            members.forEach { k ->
                addJsonObject {
                    put("id", k.member.id)
                    put("name", k.member.name)
                    put("conversation", k.member.conversationId)
                    k.machine?.let { put("machine", it) }
                    if (k.passUrl != null && k.passToken != null) {
                        putJsonObject("pass") {
                            put("url", k.passUrl)
                            put("token", k.passToken)
                        }
                    }
                }
            }
        }
    }

    private fun member(id: String, name: String, members: List<GroupMember>): GroupMember =
        members.firstOrNull { it.id.equals(id, ignoreCase = true) }
            ?: GroupMember(id, name.ifEmpty { Identity.shortName(id) }, "", "", "")

    /** The hub's thread as the screen draws it. [me] is the person on the keeping machine. */
    fun items(thread: HubThread, members: List<GroupMember>, me: String, dir: Directory): List<GroupItem> {
        val out = ArrayList<GroupItem>()
        for (i in thread.items) {
            val ts = Time.parseMs(i.ts) ?: 0L
            when (i.kind) {
                "said" -> out += GroupSaid(
                    message = Chat.toChat(Message(id = i.id, topic = thread.group.topic, from = i.from, timestamp = i.ts, body = i.text), me, dir),
                    turn = i.turn ?: i.id,
                    toNames = i.to.map { t -> member(t.id, t.name, members).name },
                    member = null,
                )
                "answer" -> {
                    val m = i.member ?: continue
                    val who = member(m.id, m.name, members)
                    out += GroupSaid(
                        message = Chat.toChat(
                            Message(id = i.id, topic = thread.group.topic, from = m.id, timestamp = i.ts, contentType = "text/markdown", body = i.text),
                            me,
                            dir,
                        ),
                        turn = i.turn ?: i.id,
                        toNames = emptyList(),
                        member = who,
                    )
                }
                "handover" -> {
                    val from = i.member ?: continue
                    val to = i.to.firstOrNull() ?: continue
                    out += GroupHandOver(
                        fromId = from.id,
                        fromName = member(from.id, from.name, members).name,
                        toId = to.id,
                        toName = member(to.id, to.name, members).name,
                        ofMessage = i.of ?: i.id,
                        tsMs = ts,
                    )
                }
                "delivered", "gave_up" -> out += GroupNote(i.id, i.text, late = i.kind == "delivered", tsMs = ts)
            }
        }
        // The hub sends its thread in order; a stable sort keeps an answer
        // before the hand-over it caused, even within one millisecond.
        return out.sortedBy { it.tsMs }
    }

    /** Members the hub said a line to and that are still thinking. */
    fun thinking(thread: HubThread, members: List<GroupMember>): List<GroupMember> =
        thread.pending.filter { it.state == "thinking" }
            .map { member(it.member.id, it.member.name, members) }
            .distinctBy { it.id.lowercase() }

    /** Members whose machine is asleep: their line waits on the hub. */
    fun away(thread: HubThread, members: List<GroupMember>): List<GroupAway> =
        thread.pending.filter { it.state == "waiting" }
            .map { GroupAway(member(it.member.id, it.member.name, members), it.machine) }
            .distinctBy { it.member.id.lowercase() }

    /** Whether the hub already holds a member (then no new pass is needed). */
    fun holds(group: HubGroup?, memberId: String): Boolean =
        group?.members?.any { it.id.equals(memberId, ignoreCase = true) } == true
}
