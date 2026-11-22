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

/** One AI platform the hub spotted on its own machine (`GET /platforms`). */
@Serializable
data class PlatformInfo(
    val id: String,
    val name: String = "",
    val sessions: Int = 0,
    val running: Boolean = false,
    /** Present on the computer (a running platform counts). */
    val installed: Boolean = false,
    /** A Mellow can be made from it (an open session of it can host). */
    @SerialName("can_chat") val canChat: Boolean = false,
    /** A session of this platform is hosting Mellows right now. */
    val hosted: Boolean = false,
    /** The way in: `one_click`, `snippet`, `message` or `none`. */
    val connect: String = "none",
    /** MellowMesh is already among its tool servers. */
    val connected: Boolean = false,
    /** A web assistant: nothing to install, invited by a pasted message. */
    val web: Boolean = false,
    /** `command`, `paste` or `none` — how it joins the network. */
    val invite: String = "none",
    val command: String = "",
)

/**
 * A message written while the network was out of reach. It waits on the
 * phone and leaves by itself, in order, as soon as the hub answers again.
 */
@Serializable
data class PendingMessage(
    val id: String,
    val conversationId: String,
    val text: String,
    val createdAtMs: Long,
    /** The machine it leaves through (null: the one in use when it was written). */
    val machineId: String? = null,
)

/**
 * One computer of the owner's network this phone was paired with. A
 * network can be spread over several machines (peered hubs); the phone
 * keeps a key for each, and pairing another one adds it.
 */
@Serializable
data class SavedMachine(
    val id: String,
    val baseUrl: String,
    val token: String? = null,
    val principal: String? = null,
    val networkName: String? = null,
    /** What people call it: the hub's machine name, else the address's host. */
    val label: String = "",
    val addedAtMs: Long = 0,
)

object Machines {
    /** "https://a.b:40000/" and "https://a.b:40000" are the same machine. */
    fun sameAddress(a: String, b: String): Boolean =
        a.trim().trimEnd('/').equals(b.trim().trimEnd('/'), ignoreCase = true)

    /** A short name from an address: its host, without a long tunnel suffix. */
    fun labelFromUrl(url: String): String {
        val host = url.substringAfter("://").substringBefore('/').substringBefore(':')
        return when {
            host.endsWith(".trycloudflare.com") -> "Tunnel " + host.substringBefore('.').take(14)
            host.isBlank() -> "Machine"
            else -> host
        }
    }

    /**
     * Add [incoming] or replace the machine at the same address (a fresh key
     * for it); every other machine is kept.
     */
    fun upsert(list: List<SavedMachine>, incoming: SavedMachine): List<SavedMachine> {
        val i = list.indexOfFirst { sameAddress(it.baseUrl, incoming.baseUrl) }
        return if (i < 0) list + incoming
        else list.toMutableList().also { it[i] = incoming.copy(id = list[i].id, addedAtMs = list[i].addedAtMs) }
    }
}

/** `POST /platforms/:id/connect` response: one sentence on what changed. */
@Serializable
data class ConnectResult(
    val connected: Boolean = false,
    val message: String = "",
)

/** `POST /platforms/:id/mellows` response: the Mellow that was just made. */
@Serializable
data class MellowCreated(
    @SerialName("agent_id") val agentId: String = "",
    val name: String = "",
    val platform: String = "",
    val purpose: String = "",
    /** The conversation to open (`_agent.<owner>.<name>`). */
    @SerialName("conversation_id") val conversationId: String = "",
)

/** `GET /platforms` response: what runs on the hub's computer right now. */
@Serializable
data class PlatformsSnapshot(
    val machine: String = "",
    @SerialName("looked_at") val lookedAt: String? = null,
    val platforms: List<PlatformInfo> = emptyList(),
)

/** A `payload` helper: the JSON object, or null. */
internal fun JsonElement?.asObjectOrNull(): JsonObject? = this as? JsonObject

// ---- notes between AIs (who invited AIs may write to) ------------------------

/** Which helpers' mailboxes a rule opens. */
enum class MailMode(val wire: String) {
    ANY("any"), ONLY("only"), NONE("none");

    companion object {
        fun of(wire: String): MailMode = entries.firstOrNull { it.wire == wire } ?: ANY
    }
}

/** One rule: `any`, `only` (the helpers in [boxes]) or `none`. */
@Serializable
data class MailRule(
    val mode: String = MailMode.ANY.wire,
    val boxes: List<String> = emptyList(),
) {
    val kind: MailMode get() = MailMode.of(mode)

    companion object {
        fun of(kind: MailMode, boxes: Collection<String> = emptyList()): MailRule =
            MailRule(kind.wire, if (kind == MailMode.ONLY) boxes.distinct().sorted() else emptyList())
    }
}

/** An invited AI or a helper's mailbox: its id and name. */
@Serializable
data class MailParty(val id: String, val name: String = "")

/** `GET /agent-mail` as the owner sees it. */
@Serializable
data class MailPolicyView(
    val network: MailRule = MailRule(),
    @SerialName("per_ai") val perAi: Map<String, MailRule> = emptyMap(),
    val invited: List<MailParty> = emptyList(),
    val mailboxes: List<MailParty> = emptyList(),
) {
    /** The rule [ai] follows, and whether it is its own. */
    fun ruleFor(ai: String): Pair<MailRule, Boolean> =
        perAi[ai]?.let { it to true } ?: (network to false)

    fun nameOf(id: String): String =
        (mailboxes + invited).firstOrNull { it.id == id }?.name?.takeIf { it.isNotBlank() }
            ?: id.substringAfterLast('/')
}

/** `PUT /agent-mail`: the network rule (no [forAi]) or one AI's ([rule] null = follow the network). */
@Serializable
data class MailChange(
    @SerialName("for") val forAi: String? = null,
    val rule: MailRule? = null,
)


// ---- the hub's own mailbox (email) ------------------------------------------

/** A mail server: host, port, and `tls`, `starttls` or `none`. */
@Serializable
data class EmailServer(val host: String = "", val port: Int = 0, val security: String = "tls")

/** The hub's mailbox and what it does with it (never the password). */
@Serializable
data class EmailSettings(
    val enabled: Boolean = false,
    val address: String = "",
    val username: String = "",
    val imap: EmailServer = EmailServer(),
    val smtp: EmailServer = EmailServer(),
    val folder: String = "INBOX",
    @SerialName("poll_seconds") val pollSeconds: Int = 30,
    @SerialName("notify_decisions") val notifyDecisions: Boolean = true,
    @SerialName("notify_mentions") val notifyMentions: Boolean = false,
    @SerialName("trust_provider") val trustProvider: Boolean = true,
)

/** An address allowed to give commands by email, and whose it is. */
@Serializable
data class EmailPerson(val address: String, val person: String = "")

@Serializable
data class EmailStatus(
    @SerialName("last_check") val lastCheck: String? = null,
    @SerialName("last_error") val lastError: String? = null,
    val received: Long = 0,
    val ignored: Long = 0,
    val sent: Long = 0,
)

/** `GET /email` as the owner sees it. */
@Serializable
data class EmailView(
    val settings: EmailSettings = EmailSettings(),
    @SerialName("password_set") val passwordSet: Boolean = false,
    @SerialName("password_from_env") val passwordFromEnv: Boolean = false,
    val people: List<EmailPerson> = emptyList(),
    val status: EmailStatus = EmailStatus(),
) {
    /** The hub has an address and an app password. */
    val configured: Boolean get() = settings.address.isNotBlank() && passwordSet
}

/** `PUT /email`: only the fields to change (nulls are left out). */
@Serializable
data class EmailPatch(
    val enabled: Boolean? = null,
    val address: String? = null,
    val username: String? = null,
    val imap: EmailServer? = null,
    val smtp: EmailServer? = null,
    @SerialName("poll_seconds") val pollSeconds: Int? = null,
    @SerialName("notify_decisions") val notifyDecisions: Boolean? = null,
    @SerialName("notify_mentions") val notifyMentions: Boolean? = null,
    @SerialName("trust_provider") val trustProvider: Boolean? = null,
    val password: String? = null,
    @SerialName("forget_password") val forgetPassword: Boolean? = null,
    @SerialName("owner_address") val ownerAddress: String? = null,
)

/** `PUT /email/people`: allow an address ([person] null = the owner), or remove it. */
@Serializable
data class EmailPersonChange(val address: String, val person: String? = null, val remove: Boolean = false)

@Serializable
data class EmailStep(val ok: Boolean = false, val detail: String = "")

/** `POST /email/check`: the inbox opens; a test email was sent. */
@Serializable
data class EmailCheck(val inbox: EmailStep = EmailStep(), val send: EmailStep = EmailStep())

/** A plausible single email address (the hub checks it again). */
fun looksLikeEmail(address: String): Boolean {
    val a = address.trim()
    val at = a.indexOf('@')
    if (at <= 0 || at != a.lastIndexOf('@') || a.any { it.isWhitespace() || it in "<>,;\"()[]\\" }) return false
    val domain = a.substring(at + 1)
    return domain.contains('.') && !domain.startsWith('.') && !domain.endsWith('.')
}


// ---- inviting an AI in one link ----------------------------------------------

/** `POST /invite`: a known AI app ([platform]) or another AI by [name]. */
@Serializable
data class InviteRequest(val platform: String? = null, val name: String? = null)

/** A one-click invitation: the link an AI opens to join, and a message around it. */
@Serializable
data class InviteLink(
    @SerialName("join_url") val joinUrl: String = "",
    val message: String = "",
    /** The short code, for an AI that can only POST `/pair/claim`. */
    val code: String = "",
    val principal: String = "",
    val name: String = "",
    val network: String = "",
    @SerialName("expires_at") val expiresAt: String = "",
    /** How many times the link opens before it stops. */
    val opens: Int = 0,
    /** False when the hub has no public address: web assistants cannot open it. */
    @SerialName("reachable_from_web") val reachableFromWeb: Boolean = true,
)

/** The AIs offered as one-tap invitations (catalog ids on the hub). */
object InviteCatalog {
    data class Entry(val platform: String, val name: String)

    val web: List<Entry> = listOf(
        Entry("grok", "Grok"),
        Entry("chatgpt", "ChatGPT"),
        Entry("claude-web", "Claude"),
        Entry("gemini-web", "Gemini"),
        Entry("mistral", "Mistral"),
    )

    /** A name the hub can turn into an identity: letters or digits, 1–40 characters. */
    fun validName(name: String): Boolean {
        val n = name.trim()
        return n.length in 1..40 && n.any { it.isLetterOrDigit() } && n.none { it.isISOControl() }
    }
}
