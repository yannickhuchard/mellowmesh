package io.mellowmesh.app.data

import io.mellowmesh.core.AgentRegistration
import io.mellowmesh.core.Backoff
import io.mellowmesh.core.Chat
import io.mellowmesh.core.ChatMessage
import io.mellowmesh.core.Conversations
import io.mellowmesh.core.Decision
import io.mellowmesh.core.Directory
import io.mellowmesh.core.GraphScope
import io.mellowmesh.core.HomeInputs
import io.mellowmesh.core.HomeModel
import io.mellowmesh.core.HomeModelBuilder
import io.mellowmesh.core.LiveListener
import io.mellowmesh.core.MeshClient
import io.mellowmesh.core.MeshConfig
import io.mellowmesh.core.MeshException
import io.mellowmesh.core.Message
import io.mellowmesh.core.NetworkOverview
import io.mellowmesh.core.NetworkPatch
import io.mellowmesh.core.Space
import io.mellowmesh.core.Task
import io.mellowmesh.core.TaskGraph
import io.mellowmesh.core.TaskPatch
import io.mellowmesh.core.Team
import io.mellowmesh.core.TeamMember
import io.mellowmesh.core.WhoAmI
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.time.LocalTime

/** Turn any failure into one plain sentence for the screen. */
fun Throwable.friendly(): String = when (this) {
    is MeshException -> friendly
    is IllegalArgumentException -> message ?: "That did not look right."
    else -> "Something went wrong. Try again."
}

/**
 * The single source of truth of the app: wraps the core [MeshClient] (all
 * calls on Dispatchers.IO), keeps the home model, and runs the live
 * WebSocket while the app is in the foreground (with reconnect backoff).
 */
class MeshRepository(private val store: ConfigStore, private val scope: CoroutineScope) {
    @Volatile
    private var client: MeshClient? = null

    private val _config = MutableStateFlow<MeshConfig?>(null)
    val config: StateFlow<MeshConfig?> = _config.asStateFlow()

    private val _whoami = MutableStateFlow<WhoAmI?>(null)
    val whoami: StateFlow<WhoAmI?> = _whoami.asStateFlow()

    private val _home = MutableStateFlow<HomeModel?>(null)
    val home: StateFlow<HomeModel?> = _home.asStateFlow()

    private val _homeError = MutableStateFlow<String?>(null)
    val homeError: StateFlow<String?> = _homeError.asStateFlow()

    private val _network = MutableStateFlow<NetworkOverview?>(null)
    val network: StateFlow<NetworkOverview?> = _network.asStateFlow()

    private val _agents = MutableStateFlow<List<AgentRegistration>>(emptyList())
    val agents: StateFlow<List<AgentRegistration>> = _agents.asStateFlow()

    private val _connected = MutableStateFlow(false)
    val connected: StateFlow<Boolean> = _connected.asStateFlow()

    private val _live = MutableSharedFlow<Message>(extraBufferCapacity = 256)
    /** Every live message (pattern `**`, filtered by the hub to what this phone may read). */
    val live: SharedFlow<Message> = _live.asSharedFlow()

    private var liveJob: Job? = null
    private var foreground = false

    init {
        store.loadConfig()?.let { use(it) }
    }

    val isPaired: Boolean get() = client != null

    /** The principal used as `from` (paired identity). */
    val me: String get() = _config.value?.principal ?: _whoami.value?.principal ?: "human://me"

    val directory: Directory get() = Directory.of(_agents.value)

    private fun use(cfg: MeshConfig) {
        client = MeshClient(cfg)
        _config.value = cfg
    }

    private fun requireClient(): MeshClient =
        client ?: throw MeshException(MeshException.Kind.UNAUTHORIZED, 401, "not paired")

    private suspend fun <T> io(block: (MeshClient) -> T): T = withContext(Dispatchers.IO) { block(requireClient()) }

    // ---- pairing -----------------------------------------------------------------

    /**
     * Check the hub and the key, then remember them. Returns who we are.
     * Older hubs without `/whoami` are accepted when an authenticated read works.
     */
    suspend fun pair(baseUrl: String, token: String?, principal: String?, networkName: String?): WhoAmI =
        withContext(Dispatchers.IO) {
            val probe = MeshClient(MeshConfig(baseUrl, token, principal))
            val who = try {
                probe.whoami()
            } catch (e: MeshException) {
                if (e.kind != MeshException.Kind.NOT_FOUND) throw e
                probe.agents()
                WhoAmI(principal = principal, kind = "human", isOwner = false, remote = true)
            }
            if (who.principal == null && !token.isNullOrEmpty()) {
                throw MeshException(MeshException.Kind.UNAUTHORIZED, 401, "unknown key")
            }
            val effectivePrincipal = who.principal ?: principal
            val name = networkName ?: try {
                probe.network().network.name.takeIf { it.isNotBlank() }
            } catch (_: Exception) {
                null
            }
            store.saveConfig(baseUrl, token, effectivePrincipal, name)
            withContext(Dispatchers.Main) {
                use(MeshConfig(baseUrl, token, effectivePrincipal, store.loadConfig()?.deviceName ?: "Android"))
                _whoami.value = who
                if (foreground) startLive()
            }
            who
        }

    fun disconnect() {
        stopLive()
        client = null
        store.clearConfig()
        _config.value = null
        _whoami.value = null
        _home.value = null
        _network.value = null
        _agents.value = emptyList()
    }

    // ---- home --------------------------------------------------------------------

    /** Reload everything the Plaza needs in one round of parallel requests. */
    suspend fun refreshHome() {
        val c = client ?: return
        try {
            val model = withContext(Dispatchers.IO) {
                coroutineScope {
                    val who = async { runCatching { c.whoami() }.getOrNull() }
                    val agents = async { c.agents() }
                    val tasks = async { c.tasks() }
                    val decisions = async { c.decisions() }
                    val history = async { c.history(null, 500) }
                    val net = async { runCatching { c.network() }.getOrNull() }
                    val w = who.await()
                    val n = net.await()
                    val a = agents.await()
                    withContext(Dispatchers.Main) {
                        if (w != null) _whoami.value = w
                        if (n != null) _network.value = n
                        _agents.value = a
                    }
                    HomeModelBuilder.build(
                        HomeInputs(
                            me = w?.principal ?: me,
                            agents = a,
                            tasks = tasks.await(),
                            decisions = decisions.await(),
                            history = history.await(),
                            network = n,
                            readMarkers = store.readMarkers(),
                            nowMs = System.currentTimeMillis(),
                            hour = LocalTime.now().hour,
                        ),
                    )
                }
            }
            _home.value = model
            _homeError.value = null
        } catch (e: Exception) {
            _homeError.value = e.friendly()
        }
    }

    // ---- conversations -----------------------------------------------------------

    suspend fun conversation(id: String, limit: Int = 200): List<ChatMessage> {
        val conv = Conversations.parseId(id) ?: return emptyList()
        val dir = directory
        val meNow = me
        return io { c -> Chat.thread(id, c.history(Conversations.historyPattern(conv), limit), meNow, dir) }
    }

    suspend fun send(conversationId: String, text: String): ChatMessage {
        val dir = directory
        val meNow = me
        return io { c -> Chat.toChat(c.sendText(conversationId, text), meNow, dir) }
    }

    suspend fun sendUiAction(conversationId: String, cardMessageId: String, actionId: String, label: String): ChatMessage {
        val dir = directory
        val meNow = me
        return io { c -> Chat.toChat(c.sendUiAction(conversationId, cardMessageId, actionId, label), meNow, dir) }
    }

    fun markRead(conversationId: String, upToMs: Long) = store.markRead(conversationId, upToMs)

    // ---- decisions, agents, tasks ------------------------------------------------

    suspend fun decisions(): List<Decision> = io { it.decisions() }

    /** Only call after BiometricPrompt succeeded. Never queued: a live round trip or an error. */
    suspend fun respond(decisionId: String, optionId: String) = io { it.respondDecision(decisionId, optionId) }

    suspend fun loadAgents(): List<AgentRegistration> {
        val a = io { it.agents() }
        _agents.value = a
        return a
    }

    suspend fun taskGraph(scope: GraphScope): TaskGraph = io { it.taskGraph(scope) }

    suspend fun updateTask(id: String, patch: TaskPatch): Task? = io { it.updateTask(id, patch) }

    // ---- network -----------------------------------------------------------------

    suspend fun loadNetwork(): NetworkOverview {
        val n = io { it.network() }
        _network.value = n
        if (_whoami.value == null) _whoami.value = runCatching { io { it.whoami() } }.getOrNull()
        return n
    }

    suspend fun updateNetwork(patch: NetworkPatch) = io { it.updateNetwork(patch) }
    suspend fun createSpace(space: Space) = io { it.createSpace(space) }
    suspend fun updateSpace(space: Space) = io { it.updateSpace(space) }
    suspend fun deleteSpace(id: String) = io { it.deleteSpace(id) }
    suspend fun createTeam(team: Team) = io { it.createTeam(team) }
    suspend fun updateTeam(team: Team) = io { it.updateTeam(team) }
    suspend fun deleteTeam(id: String) = io { it.deleteTeam(id) }
    suspend fun addTeamMember(teamId: String, member: TeamMember) = io { it.addTeamMember(teamId, member) }
    suspend fun removeTeamMember(teamId: String, principal: String) = io { it.removeTeamMember(teamId, principal) }

    // ---- live --------------------------------------------------------------------

    /** Called by the process lifecycle: the WebSocket runs only in the foreground. */
    fun setForeground(value: Boolean) {
        foreground = value
        if (value) {
            startLive()
            scope.launch { refreshHome() }
        } else {
            stopLive()
        }
    }

    private fun startLive() {
        if (liveJob?.isActive == true || client == null) return
        liveJob = scope.launch {
            val backoff = Backoff()
            while (isActive) {
                val c = client ?: break
                val closed = CompletableDeferred<Throwable?>()
                val sub = withContext(Dispatchers.IO) {
                    c.subscribe("**", object : LiveListener {
                        override fun onOpen() {
                            backoff.reset()
                            _connected.value = true
                        }

                        override fun onMessage(message: Message) {
                            _live.tryEmit(message)
                        }

                        override fun onClosed(error: Throwable?) {
                            closed.complete(error)
                        }
                    })
                }
                try {
                    closed.await()
                } finally {
                    sub.close()
                    _connected.value = false
                }
                if (client !== c) break
                delay(backoff.nextDelayMs())
                // Catch up on what we missed while disconnected.
                refreshHome()
            }
        }
    }

    private fun stopLive() {
        liveJob?.cancel()
        liveJob = null
        _connected.value = false
    }
}
