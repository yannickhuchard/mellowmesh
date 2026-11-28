package io.mellowmesh.core

import kotlinx.serialization.KSerializer
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.add
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import java.io.IOException
import java.util.concurrent.TimeUnit

/** Hub connection settings. The token is a secret: never log it. */
data class MeshConfig(
    val baseUrl: String,
    val token: String?,
    /** The paired principal (e.g. `human://yannick`); used as `from`. */
    val principal: String? = null,
    val deviceName: String = "Android",
) {
    override fun toString(): String =
        "MeshConfig(baseUrl=$baseUrl, token=${if (token == null) "none" else "***"}, principal=$principal, deviceName=$deviceName)"
}

/** A failed hub call, with a plain-language message for the UI. */
class MeshException(
    val kind: Kind,
    val status: Int,
    detail: String,
    cause: Throwable? = null,
) : IOException(detail, cause) {
    enum class Kind { UNAUTHORIZED, FORBIDDEN, NOT_FOUND, CONFLICT, INVALID, SERVER, NETWORK, PROTOCOL }

    /** Short sentence for people who do not know HTTP. */
    val friendly: String
        get() = when (kind) {
            Kind.UNAUTHORIZED -> "Your hub did not accept this phone's key. Pair again."
            Kind.FORBIDDEN -> "You are not allowed to do that on this network."
            Kind.NOT_FOUND -> "That is not there any more."
            Kind.CONFLICT -> "Someone else changed this first. Try again."
            Kind.INVALID -> "The hub did not accept that: ${message ?: ""}".trim()
            Kind.SERVER -> "Your hub had a problem. Try again in a moment."
            Kind.NETWORK -> "Cannot reach your hub. Is your phone on the same Wi-Fi and the hub running?"
            Kind.PROTOCOL -> "Your hub answered something unexpected. Is it up to date?"
        }

    companion object {
        fun fromStatus(status: Int, body: String): MeshException {
            val kind = when (status) {
                401 -> Kind.UNAUTHORIZED
                403 -> Kind.FORBIDDEN
                404 -> Kind.NOT_FOUND
                409 -> Kind.CONFLICT
                in 400..499 -> Kind.INVALID
                else -> Kind.SERVER
            }
            return MeshException(kind, status, Sanitize.line(body).take(300))
        }
    }
}

/** Receives live messages from [MeshClient.subscribe]. Called on OkHttp's thread. */
interface LiveListener {
    fun onOpen() {}
    fun onMessage(message: Message)
    /** The connection ended. `error` is null for a normal close. */
    fun onClosed(error: Throwable?)
}

/** An open subscription. */
interface Subscription {
    fun close()
}

/**
 * Blocking client for the hub's REST API and WebSocket. Call from a
 * background thread (the app wraps it in `Dispatchers.IO`).
 */
class MeshClient(
    val config: MeshConfig,
    httpClient: OkHttpClient? = null,
) {
    private val base: HttpUrl = config.baseUrl.trimEnd('/').toHttpUrl()
    private val http: OkHttpClient = httpClient ?: OkHttpClient.Builder()
        .connectTimeout(10, TimeUnit.SECONDS)
        .readTimeout(30, TimeUnit.SECONDS)
        .writeTimeout(30, TimeUnit.SECONDS)
        .pingInterval(25, TimeUnit.SECONDS)
        .build()

    private val jsonType = "application/json".toMediaType()

    // ---- low level -----------------------------------------------------------

    internal fun url(path: String, query: Map<String, String?> = emptyMap()): HttpUrl {
        val b = base.newBuilder()
        path.trim('/').split('/').filter { it.isNotEmpty() }.forEach { b.addPathSegment(it) }
        query.forEach { (k, v) -> if (v != null) b.addQueryParameter(k, v) }
        return b.build()
    }

    private fun request(url: HttpUrl): Request.Builder {
        val b = Request.Builder().url(url).header("Accept", "application/json")
        config.token?.takeIf { it.isNotEmpty() }?.let { b.header("Authorization", "Bearer $it") }
        return b
    }

    private fun execute(req: Request): String {
        val response: Response = try {
            http.newCall(req).execute()
        } catch (e: IOException) {
            throw MeshException(MeshException.Kind.NETWORK, 0, e.javaClass.simpleName, e)
        }
        response.use { r ->
            val text = try {
                r.body?.string() ?: ""
            } catch (e: IOException) {
                throw MeshException(MeshException.Kind.NETWORK, r.code, "read failed", e)
            }
            if (!r.isSuccessful) throw MeshException.fromStatus(r.code, text)
            return text
        }
    }

    private fun <T> decode(serializer: KSerializer<T>, text: String): T = try {
        MeshJson.decodeFromString(serializer, text)
    } catch (e: Exception) {
        throw MeshException(MeshException.Kind.PROTOCOL, 200, "unexpected response", e)
    }

    private fun <T> get(path: String, serializer: KSerializer<T>, query: Map<String, String?> = emptyMap()): T =
        decode(serializer, execute(request(url(path, query)).get().build()))

    private fun send(method: String, path: String, body: String?): String {
        val rb = (body ?: "").toRequestBody(jsonType)
        val req = request(url(path)).method(method, if (method == "DELETE" && body == null) null else rb).build()
        return execute(req)
    }

    private fun <B> encode(serializer: KSerializer<B>, value: B): String = MeshJson.encodeToString(serializer, value)

    // ---- identity & health -------------------------------------------------

    fun health(): Boolean = try {
        execute(request(url("health")).get().build()); true
    } catch (e: MeshException) {
        if (e.kind == MeshException.Kind.NETWORK) throw e
        false
    }

    fun whoami(): WhoAmI = get("whoami", WhoAmI.serializer())

    // ---- network, spaces, teams ----------------------------------------------

    fun network(): NetworkOverview = get("network", NetworkOverview.serializer())

    fun updateNetwork(patch: NetworkPatch) {
        send("PUT", "network", encode(NetworkPatch.serializer(), patch))
    }

    fun spaces(): List<Space> = get("spaces", ListSerializer(Space.serializer()))

    fun createSpace(space: Space): Space {
        require(Slug.isValid(space.id)) { "invalid space id" }
        val text = send("POST", "spaces", encode(Space.serializer(), space))
        return if (text.isBlank()) space else decode(Space.serializer(), text)
    }

    fun updateSpace(space: Space) {
        require(Slug.isValid(space.id)) { "invalid space id" }
        send("PUT", "spaces/${space.id}", encode(Space.serializer(), space))
    }

    fun deleteSpace(id: String) {
        require(Slug.isValid(id)) { "invalid space id" }
        send("DELETE", "spaces/$id", null)
    }

    fun teams(): List<Team> = get("teams", ListSerializer(Team.serializer()))

    fun createTeam(team: Team): Team {
        require(Slug.isValid(team.id)) { "invalid team id" }
        val text = send("POST", "teams", encode(Team.serializer(), team))
        return if (text.isBlank()) team else decode(Team.serializer(), text)
    }

    fun updateTeam(team: Team) {
        require(Slug.isValid(team.id)) { "invalid team id" }
        send("PUT", "teams/${team.id}", encode(Team.serializer(), team))
    }

    fun deleteTeam(id: String) {
        require(Slug.isValid(id)) { "invalid team id" }
        send("DELETE", "teams/$id", null)
    }

    fun addTeamMember(teamId: String, member: TeamMember) {
        require(Slug.isValid(teamId)) { "invalid team id" }
        send("POST", "teams/$teamId/members", encode(TeamMember.serializer(), member))
    }

    fun removeTeamMember(teamId: String, principal: String) {
        require(Slug.isValid(teamId)) { "invalid team id" }
        // addPathSegment percent-encodes '/' and ':' safely.
        val u = base.newBuilder().addPathSegment("teams").addPathSegment(teamId)
            .addPathSegment("members").addPathSegment(principal).build()
        execute(request(u).delete().build())
    }

    fun accessCheck(topic: String, principal: String?): AccessCheck =
        get("access/check", AccessCheck.serializer(), mapOf("topic" to topic, "principal" to principal))

    // ---- messages ------------------------------------------------------------

    fun topics(): List<String> = get("topics", ListSerializer(String.serializer()))

    /** `GET /history?pattern=&limit=` (limit clamped to 1..1000). */
    fun history(pattern: String? = null, limit: Int = 50): List<Message> =
        get(
            "history",
            ListSerializer(Message.serializer()),
            mapOf("limit" to limit.coerceIn(1, 1000).toString(), "pattern" to pattern),
        )

    /** Publish; returns the message as stored by the hub (with its id and time). */
    fun publish(message: Message): Message {
        val text = send("POST", "publish", encode(Message.serializer(), message))
        return try {
            MeshJson.decodeFromString(Message.serializer(), text)
        } catch (_: Exception) {
            message
        }
    }

    /** A plain text message from the paired principal (device name in a header). */
    fun outgoing(topic: String, body: String): Message = Message(
        topic = topic,
        from = config.principal ?: "human://me",
        timestamp = Time.formatMs(System.currentTimeMillis()),
        contentType = "text/plain",
        body = body,
        headers = mapOf("x-mellowmesh-device" to config.deviceName.take(64)),
    )

    /**
     * Send a text into a conversation: an agent's inbox for Agent
     * conversations, the team room for Team, the root topic otherwise.
     */
    fun sendText(conversationId: String, text: String): Message {
        val conv = Conversations.parseId(conversationId) ?: throw MeshException(MeshException.Kind.INVALID, 0, "not a conversation")
        require(text.isNotBlank()) { "message is empty" }
        require(text.toByteArray(Charsets.UTF_8).size <= MAX_SEND_BYTES) { "message too long" }
        return publish(outgoing(Conversations.sendTopic(conv), text))
    }

    /** [sendText] with a payload (a line said in a group of Mellows). */
    fun sendWithPayload(conversationId: String, text: String, payload: JsonObject): Message {
        val conv = Conversations.parseId(conversationId) ?: throw MeshException(MeshException.Kind.INVALID, 0, "not a conversation")
        require(text.isNotBlank()) { "message is empty" }
        require(text.toByteArray(Charsets.UTF_8).size <= MAX_SEND_BYTES) { "message too long" }
        return publish(outgoing(Conversations.sendTopic(conv), text).copy(payload = payload))
    }

    /**
     * Reply to a MeshUI action tap (same shape as app-core `send_ui_action`):
     * `payload.ui_action = {card_message_id, action_id}`, `parent_id` = card
     * message. Never answers a decision.
     */
    fun sendUiAction(conversationId: String, cardMessageId: String, actionId: String, label: String): Message {
        val conv = Conversations.parseId(conversationId) ?: throw MeshException(MeshException.Kind.INVALID, 0, "not a conversation")
        require(MeshUi.isValidActionId(actionId)) { "invalid action id" }
        require(cardMessageId.isNotEmpty() && cardMessageId.length <= 128) { "invalid card message id" }
        val clean = Sanitize.line(label).trim().take(40)
        val payload: JsonObject = buildJsonObject {
            put(MeshUi.ACTION_KEY, buildJsonObject {
                put("card_message_id", cardMessageId)
                put("action_id", actionId)
            })
        }
        val msg = outgoing(Conversations.sendTopic(conv), clean.ifEmpty { actionId }).copy(
            contentType = MeshUi.ACTION_CONTENT_TYPE,
            parentId = cardMessageId,
            payload = payload,
        )
        return publish(msg)
    }

    // ---- agents, decisions, tasks --------------------------------------------

    fun agents(): List<AgentRegistration> = get("agents", ListSerializer(AgentRegistration.serializer()))

    // ---- groups kept by this hub -------------------------------------------------

    /** The groups of Mellows this hub keeps (owner only). */
    fun groups(): List<HubGroup> = get("groups", ListSerializer(HubGroup.serializer()))

    /** Keep a group here (new, or the same id again). */
    fun keepGroup(body: JsonObject): HubGroup =
        decode(HubGroup.serializer(), send("POST", "groups", body.toString()))

    fun groupThread(id: String, limit: Int = 200): HubThread =
        get("groups/$id/thread", HubThread.serializer(), mapOf("limit" to limit.toString()))

    /** Say a line in the group (to member ids; empty = `@Name`s, else everyone). */
    fun groupSay(id: String, text: String, to: List<String>) {
        require(text.isNotBlank()) { "message is empty" }
        send(
            "POST",
            "groups/$id/say",
            buildJsonObject {
                put("text", text)
                putJsonArray("to") { to.forEach { add(it) } }
            }.toString(),
        )
    }

    fun forgetGroup(id: String) {
        send("DELETE", "groups/$id", null)
    }

    /**
     * A pass for the hub that keeps a group: a token of this machine, as
     * [principal], that may write only [conversationId]'s inbox and read only
     * that conversation (owner only). Returns the token.
     */
    fun createPass(principal: String, conversationId: String, label: String): String {
        val text = send(
            "POST",
            "auth/tokens",
            buildJsonObject {
                put("principal", principal)
                putJsonArray("read_scopes") { add("$conversationId.*") }
                putJsonArray("write_scopes") { add("$conversationId.inbox") }
                put("label", label.take(100))
            }.toString(),
        )
        val obj = MeshJson.parseToJsonElement(text) as? JsonObject
        return (obj?.get("token") as? JsonPrimitive)?.content
            ?: throw MeshException(MeshException.Kind.PROTOCOL, 200, "no token")
    }

    /** Who invited AIs may write to (owner: the whole policy). */
    fun agentMail(): MailPolicyView = get("agent-mail", MailPolicyView.serializer())

    /** Change the network rule or one AI's rule (owner only); returns the new policy. */
    fun setAgentMail(change: MailChange): MailPolicyView =
        decode(MailPolicyView.serializer(), send("PUT", "agent-mail", encode(MailChange.serializer(), change)))

    /** A one-click invitation for an AI: the link it opens to join (owner only). */
    fun inviteAi(request: InviteRequest): InviteLink =
        decode(InviteLink.serializer(), send("POST", "invite", encode(InviteRequest.serializer(), request)))

    /** The hub's own mailbox (owner: settings, people, status). */
    fun email(): EmailView = get("email", EmailView.serializer())

    /** Change the hub's email (owner only); returns the new view. */
    fun setEmail(patch: EmailPatch): EmailView =
        decode(EmailView.serializer(), send("PUT", "email", encode(EmailPatch.serializer(), patch)))

    /** Open the hub's inbox and send the owner a test email (owner only). */
    fun checkEmail(): EmailCheck = decode(EmailCheck.serializer(), send("POST", "email/check", "{}"))

    /** Allow or remove an address that may give commands by email (owner only). */
    fun setEmailPerson(change: EmailPersonChange): EmailView =
        decode(EmailView.serializer(), send("PUT", "email/people", encode(EmailPersonChange.serializer(), change)))

    /** AI platforms the hub spotted on its own machine (owner only). */
    fun platforms(): PlatformsSnapshot = get("platforms", PlatformsSnapshot.serializer())

    /**
     * Connect an AI app on the hub's computer to the network in one tap:
     * the hub writes that app's own tool-server setting (owner only).
     */
    fun connectPlatform(platformId: String): ConnectResult {
        require(platformId.isNotBlank() && platformId.all { it.isLetterOrDigit() || it == '-' || it == '_' }) {
            "invalid platform id"
        }
        return decode(ConnectResult.serializer(), send("POST", "platforms/$platformId/connect", "{}"))
    }

    /**
     * Make a Mellow from an AI app installed on the hub's computer: it is
     * registered on the network with this name and purpose, and answers in
     * its conversation from then on (owner only).
     */
    fun createMellow(platformId: String, name: String, purpose: String): MellowCreated {
        require(platformId.isNotBlank() && platformId.all { it.isLetterOrDigit() || it == '-' || it == '_' }) {
            "invalid platform id"
        }
        val body = buildJsonObject {
            put("name", JsonPrimitive(name.trim()))
            put("purpose", JsonPrimitive(purpose.trim()))
        }
        return decode(MellowCreated.serializer(), send("POST", "platforms/$platformId/mellows", body.toString()))
    }

    fun decisions(): List<Decision> = get("decisions", ListSerializer(Decision.serializer()))

    /** Answer a decision. The app gates this behind BiometricPrompt. Never queued offline. */
    fun respondDecision(decisionId: String, optionId: String) {
        require(optionId.isNotBlank() && optionId.length <= 128) { "invalid option id" }
        require(decisionId.isNotBlank() && decisionId.length <= 128 && decisionId.all { it.isLetterOrDigit() || it == '_' || it == '-' }) {
            "invalid decision id"
        }
        val body = buildJsonObject { put("option_id", JsonPrimitive(optionId)) }
        send("POST", "decisions/$decisionId/respond", body.toString())
    }

    fun tasks(): List<Task> = get("tasks", ListSerializer(Task.serializer()))

    /** `POST /tasks`: the hub stamps the creator and the lifecycle; returns the task as stored. */
    fun createTask(task: Task): Task {
        require(task.title.isNotBlank() && task.title.length <= 200) { "invalid task title" }
        require(task.id.isEmpty() || (task.id.length <= 128 && task.id.all { it.isLetterOrDigit() || it == '_' || it == '-' })) {
            "invalid task id"
        }
        return decode(Task.serializer(), send("POST", "tasks", encode(Task.serializer(), task)))
    }

    /** `PATCH /tasks/:id`; returns the updated task when the hub sends it back. */
    fun updateTask(taskId: String, patch: TaskPatch): Task? {
        val u = base.newBuilder().addPathSegment("tasks").addPathSegment(taskId).build()
        val req = request(u).patch(encode(TaskPatch.serializer(), patch).toRequestBody(jsonType)).build()
        val text = execute(req)
        return if (text.isBlank()) null else try {
            MeshJson.decodeFromString(Task.serializer(), text)
        } catch (_: Exception) {
            null
        }
    }

    fun taskGraph(scope: GraphScope = GraphScope.All): TaskGraph =
        get("tasks/graph", TaskGraph.serializer(), mapOf("scope" to scope.query))

    // ---- live ----------------------------------------------------------------

    /** The WebSocket URL: `/ws?pattern=<p>`. The token travels only in the
     * Authorization header (OkHttp sends it on the upgrade request), never
     * in the URL, where proxies and logs would see it. */
    internal fun wsUrl(pattern: String): HttpUrl = url("ws", mapOf("pattern" to pattern))

    /** Subscribe to `pattern`; frames are JSON messages. Non-message frames are ignored. */
    fun subscribe(pattern: String, listener: LiveListener): Subscription {
        val req = request(wsUrl(pattern)).build()
        val ws = http.newWebSocket(req, object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) = listener.onOpen()

            override fun onMessage(webSocket: WebSocket, text: String) {
                val msg = try {
                    MeshJson.decodeFromString(Message.serializer(), text)
                } catch (_: Exception) {
                    null
                }
                if (msg != null) listener.onMessage(msg)
            }

            override fun onClosing(webSocket: WebSocket, code: Int, reason: String) {
                webSocket.close(1000, null)
            }

            override fun onClosed(webSocket: WebSocket, code: Int, reason: String) = listener.onClosed(null)

            override fun onFailure(webSocket: WebSocket, t: Throwable, response: Response?) {
                val err = if (response != null) MeshException.fromStatus(response.code, "") else
                    MeshException(MeshException.Kind.NETWORK, 0, t.javaClass.simpleName, t)
                listener.onClosed(err)
            }
        })
        return object : Subscription {
            override fun close() {
                ws.close(1000, "bye")
            }
        }
    }

    companion object {
        const val MAX_SEND_BYTES = 16 * 1024
        const val DEFAULT_PORT = 40000
    }
}

/** Exponential reconnect backoff with a cap (1 s, 2 s, 4 s … 60 s). */
class Backoff(private val baseMs: Long = 1_000, private val maxMs: Long = 60_000) {
    private var attempt = 0

    fun nextDelayMs(): Long {
        val shift = attempt.coerceAtMost(16)
        attempt++
        return (baseMs shl shift).coerceAtMost(maxMs)
    }

    fun reset() {
        attempt = 0
    }

    val attempts: Int get() = attempt
}
