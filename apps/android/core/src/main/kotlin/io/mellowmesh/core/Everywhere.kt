package io.mellowmesh.core

/**
 * "Everywhere": every Mellow of every paired machine in one view. A person
 * thinks in *my Mellows*, not in *which computer hosts them*; and a machine
 * that fails must never hide the ones that work.
 */

/** How a paired machine is doing, in four honest states (and "not asked yet"). */
enum class MachineHealth {
    /** Its hub answered: its Mellows are live. */
    ANSWERS,

    /** Out of reach (off, asleep, another network): notes wait on the phone. */
    RESTING,

    /** Something answers at the address, but not a hub (a closed tunnel). */
    ADDRESS_LOST,

    /** The hub no longer knows this phone's key. */
    KEY_REFUSED,

    /** Not asked yet. */
    UNKNOWN;

    /** A trouble the person has to fix (pair again, or remove). */
    val needsFix: Boolean get() = this == ADDRESS_LOST || this == KEY_REFUSED

    companion object {
        /**
         * What a failure on the machine's most basic read (its Mellows) says
         * about the machine. Null: the failure says nothing about it.
         */
        fun ofFailure(e: Throwable): MachineHealth? = when ((e as? MeshException)?.kind) {
            MeshException.Kind.NETWORK, MeshException.Kind.SERVER -> RESTING
            // Every hub has its Mellows list: "not found" or a page that is
            // not the hub's JSON means the address leads somewhere else.
            MeshException.Kind.NOT_FOUND, MeshException.Kind.PROTOCOL -> ADDRESS_LOST
            MeshException.Kind.UNAUTHORIZED -> KEY_REFUSED
            else -> null
        }
    }
}

/** One machine as Everywhere sees it: its Plaza when it answered, else what the phone remembers. */
data class MachineHome(
    val machineId: String,
    val label: String,
    val networkName: String?,
    val health: MachineHealth,
    /** Built from the machine's own data when it answered. */
    val home: HomeModel?,
    /** The Mellows last seen there (used when [home] is null). */
    val cachedAgents: List<AgentRegistration>,
    /** Who this phone is on that machine. */
    val me: String,
)

/** A Mellow and the machine it lives on. */
data class PlacedHelper(
    val card: HelperCard,
    val machineId: String,
    val machineLabel: String,
    val networkName: String?,
    val health: MachineHealth,
) {
    /** The small tag next to the Mellow: its machine, and its network when there are several. */
    fun tag(withNetwork: Boolean): String =
        if (withNetwork && !networkName.isNullOrBlank()) "$networkName · $machineLabel" else machineLabel
}

data class MachineSummary(
    val machineId: String,
    val label: String,
    val networkName: String?,
    val health: MachineHealth,
    val mellows: Int,
    val needsYou: Int,
    /** Another address of a machine already listed: all its Mellows are shown there. */
    val sameAs: String? = null,
)

data class EverywhereModel(
    val machines: List<MachineSummary>,
    /** Every Mellow once, what needs the person first; yours before the others'. */
    val helpers: List<PlacedHelper>,
    /** Requests waiting for the person, over every machine (each counted once). */
    val needsYou: Int,
    /** More than one network: tags name the network too. */
    val showNetworks: Boolean,
) {
    /** The machines where something waits for the person. */
    val needsYouOn: List<MachineSummary> get() = machines.filter { it.needsYou > 0 }

    /** The list narrowed to one machine (null: all of them). */
    fun only(machineId: String?): List<PlacedHelper> =
        if (machineId == null) helpers else helpers.filter { it.machineId == machineId }

    fun mellowsOn(machineId: String): Int = helpers.count { it.machineId == machineId }
}

object Everywhere {
    fun build(homes: List<MachineHome>): EverywhereModel {
        // A Mellow reached through two addresses (LAN and tunnel to the same
        // hub, or peered hubs) is listed once, where it answers.
        val ordered = homes.sortedBy { healthRank(it.health) }
        val seen = HashSet<String>()
        val placed = mutableListOf<PlacedHelper>()
        val decisionIds = HashSet<String>()
        val needsPerMachine = HashMap<String, Int>()
        for (m in ordered) {
            val cards = m.home?.helpers ?: m.cachedAgents.map { asleep(it, m.me) }
            for (c in cards) {
                if (seen.add(c.id.lowercase())) {
                    placed += PlacedHelper(c, m.machineId, m.label, m.networkName?.takeIf { it.isNotBlank() }, m.health)
                }
            }
            val pending = m.home?.pendingDecisions.orEmpty()
            needsPerMachine[m.machineId] = pending.count { decisionIds.add(it.id) }
        }
        val helpers = placed.sortedWith(
            compareByDescending<PlacedHelper> { it.card.isMine }
                .thenBy { moodRank(it.card.mood) }
                .thenBy { healthRank(it.health) }
                .thenBy { it.card.name.lowercase() }
                .thenBy { it.card.id },
        )
        val placedOn = placed.associate { it.card.id.lowercase() to it.machineId }
        val labels = homes.associate { it.machineId to it.label }
        val machines = homes.map { m ->
            // An answering address whose Mellows are all listed under one
            // other machine is that machine reached another way.
            val ids = m.home?.helpers.orEmpty().map { it.id.lowercase() }
            val elsewhere = ids.mapNotNull { placedOn[it] }.toSet()
            val sameAs = if (ids.isNotEmpty() && elsewhere.size == 1 && m.machineId !in elsewhere) labels[elsewhere.first()] else null
            MachineSummary(
                machineId = m.machineId,
                label = m.label,
                networkName = m.networkName?.takeIf { it.isNotBlank() },
                health = m.health,
                mellows = helpers.count { it.machineId == m.machineId },
                needsYou = needsPerMachine[m.machineId] ?: 0,
                sameAs = sameAs,
            )
        }
        val networks = homes.mapNotNull { it.networkName?.trim()?.lowercase()?.takeIf { n -> n.isNotEmpty() } }.toSet()
        return EverywhereModel(
            machines = machines,
            helpers = helpers,
            needsYou = needsPerMachine.values.sum(),
            showNetworks = networks.size > 1,
        )
    }

    /** A Mellow of a machine that did not answer: listed, asleep, still callable. */
    fun asleep(a: AgentRegistration, me: String): HelperCard = HelperCard(
        id = a.id,
        name = Directory.of(listOf(a)).nameOf(a.id),
        owner = a.owner,
        isMine = Identity.samePrincipal(a.owner, me),
        capabilities = a.capabilities.map { Sanitize.line(it) },
        presence = Presence.OFFLINE,
        mood = Mood.ASLEEP,
        progress = null,
        currentTask = null,
        accent = Identity.accentOf(a.id),
        conversationId = Conversations.agentConversationId(a.id),
        lastSeenMs = null,
    )

    private fun healthRank(h: MachineHealth): Int = when (h) {
        MachineHealth.ANSWERS -> 0
        MachineHealth.UNKNOWN -> 1
        MachineHealth.RESTING -> 2
        MachineHealth.ADDRESS_LOST -> 3
        MachineHealth.KEY_REFUSED -> 4
    }

    private fun moodRank(m: Mood): Int = when (m) {
        Mood.WAITING -> 0
        Mood.WORKING -> 1
        Mood.THINKING -> 2
        Mood.DONE -> 3
        Mood.IDLE -> 4
        Mood.ASLEEP -> 5
    }
}
