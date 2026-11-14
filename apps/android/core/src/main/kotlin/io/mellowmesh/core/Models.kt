package io.mellowmesh.core

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject

/**
 * Wire models of the hub API. Field names follow the Rust structs in
 * `crates/mellowmesh-core` and the network contract in `docs/network.md`
 * exactly; unknown fields are ignored so a newer hub never breaks the app.
 */
val MeshJson: Json = Json {
    ignoreUnknownKeys = true
    explicitNulls = false
    encodeDefaults = true
    coerceInputValues = true
    isLenient = false
}

/** The message envelope (PROTOCOL.md §5). `id` and `timestamp` are hub-assigned. */
@Serializable
data class Message(
    val id: String = "",
    val topic: String,
    val from: String,
    val owner: String? = null,
    /** RFC 3339. The hub replaces it; a valid value is still required on the wire. */
    val timestamp: String = "1970-01-01T00:00:00Z",
    @SerialName("content_type") val contentType: String = "text/plain",
    val body: String = "",
    val headers: Map<String, String>? = null,
    val payload: JsonElement? = null,
    @SerialName("parent_id") val parentId: String? = null,
) {
    /** Milliseconds since the epoch, 0 when the timestamp cannot be parsed. */
    val timestampMs: Long get() = Time.parseMs(timestamp) ?: 0L
}

@Serializable
enum class DecisionOutcome {
    @SerialName("approve") APPROVE,
    @SerialName("reject") REJECT,
    @SerialName("neutral") NEUTRAL,
}

@Serializable
data class DecisionOption(
    val id: String,
    val label: String,
    val pros: List<String> = emptyList(),
    val cons: List<String> = emptyList(),
    val outcome: DecisionOutcome? = null,
)

@Serializable
data class Decision(
    val id: String,
    val title: String = "",
    val question: String = "",
    @SerialName("created_by") val createdBy: String = "",
    @SerialName("required_decider") val requiredDecider: String = "",
    val status: String = "requested",
    val options: List<DecisionOption> = emptyList(),
    @SerialName("response_option_id") val responseOptionId: String? = null,
    @SerialName("response_timestamp") val responseTimestamp: String? = null,
    @SerialName("responded_by") val respondedBy: String? = null,
) {
    /** `requested` and `discussed` still wait for an answer. */
    val isPending: Boolean
        get() = status.lowercase() in setOf("requested", "discussed")
}

@Serializable
data class AgentRegistration(
    val id: String,
    val name: String = "",
    val owner: String = "",
    val mode: String = "",
    val capabilities: List<String> = emptyList(),
)

/** Task (PROTOCOL.md §6) including the graph fields (`depends_on`, `team`, timestamps). */
@Serializable
data class Task(
    val id: String,
    val title: String = "",
    val description: String? = null,
    @SerialName("created_from") val createdFrom: String? = null,
    @SerialName("created_by") val createdBy: String = "",
    val status: String = TaskStatus.OPEN,
    val priority: String = "medium",
    val topics: List<String> = emptyList(),
    @SerialName("required_capabilities") val requiredCapabilities: List<String> = emptyList(),
    @SerialName("assigned_to") val assignedTo: String? = null,
    @SerialName("claimed_by") val claimedBy: String? = null,
    val deadline: String? = null,
    val artifacts: List<String> = emptyList(),
    val decisions: List<String> = emptyList(),
    @SerialName("parent_id") val parentId: String? = null,
    @SerialName("lease_seconds") val leaseSeconds: Long? = null,
    @SerialName("claim_expires_at") val claimExpiresAt: String? = null,
    @SerialName("depends_on") val dependsOn: List<String> = emptyList(),
    val team: String? = null,
    @SerialName("created_at") val createdAt: String? = null,
    @SerialName("claimed_at") val claimedAt: String? = null,
    @SerialName("started_at") val startedAt: String? = null,
    @SerialName("completed_at") val completedAt: String? = null,
)

object TaskStatus {
    const val OPEN = "open"
    const val CLAIMED = "claimed"
    const val IN_PROGRESS = "in_progress"
    const val BLOCKED = "blocked"
    const val COMPLETED = "completed"
    const val CANCELLED = "cancelled"

    val ALL = listOf(OPEN, CLAIMED, IN_PROGRESS, BLOCKED, COMPLETED, CANCELLED)

    fun isTerminal(status: String): Boolean =
        status.lowercase() in setOf("completed", "cancelled", "canceled", "failed", "done")
}

/** `PATCH /tasks/:id` body. Null fields are left out of the request. */
@Serializable
data class TaskPatch(
    val status: String? = null,
    @SerialName("assigned_to") val assignedTo: String? = null,
    val priority: String? = null,
    @SerialName("depends_on") val dependsOn: List<String>? = null,
    val team: String? = null,
    val title: String? = null,
    val description: String? = null,
)

// ---- network, spaces, teams (docs/network.md) ------------------------------

@Serializable
data class WhoAmI(
    val principal: String? = null,
    val kind: String? = null,
    @SerialName("is_owner") val isOwner: Boolean = false,
    val remote: Boolean = false,
)

object Access {
    const val PRIVATE = "private"
    const val CLOSED = "closed"
    const val OPEN = "open"
    val ALL = listOf(PRIVATE, CLOSED, OPEN)
}

@Serializable
data class Network(
    val name: String = "",
    val description: String = "",
    @SerialName("default_access") val defaultAccess: String = Access.CLOSED,
    @SerialName("visitor_write") val visitorWrite: Boolean = false,
)

/** Partial `PUT /network` body: only non-null fields are sent. */
@Serializable
data class NetworkPatch(
    val name: String? = null,
    val description: String? = null,
    @SerialName("default_access") val defaultAccess: String? = null,
    @SerialName("visitor_write") val visitorWrite: Boolean? = null,
)

@Serializable
data class Space(
    val id: String,
    val name: String = "",
    val description: String = "",
    val patterns: List<String> = emptyList(),
    val access: String = Access.CLOSED,
    @SerialName("visitor_write") val visitorWrite: Boolean = false,
    @SerialName("share_with_peers") val shareWithPeers: Boolean = false,
    val members: List<String> = emptyList(),
    val teams: List<String> = emptyList(),
    @SerialName("created_at") val createdAt: String? = null,
    @SerialName("updated_at") val updatedAt: String? = null,
)

object TeamRole {
    const val OWNER = "owner"
    const val ADMIN = "admin"
    const val MEMBER = "member"
    val ALL = listOf(OWNER, ADMIN, MEMBER)
}

@Serializable
data class TeamMember(
    val principal: String,
    val role: String = TeamRole.MEMBER,
)

@Serializable
data class Team(
    val id: String,
    val name: String = "",
    val description: String = "",
    val color: String? = null,
    val members: List<TeamMember> = emptyList(),
    @SerialName("created_at") val createdAt: String? = null,
) {
    /** The team room topic (`_team.<id>.chat`). */
    val roomTopic: String get() = "_team.$id.chat"
}

@Serializable
data class NetworkOverview(
    val network: Network = Network(),
    val spaces: List<Space> = emptyList(),
    val teams: List<Team> = emptyList(),
)

@Serializable
data class AccessCheck(
    val read: Boolean = false,
    val write: Boolean = false,
    val space: String? = null,
    val level: String = Access.CLOSED,
    val reason: String = "",
)

/** Slug rule for space and team ids: `[a-z0-9][a-z0-9-]{0,47}`. */
object Slug {
    private val RE = Regex("^[a-z0-9][a-z0-9-]{0,47}$")
    fun isValid(id: String): Boolean = RE.matches(id)

    /** Best-effort slug from a display name ("Family room" → "family-room"). */
    fun from(name: String): String {
        val s = name.lowercase()
            .map { if (it in 'a'..'z' || it in '0'..'9') it else '-' }
            .joinToString("")
            .replace(Regex("-+"), "-")
            .trim('-')
            .take(48)
            .trimEnd('-')
        return s.ifEmpty { "space" }
    }
}

// ---- task graph (GET /tasks/graph) -------------------------------------------

@Serializable
data class Lane(
    val id: String,
    /** `agent`, `human`, `team` or `unassigned`. */
    val kind: String = "unassigned",
    val label: String = "",
)

/** A task as returned by `/tasks/graph`: the task plus server-computed fields. */
@Serializable
data class GraphTask(
    val id: String,
    val title: String = "",
    val description: String? = null,
    @SerialName("created_by") val createdBy: String = "",
    val status: String = TaskStatus.OPEN,
    val priority: String = "medium",
    @SerialName("assigned_to") val assignedTo: String? = null,
    @SerialName("claimed_by") val claimedBy: String? = null,
    @SerialName("depends_on") val dependsOn: List<String> = emptyList(),
    val team: String? = null,
    @SerialName("created_at") val createdAt: String? = null,
    @SerialName("claimed_at") val claimedAt: String? = null,
    @SerialName("started_at") val startedAt: String? = null,
    @SerialName("completed_at") val completedAt: String? = null,
    val lane: String = "unassigned",
    @SerialName("blocked_by") val blockedBy: List<String> = emptyList(),
    @SerialName("waiting_seconds") val waitingSeconds: Long = 0,
    @SerialName("running_seconds") val runningSeconds: Long = 0,
    val critical: Boolean = false,
)

@Serializable
data class GraphEdge(val from: String, val to: String)

@Serializable
data class TaskGraph(
    val now: String? = null,
    val lanes: List<Lane> = emptyList(),
    val tasks: List<GraphTask> = emptyList(),
    val edges: List<GraphEdge> = emptyList(),
)

/** Scope of `GET /tasks/graph`. */
sealed class GraphScope(val query: String) {
    object All : GraphScope("all")
    object Mine : GraphScope("mine")
    class Agent(principal: String) : GraphScope("agent:$principal")
    class TeamScope(teamId: String) : GraphScope("team:$teamId")

    override fun toString(): String = query

    companion object {
        /** `all`, `mine`, `agent:<principal>`, `team:<id>`; anything else is `all`. */
        fun parse(s: String?): GraphScope = when {
            s == null -> All
            s == "mine" -> Mine
            s.startsWith("agent:") && s.length > 6 -> Agent(s.removePrefix("agent:"))
            s.startsWith("team:") && s.length > 5 -> TeamScope(s.removePrefix("team:"))
            else -> All
        }
    }
}

/** A `payload` helper: the JSON object, or null. */
internal fun JsonElement?.asObjectOrNull(): JsonObject? = this as? JsonObject
