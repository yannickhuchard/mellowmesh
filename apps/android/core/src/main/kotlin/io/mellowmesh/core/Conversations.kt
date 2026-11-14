package io.mellowmesh.core

/**
 * Conversations derived from topic names (port of app-core `conversation.rs`,
 * plus Team rooms from `docs/network.md`).
 *
 * | Topic                                     | Kind           | Conversation id          |
 * |-------------------------------------------|----------------|--------------------------|
 * | `_agent.<owner>.<name>.inbox` / `.outbox` | Agent          | `_agent.<owner>.<name>`  |
 * | `_project.<name>[.**]`                    | Squad / Group  | `_project.<name>`        |
 * | `_forum.<name>[.**]`                      | Squad / Group  | `_forum.<name>`          |
 * | `_chat.dm.<a>.<b>`                        | Person         | `_chat.dm.<a>.<b>` sorted|
 * | `_chat.group.<name>[.**]`                 | Group          | `_chat.group.<name>`     |
 * | `_team.<id>[.**]`                         | Team           | `_team.<id>`             |
 * | `_org.<org>[.**]`                         | Organization   | `_org.<org>`             |
 */
enum class ConversationKind { AGENT, SQUAD, GROUP, PERSON, TEAM, ORGANIZATION }

data class ConversationRef(
    /** The topic root: the stable id of the conversation. */
    val id: String,
    val kind: ConversationKind,
    /** `agent://<owner>/<name>` for Agent conversations. */
    val agentId: String? = null,
    /** The two human handles of a Person conversation, sorted. */
    val dmHandles: List<String> = emptyList(),
    /** Team id for Team conversations. */
    val teamId: String? = null,
)

object Conversations {
    private fun isWildcard(seg: String) = seg == "*" || seg == "**" || seg == ">"

    /** Map a topic to the conversation it belongs to, or null if it is not chat. */
    fun classify(topic: String): ConversationRef? {
        val lower = topic.trim().lowercase()
        val segs = lower.split('.')
        if (segs.any { it.isEmpty() || isWildcard(it) }) return null
        val head = segs[0]
        return when {
            head == "_agent" && segs.size >= 4 && (segs.last() == "inbox" || segs.last() == "outbox") -> {
                val path = segs.subList(1, segs.size - 1)
                ConversationRef(
                    id = "_agent." + path.joinToString("."),
                    kind = ConversationKind.AGENT,
                    agentId = Identity.agentIdFromPath(path),
                )
            }
            head == "_project" && segs.size >= 2 ->
                ConversationRef("_project.${segs[1]}", ConversationKind.SQUAD)
            head == "_forum" && segs.size >= 2 ->
                ConversationRef("_forum.${segs[1]}", ConversationKind.SQUAD)
            head == "_chat" && segs.size == 4 && segs[1] == "dm" && segs[2] != segs[3] -> {
                val handles = listOf(segs[2], segs[3]).sorted()
                ConversationRef(
                    id = "_chat.dm.${handles[0]}.${handles[1]}",
                    kind = ConversationKind.PERSON,
                    dmHandles = handles,
                )
            }
            head == "_chat" && segs.size >= 3 && segs[1] == "group" ->
                ConversationRef("_chat.group.${segs[2]}", ConversationKind.GROUP)
            head == "_team" && segs.size >= 2 ->
                ConversationRef("_team.${segs[1]}", ConversationKind.TEAM, teamId = segs[1])
            head == "_org" && segs.size >= 2 ->
                ConversationRef("_org.${segs[1]}", ConversationKind.ORGANIZATION)
            else -> null
        }
    }

    /** Validate a conversation id: it must be exactly a conversation root. */
    fun parseId(id: String): ConversationRef? {
        val trimmed = id.trim().lowercase()
        val probe = when {
            trimmed.startsWith("_agent.") -> "$trimmed.inbox"
            else -> trimmed
        }
        val c = classify(probe) ?: return null
        return if (c.id == trimmed) c else null
    }

    /** Server-side history pattern covering a conversation. */
    fun historyPattern(conv: ConversationRef): String = when (conv.kind) {
        ConversationKind.AGENT -> "${conv.id}.*"
        ConversationKind.PERSON -> conv.id
        else -> "${conv.id}.**"
    }

    /** Whether `topic` is chat traffic of `conv` (excludes heartbeats etc.). */
    fun isChatTopic(conv: ConversationRef, topic: String): Boolean = classify(topic)?.id == conv.id

    /** Topic a message the user types into `conv` is published on. */
    fun sendTopic(conv: ConversationRef): String = when (conv.kind) {
        ConversationKind.AGENT -> "${conv.id}.inbox"
        ConversationKind.TEAM -> "${conv.id}.chat"
        else -> conv.id
    }

    /** `_agent.<owner>.<name>` for an agent URI (needs owner + name). */
    fun agentConversationId(agentId: String): String? =
        Identity.agentTopicPath(agentId)?.takeIf { '.' in it }?.let { "_agent.$it" }

    /** Team room conversation id for a team id. */
    fun teamConversationId(teamId: String): String = "_team.${teamId.lowercase()}"

    /** Canonical DM id between two human handles, or null if invalid. */
    fun dmConversationId(a: String, b: String): String? {
        fun clean(h: String): String? {
            val x = h.removePrefix("human://").trim().lowercase()
            val ok = x.isNotEmpty() && x.length <= 64 && x.all { it in 'a'..'z' || it in '0'..'9' || it == '_' || it == '-' }
            return if (ok) x else null
        }
        val x = clean(a) ?: return null
        val y = clean(b) ?: return null
        if (x == y) return null
        val (p, q) = if (x < y) x to y else y to x
        return "_chat.dm.$p.$q"
    }

    /** Squad becomes Group as soon as a human other than me posts in it. */
    fun effectiveKind(conv: ConversationRef, senders: List<String>, me: String): ConversationKind {
        if (conv.kind != ConversationKind.SQUAD) return conv.kind
        val otherHuman = senders.any {
            val k = PrincipalKind.of(it)
            (k == PrincipalKind.HUMAN || k == PrincipalKind.INTERFACE) && !Identity.samePrincipal(it, me)
        }
        return if (otherHuman) ConversationKind.GROUP else ConversationKind.SQUAD
    }

    /** `release-train_v2` → `Release train v2`. */
    fun pretty(name: String): String {
        val spaced = Sanitize.line(name.replace('-', ' ').replace('_', ' '))
        return spaced.replaceFirstChar { it.uppercase() }
    }

    /** Title shown in the conversation list. */
    fun title(conv: ConversationRef, me: String, directory: Directory, teams: Map<String, String> = emptyMap()): String =
        when (conv.kind) {
            ConversationKind.AGENT -> conv.agentId?.let { directory.nameOf(it) } ?: conv.id
            ConversationKind.PERSON -> {
                val mine = Identity.shortName(me).lowercase()
                val other = conv.dmHandles.firstOrNull { it != mine } ?: conv.dmHandles.joinToString(" & ")
                directory.nameOf("human://$other")
            }
            ConversationKind.TEAM -> teams[conv.teamId]?.takeIf { it.isNotBlank() }?.let { Sanitize.line(it) }
                ?: pretty(conv.teamId ?: conv.id)
            else -> pretty(conv.id.substringAfterLast('.'))
        }
}

/** Display names known to the app (agent registry), keyed by lowercase URI. */
class Directory(names: Map<String, String> = emptyMap()) {
    private val names: Map<String, String> =
        names.entries.filter { it.value.isNotBlank() }.associate { it.key.lowercase() to Sanitize.line(it.value.trim()) }

    fun nameOf(id: String): String = names[id.lowercase()] ?: Identity.shortName(id)

    companion object {
        fun of(agents: List<AgentRegistration>): Directory = Directory(agents.associate { it.id to it.name })
    }
}
