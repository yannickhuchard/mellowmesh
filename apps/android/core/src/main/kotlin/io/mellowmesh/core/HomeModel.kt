package io.mellowmesh.core

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.doubleOrNull

/** Presence as derived by app-core `presence.rs`. */
enum class Presence { ONLINE, BUSY, IDLE, OFFLINE }

/** A helper's mood (design language §2): how the clay blob looks. */
enum class Mood { IDLE, THINKING, WORKING, WAITING, DONE, ASLEEP }

/** Places on the Plaza map. */
enum class Place { HELPERS, MEETING_ROOMS, CAFE, TEAMS, OFFICES, TODO_DESK, SHOPS, HELP_DESK }

data class HelperCard(
    val id: String,
    val name: String,
    val owner: String,
    val isMine: Boolean,
    val capabilities: List<String>,
    val presence: Presence,
    val mood: Mood,
    /** 0..1 when working on a task with reported progress. */
    val progress: Float?,
    val currentTask: String?,
    val accent: Accent,
    val conversationId: String?,
    val lastSeenMs: Long?,
) {
    /** Plain-language line under the helper's name. */
    val statusLine: String
        get() = when (mood) {
            Mood.WAITING -> "Needs you"
            Mood.WORKING -> currentTask?.let { "Working on: $it" } ?: "Working"
            Mood.THINKING -> "Thinking…"
            Mood.DONE -> "Just finished"
            Mood.ASLEEP -> "Resting"
            Mood.IDLE -> "Ready"
        }
}

data class NetworkCard(
    val name: String,
    val isOpen: Boolean,
    val spaces: Int,
    val teams: Int,
    val openSpaces: Int,
)

data class DecisionPreview(val ifYes: String, val ifNotNow: String, val undo: String)

data class HomeModel(
    val greeting: String,
    val me: String,
    val network: NetworkCard?,
    val helpers: List<HelperCard>,
    val pendingDecisions: List<Decision>,
    val conversations: List<ConversationSummary>,
    val activeTasks: List<Task>,
    val badges: Map<Place, Int>,
    val nextStop: Place,
    val generatedMs: Long,
) {
    val needsYou: Int get() = pendingDecisions.size
}

/** Inputs for [HomeModelBuilder.build]; all pure data. */
data class HomeInputs(
    val me: String,
    val agents: List<AgentRegistration>,
    val tasks: List<Task>,
    val decisions: List<Decision>,
    val history: List<Message>,
    val network: NetworkOverview? = null,
    val readMarkers: Map<String, Long> = emptyMap(),
    val nowMs: Long,
    /** Local hour of day (0..23) for the greeting. */
    val hour: Int = 12,
    val displayName: String? = null,
)

object PresenceRules {
    const val ONLINE_WINDOW_MS = 2 * 60 * 1000L
    const val IDLE_WINDOW_MS = 30 * 60 * 1000L

    fun fromStatus(status: String): Presence? = when (status.trim().lowercase()) {
        "available", "online", "ready", "active" -> Presence.ONLINE
        "busy", "working" -> Presence.BUSY
        "idle", "away" -> Presence.IDLE
        "offline", "stopped", "down" -> Presence.OFFLINE
        else -> null
    }

    fun derive(nowMs: Long, lastSeenMs: Long?, hasActiveClaim: Boolean, explicit: Pair<Presence, Long>?): Presence {
        if (explicit != null && explicit.first == Presence.OFFLINE && (lastSeenMs == null || explicit.second >= lastSeenMs)) {
            return Presence.OFFLINE
        }
        if (hasActiveClaim) return Presence.BUSY
        val age = lastSeenMs?.let { nowMs - it } ?: return Presence.OFFLINE
        return when {
            age <= ONLINE_WINDOW_MS -> if (explicit?.first == Presence.BUSY) Presence.BUSY else Presence.ONLINE
            age <= IDLE_WINDOW_MS -> Presence.IDLE
            else -> Presence.OFFLINE
        }
    }
}

object HomeModelBuilder {
    /** A helper that finished within this window looks happy (^ ^). */
    const val DONE_WINDOW_MS = 15 * 60 * 1000L
    /** A message waiting in the inbox this recent means "thinking". */
    const val THINKING_WINDOW_MS = 5 * 60 * 1000L

    fun greeting(hour: Int, name: String?): String {
        val part = when (hour) {
            in 5..11 -> "Good morning"
            in 12..17 -> "Good afternoon"
            else -> "Good evening"
        }
        return if (name.isNullOrBlank()) part else "$part, ${name.replaceFirstChar { it.uppercase() }}"
    }

    fun hasActiveClaim(t: Task, agentId: String, nowMs: Long): Boolean {
        val claimed = t.status.lowercase() in setOf(TaskStatus.CLAIMED, TaskStatus.IN_PROGRESS)
        val holder = Identity.samePrincipal(t.claimedBy, agentId)
        val exp = Time.parseMs(t.claimExpiresAt)
        return claimed && holder && (exp == null || exp > nowMs)
    }

    /** Progress 0..1 from a `_task.<id>.progress` payload (`progress` 0..1 or `percent` 0..100). */
    fun progressOf(msg: Message): Float? {
        val p = msg.payload as? JsonObject ?: return null
        (p["progress"] as? JsonPrimitive)?.doubleOrNull?.let { if (it in 0.0..1.0) return it.toFloat() }
        (p["percent"] as? JsonPrimitive)?.doubleOrNull?.let { if (it in 0.0..100.0) return (it / 100.0).toFloat() }
        return null
    }

    /** Plain "what happens if" sentences for a decision (design: preview before consequence). */
    fun preview(d: Decision, helperName: String): DecisionPreview {
        val yes = d.options.firstOrNull { it.outcome == DecisionOutcome.APPROVE } ?: d.options.firstOrNull()
        val ifYes = when {
            yes == null -> "$helperName goes ahead."
            yes.pros.isNotEmpty() -> "$helperName goes ahead with “${Sanitize.line(yes.label)}”. ${yes.pros.joinToString(" ") { Sanitize.line(it).trimEnd('.') + "." }}"
            else -> "$helperName goes ahead with “${Sanitize.line(yes.label)}”."
        }
        val ifNotNow = "Nothing happens. The request waits safely at your To-do desk."
        val risks = yes?.cons?.filter { it.isNotBlank() }.orEmpty()
        val undo = if (risks.isEmpty()) {
            "You can ask $helperName to undo it in the chat."
        } else {
            "Keep in mind: ${risks.joinToString(" ") { Sanitize.line(it).trimEnd('.') + "." }}"
        }
        return DecisionPreview(ifYes.trim(), ifNotNow, undo)
    }

    fun build(inp: HomeInputs): HomeModel {
        val dir = Directory.of(inp.agents)
        val now = inp.nowMs

        // Last seen, explicit presence, inbox activity, latest progress per task.
        val lastSeen = HashMap<String, Long>()
        val explicit = HashMap<String, Pair<Presence, Long>>()
        val lastInbox = HashMap<String, Long>()
        val lastFromAgent = HashMap<String, Long>()
        val progressByTask = HashMap<String, Pair<Float, Long>>()
        val pathToAgent = inp.agents.mapNotNull { a -> Identity.agentTopicPath(a.id)?.let { it to a.id.lowercase() } }.toMap()

        for (m in inp.history) {
            val t = m.timestampMs
            if (PrincipalKind.of(m.from) == PrincipalKind.AGENT) {
                val k = m.from.lowercase()
                lastSeen[k] = maxOf(lastSeen[k] ?: t, t)
                lastFromAgent[k] = maxOf(lastFromAgent[k] ?: t, t)
            }
            val lower = m.topic.lowercase()
            val segs = lower.split('.')
            if (segs.size >= 3 && segs[0] == "_agent") {
                val path = segs.subList(1, segs.size - 1).joinToString(".")
                val leaf = segs.last()
                val id = pathToAgent[path] ?: Identity.agentIdFromPath(segs.subList(1, segs.size - 1)).lowercase()
                if (leaf == "inbox") {
                    if (!Identity.samePrincipal(m.from, id)) lastInbox[id] = maxOf(lastInbox[id] ?: t, t)
                } else {
                    lastSeen[id] = maxOf(lastSeen[id] ?: t, t)
                }
                val status = ((m.payload as? JsonObject)?.get("status") as? JsonPrimitive)?.takeIf { it.isString }?.content
                val p = when (leaf) {
                    "heartbeat" -> status?.let { PresenceRules.fromStatus(it) } ?: Presence.ONLINE
                    "status" -> status?.let { PresenceRules.fromStatus(it) }
                    else -> null
                }
                if (p != null && t >= (explicit[id]?.second ?: Long.MIN_VALUE)) explicit[id] = p to t
            }
            if (segs.size == 3 && segs[0] == "_task" && segs[2] == "progress") {
                progressOf(m)?.let { pr ->
                    if (t >= (progressByTask[segs[1]]?.second ?: Long.MIN_VALUE)) progressByTask[segs[1]] = pr to t
                }
            }
        }

        val pending = inp.decisions.filter { it.isPending }
            .sortedWith(compareByDescending<Decision> { Identity.samePrincipal(it.requiredDecider, inp.me) }.thenBy { it.id })
        val waitingAgents = pending.map { it.createdBy.lowercase() }.toSet()

        val helpers = inp.agents.map { a ->
            val key = a.id.lowercase()
            val claimed = inp.tasks.filter { hasActiveClaim(it, a.id, now) }
            val seen = lastSeen[key]
            val presence = PresenceRules.derive(now, seen, claimed.isNotEmpty(), explicit[key])
            val recentlyDone = inp.tasks.any { t ->
                t.status.lowercase() == TaskStatus.COMPLETED && Identity.samePrincipal(t.claimedBy, a.id) &&
                    (Time.parseMs(t.completedAt)?.let { now - it in 0..DONE_WINDOW_MS } ?: false)
            }
            val inbox = lastInbox[key]
            val thinking = inbox != null && now - inbox <= THINKING_WINDOW_MS && inbox > (lastFromAgent[key] ?: Long.MIN_VALUE)
            val mood = when {
                key in waitingAgents -> Mood.WAITING
                claimed.isNotEmpty() -> Mood.WORKING
                presence == Presence.OFFLINE && !thinking -> Mood.ASLEEP
                thinking -> Mood.THINKING
                recentlyDone -> Mood.DONE
                presence == Presence.BUSY -> Mood.WORKING
                else -> Mood.IDLE
            }
            val task = claimed.firstOrNull()
            HelperCard(
                id = a.id,
                name = dir.nameOf(a.id),
                owner = a.owner,
                isMine = Identity.samePrincipal(a.owner, inp.me),
                capabilities = a.capabilities.map { Sanitize.line(it) },
                presence = presence,
                mood = mood,
                progress = task?.let { progressByTask[it.id.lowercase()]?.first },
                currentTask = task?.let { Sanitize.line(it.title) },
                accent = Identity.accentOf(a.id),
                conversationId = Conversations.agentConversationId(a.id),
                lastSeenMs = seen,
            )
        }.sortedWith(
            compareByDescending<HelperCard> { it.isMine }
                .thenBy { moodRank(it.mood) }
                .thenBy { it.name.lowercase() }
                .thenBy { it.id },
        )

        val teams = inp.network?.teams.orEmpty()
        val conversations = Chat.summaries(
            inp.history, inp.me, dir,
            myAgentIds = helpers.filter { it.isMine }.map { it.id },
            teams = teams,
            readMarkers = inp.readMarkers,
        )

        val activeTasks = inp.tasks.filter { !TaskStatus.isTerminal(it.status) }
        val unreadBy = { kinds: Set<ConversationKind> -> conversations.filter { it.kind in kinds }.sumOf { it.unread } }
        val badges = buildMap {
            put(Place.TODO_DESK, pending.size)
            put(Place.HELPERS, unreadBy(setOf(ConversationKind.AGENT)))
            put(Place.MEETING_ROOMS, unreadBy(setOf(ConversationKind.SQUAD)))
            put(Place.CAFE, unreadBy(setOf(ConversationKind.PERSON, ConversationKind.GROUP)))
            put(Place.TEAMS, unreadBy(setOf(ConversationKind.TEAM)))
            put(Place.OFFICES, unreadBy(setOf(ConversationKind.ORGANIZATION)))
            put(Place.SHOPS, 0)
            put(Place.HELP_DESK, 0)
        }
        val next = when {
            pending.isNotEmpty() -> Place.TODO_DESK
            (badges[Place.HELPERS] ?: 0) > 0 -> Place.HELPERS
            (badges[Place.CAFE] ?: 0) > 0 -> Place.CAFE
            (badges[Place.TEAMS] ?: 0) > 0 -> Place.TEAMS
            (badges[Place.MEETING_ROOMS] ?: 0) > 0 -> Place.MEETING_ROOMS
            (badges[Place.OFFICES] ?: 0) > 0 -> Place.OFFICES
            helpers.none { it.isMine } -> Place.SHOPS
            else -> Place.HELPERS
        }

        val networkCard = inp.network?.let { n ->
            NetworkCard(
                name = Sanitize.line(n.network.name).ifBlank { "My network" },
                isOpen = n.network.defaultAccess == Access.OPEN,
                spaces = n.spaces.size,
                teams = n.teams.size,
                openSpaces = n.spaces.count { it.access == Access.OPEN },
            )
        }

        return HomeModel(
            greeting = greeting(inp.hour, inp.displayName ?: Identity.shortName(inp.me).takeIf { inp.me.isNotBlank() }),
            me = inp.me,
            network = networkCard,
            helpers = helpers,
            pendingDecisions = pending,
            conversations = conversations,
            activeTasks = activeTasks,
            badges = badges,
            nextStop = next,
            generatedMs = now,
        )
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
