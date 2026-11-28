package io.mellowmesh.core

import kotlinx.serialization.Serializable

/**
 * Chats as one conversation space over every network: who is there (every
 * Mellow of every machine, by network segment), the person's own groups,
 * and the conversations of every machine, newest first. Talking to a
 * Mellow should feel like talking to a person — wherever it runs.
 */

/** A group the person made: a name and some Mellows, from any network. */
@Serializable
data class MellowGroup(
    val id: String,
    val name: String,
    /** Mellow ids (`agent://…`), in the order they were added. */
    val members: List<String> = emptyList(),
    /**
     * The machine (saved machine id) that keeps the group going while the
     * phone sleeps; null while only the phone runs it.
     */
    val keptOn: String? = null,
)

/** One network segment (a network, or a machine when its network has no name). */
data class NetworkSegment(
    val key: String,
    val name: String,
    /** Stable colour slot, 0..5. */
    val slot: Int,
    val mellows: Int,
    /** At least one of its machines answers. */
    val live: Boolean,
)

/** A conversation and the machine it lives on. */
data class PlacedConversation(
    val summary: ConversationSummary,
    val machineId: String,
    val segmentKey: String,
)

data class GroupView(val group: MellowGroup, val members: List<PlacedHelper>) {
    /** "Coder is working" — the most telling member first. */
    val line: String
        get() = members.firstOrNull { it.card.mood == Mood.WAITING }?.let { "${it.card.name} needs you" }
            ?: members.firstOrNull { it.card.mood == Mood.WORKING }?.let { "${it.card.name} is working" }
            ?: members.joinToString(" · ") { it.card.name }.ifEmpty { "No Mellow yet" }
}

data class ChatSpaceModel(
    val segments: List<NetworkSegment>,
    val groups: List<GroupView>,
    /** Every Mellow of the selected segment (or all), what needs the person first. */
    val everyone: List<PlacedHelper>,
    val conversations: List<PlacedConversation>,
    val total: Int,
)

object ChatSpace {
    const val MAX_GROUP_NAME = 32
    const val MAX_GROUPS = 24

    /** The segment a machine belongs to: its network's name, else the machine. */
    fun segmentKey(networkName: String?, machineLabel: String): String =
        networkName?.trim()?.takeIf { it.isNotEmpty() }?.lowercase() ?: "machine:${machineLabel.lowercase()}"

    fun build(
        everywhere: EverywhereModel,
        conversations: List<PlacedConversation>,
        groups: List<MellowGroup>,
        selected: String? = null,
    ): ChatSpaceModel {
        val keyOf = everywhere.machines.associate { it.machineId to segmentKey(it.networkName, it.label) }
        val nameOf = LinkedHashMap<String, String>()
        val live = HashMap<String, Boolean>()
        for (m in everywhere.machines) {
            if (m.sameAs != null) continue
            val k = keyOf.getValue(m.machineId)
            nameOf.putIfAbsent(k, m.networkName ?: m.label)
            live[k] = (live[k] ?: false) || m.health == MachineHealth.ANSWERS
        }
        val segments = nameOf.entries.mapIndexed { i, (k, name) ->
            NetworkSegment(
                key = k,
                name = name,
                slot = i % 6,
                mellows = everywhere.helpers.count { keyOf[it.machineId] == k },
                live = live[k] == true,
            )
        }
        val sel = selected?.takeIf { s -> segments.any { it.key == s } }
        val inSel = { machineId: String -> sel == null || keyOf[machineId] == sel }
        val byId = everywhere.helpers.associateBy { it.card.id.lowercase() }
        val views = groups.map { g -> GroupView(g, g.members.mapNotNull { byId[it.lowercase()] }) }
        // A conversation reached through two addresses of one hub is listed
        // once; one with no message yet is not listed (its Mellow is in
        // Everyone, one tap away — no double entry).
        val convs = conversations
            .filter { inSel(it.machineId) && it.summary.lastActivityMs > 0 }
            .sortedByDescending { it.summary.lastActivityMs }
            .distinctBy { it.summary.id.lowercase() + "|" + it.segmentKey }
        return ChatSpaceModel(
            segments = segments,
            groups = views,
            everyone = everywhere.helpers.filter { inSel(it.machineId) },
            conversations = convs,
            total = everywhere.helpers.size,
        )
    }

    /** A clean group name, or null when it is not usable. */
    fun cleanName(raw: String): String? =
        Sanitize.line(raw).trim().take(MAX_GROUP_NAME).takeIf { it.isNotEmpty() }

    /** Add [agentId] to [group] (once). */
    fun add(group: MellowGroup, agentId: String): MellowGroup =
        if (group.members.any { it.equals(agentId, ignoreCase = true) }) group else group.copy(members = group.members + agentId)

    fun remove(group: MellowGroup, agentId: String): MellowGroup =
        group.copy(members = group.members.filterNot { it.equals(agentId, ignoreCase = true) })
}
