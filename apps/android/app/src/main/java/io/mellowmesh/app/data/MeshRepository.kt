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
import io.mellowmesh.core.ChatSpace
import io.mellowmesh.core.Everywhere
import io.mellowmesh.core.MellowGroup
import io.mellowmesh.core.PlacedConversation
import io.mellowmesh.core.EverywhereModel
import io.mellowmesh.core.LiveListener
import io.mellowmesh.core.MachineHealth
import io.mellowmesh.core.MachineHome
import io.mellowmesh.core.Machines
import io.mellowmesh.core.MeshClient
import io.mellowmesh.core.MeshConfig
import io.mellowmesh.core.MeshException
import io.mellowmesh.core.Message
import io.mellowmesh.core.NetworkOverview
import io.mellowmesh.core.NetworkPatch
import io.mellowmesh.core.PendingMessage
import io.mellowmesh.core.SavedMachine
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
import kotlinx.coroutines.awaitAll
import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.callbackFlow
import kotlinx.coroutines.flow.flowOn
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import java.time.LocalTime
import java.util.concurrent.ConcurrentHashMap

/** Turn any failure into one plain sentence for the screen. */
fun Throwable.friendly(): String = when (this) {
    is MeshException -> friendly
    is IllegalArgumentException -> message ?: "That did not look right."
    else -> "Something went wrong. Try again."
}

/** One paired machine as the app sees it right now. */
data class MachineStatus(
    val machine: SavedMachine,
    /** The machine the Plaza, boards and to-dos come from. */
    val active: Boolean,
    /** Did it answer the last time we asked? null = not asked yet. */
    val reachable: Boolean?,
    val agents: List<AgentRegistration>,
    val lastAnswerMs: Long?,
    /** Something answers at the address but it is not usable: a lost address or a refused key. */
    val trouble: MachineHealth? = null,
) {
    /** The name to show (never the hub's `node://…` id). */
    val name: String get() = Machines.displayLabel(machine.label, machine.baseUrl)

    val health: MachineHealth
        get() = trouble ?: when (reachable) {
            true -> MachineHealth.ANSWERS
            false -> MachineHealth.RESTING
            null -> MachineHealth.UNKNOWN
        }
}

/** A Mellow and the machine it lives on (for calls across machines). */
data class MachineAgent(
    val machineId: String,
    val machineLabel: String,
    val agent: AgentRegistration,
    val machineReachable: Boolean?,
)

/**
 * The single source of truth of the app. A network can be spread over
 * several machines: the phone keeps a client per paired machine. One is
 * "in use" (home model, boards, the live WebSocket while in the
 * foreground); the others are checked regularly so their Mellows stay
 * callable and messages written for them leave when they answer.
 */
class MeshRepository(private val store: ConfigStore, private val scope: CoroutineScope) {
    @Volatile
    private var client: MeshClient? = null

    @Volatile
    private var activeId: String? = null

    /** machine id → its client (rebuilt when its address or key changes). */
    private val clients = ConcurrentHashMap<String, MeshClient>()

    private val _config = MutableStateFlow<MeshConfig?>(null)
    val config: StateFlow<MeshConfig?> = _config.asStateFlow()

    private val _machines = MutableStateFlow<List<MachineStatus>>(emptyList())
    val machines: StateFlow<List<MachineStatus>> = _machines.asStateFlow()

    private val _allMellows = MutableStateFlow<List<MachineAgent>>(emptyList())
    /** Every Mellow of every paired machine (each once), free machines first. */
    val allMellows: StateFlow<List<MachineAgent>> = _allMellows.asStateFlow()

    private val _everywhere = MutableStateFlow<EverywhereModel?>(null)
    /** Every Mellow of every machine with what needs the person (the Everywhere Plaza). */
    val everywhere: StateFlow<EverywhereModel?> = _everywhere.asStateFlow()

    private val _allConversations = MutableStateFlow<List<PlacedConversation>>(emptyList())
    /** The conversations of every machine (Chats shows them together). */
    val allConversations: StateFlow<List<PlacedConversation>> = _allConversations.asStateFlow()

    private val _groups = MutableStateFlow(store.loadGroups())
    /** The person's own groups of Mellows, from any network. */
    val groups: StateFlow<List<MellowGroup>> = _groups.asStateFlow()

    /** Make a group (returns it), or null when the name is not usable or there are too many. */
    fun createGroup(name: String, members: List<String> = emptyList()): MellowGroup? {
        val clean = ChatSpace.cleanName(name) ?: return null
        if (_groups.value.size >= ChatSpace.MAX_GROUPS) return null
        val g = MellowGroup("g_" + java.util.UUID.randomUUID().toString().take(8), clean, members.distinctBy { it.lowercase() })
        saveGroups(_groups.value + g)
        return g
    }

    fun addToGroup(groupId: String, agentId: String) =
        saveGroups(_groups.value.map { if (it.id == groupId) ChatSpace.add(it, agentId) else it })

    fun removeFromGroup(groupId: String, agentId: String) =
        saveGroups(_groups.value.map { if (it.id == groupId) ChatSpace.remove(it, agentId) else it })

    fun deleteGroup(groupId: String) = saveGroups(_groups.value.filterNot { it.id == groupId })

    private fun saveGroups(list: List<MellowGroup>) {
        _groups.value = list
        store.saveGroups(list)
    }

    private val _everywhereOn = MutableStateFlow(false)
    /** The Plaza shows every machine at once (only offered with two machines or more). */
    val everywhereOn: StateFlow<Boolean> = _everywhereOn.asStateFlow()

    private val _whoami = MutableStateFlow<WhoAmI?>(null)
    val whoami: StateFlow<WhoAmI?> = _whoami.asStateFlow()

    private val _home = MutableStateFlow<HomeModel?>(null)
    val home: StateFlow<HomeModel?> = _home.asStateFlow()

    private val _homeError = MutableStateFlow<String?>(null)
    val homeError: StateFlow<String?> = _homeError.asStateFlow()

    private val _network = MutableStateFlow<NetworkOverview?>(null)
    val network: StateFlow<NetworkOverview?> = _network.asStateFlow()

    private val _agents = MutableStateFlow<List<AgentRegistration>>(emptyList())
    /** The Mellows of the machine in use. */
    val agents: StateFlow<List<AgentRegistration>> = _agents.asStateFlow()

    private val _connected = MutableStateFlow(false)
    val connected: StateFlow<Boolean> = _connected.asStateFlow()

    /** Whether the machine in use answered the last time we asked: null = not asked yet. */
    private val _reachable = MutableStateFlow<Boolean?>(null)
    val reachable: StateFlow<Boolean?> = _reachable.asStateFlow()

    /** Messages written while their machine was out of reach, oldest first. */
    private val _outbox = MutableStateFlow<List<PendingMessage>>(emptyList())
    val outbox: StateFlow<List<PendingMessage>> = _outbox.asStateFlow()
    private val flushing = kotlinx.coroutines.sync.Mutex()

    private val _live = MutableSharedFlow<Message>(extraBufferCapacity = 256)
    /** Every live message of the machine in use (pattern `**`, filtered by the hub). */
    val live: SharedFlow<Message> = _live.asSharedFlow()

    private var liveJob: Job? = null
    private var watchJob: Job? = null
    private var foreground = false

    init {
        syncMachines()
        activate(store.activeMachineId)
        // The Mellows met before stay on screen (and callable) offline.
        _agents.value = activeId?.let { store.loadAgentsCache(it) }.orEmpty()
        // Messages saved before machines existed belong to the machine that
        // was in use then: bind them now, so adding a machine never
        // redirects them.
        val loaded = store.loadOutbox()
        val bound = loaded.map { if (it.machineId == null && activeId != null) it.copy(machineId = activeId) else it }
        if (bound != loaded) store.saveOutbox(bound)
        _outbox.value = bound
    }

    val isPaired: Boolean get() = client != null

    /** The principal used as `from` on the machine in use. */
    val me: String get() = _config.value?.principal ?: _whoami.value?.principal ?: "human://me"

    /** Names of every Mellow this phone knows, on any machine. */
    val directory: Directory
        get() = Directory.of((_agents.value + _allMellows.value.map { it.agent }).distinctBy { it.id.lowercase() })

    // ---- machines ------------------------------------------------------------------

    /** Align the clients and statuses with the stored machines. */
    private fun syncMachines() {
        val saved = store.loadMachines()
        val ids = saved.map { it.id }.toSet()
        clients.keys.retainAll(ids)
        for (m in saved) {
            val c = clients[m.id]
            if (c == null || c.config.baseUrl != m.baseUrl || c.config.token != m.token) {
                clients[m.id] = MeshClient(store.configOf(m))
            }
        }
        val active = store.activeMachineId
        val before = _machines.value.associateBy { it.machine.id }
        _machines.value = saved.map { m ->
            val old = before[m.id]
            MachineStatus(
                machine = m,
                active = m.id == active,
                reachable = old?.reachable,
                agents = old?.agents ?: store.loadAgentsCache(m.id),
                lastAnswerMs = old?.lastAnswerMs,
                trouble = old?.trouble,
            )
        }
        rebuildMellows()
        updateEverywhereOn()
    }

    /** Everywhere is offered from two machines on, and is the default there unless the person chose one machine. */
    private fun updateEverywhereOn() {
        _everywhereOn.value = _machines.value.size >= 2 && store.everywhere != false
    }

    /** Show every machine on the Plaza (true) or the machine in use only. */
    fun setEverywhere(on: Boolean) {
        store.everywhere = on
        updateEverywhereOn()
        scope.launch { if (_everywhereOn.value) refreshEverywhere() else refreshHome() }
    }

    private fun activate(id: String?) {
        val m = _machines.value.firstOrNull { it.machine.id == id }?.machine
        activeId = m?.id
        client = m?.let { clients[it.id] }
        _config.value = m?.let { store.configOf(it) }
        _machines.update { list -> list.map { it.copy(active = it.machine.id == activeId) } }
        rebuildMellows()
    }

    private fun updateStatus(id: String, change: (MachineStatus) -> MachineStatus) {
        _machines.update { list -> list.map { if (it.machine.id == id) change(it) else it } }
        rebuildMellows()
    }

    private fun markReachable(id: String, value: Boolean) {
        updateStatus(id) {
            it.copy(
                reachable = value,
                lastAnswerMs = if (value) System.currentTimeMillis() else it.lastAnswerMs,
                trouble = if (value) null else it.trouble,
            )
        }
        if (id == activeId) _reachable.value = value
    }

    /**
     * Record what a failed read of a machine's Mellows says about it: out
     * of reach, an address that no longer leads to the hub, or a refused
     * key. Returns that health (null: the failure says nothing about the machine).
     */
    private fun markFailure(id: String, e: Throwable): MachineHealth? {
        val h = MachineHealth.ofFailure(e) ?: return null
        markReachable(id, false)
        if (h.needsFix) updateStatus(id) { it.copy(trouble = h) }
        return h
    }

    private fun rebuildMellows() {
        val seen = HashSet<String>()
        val out = mutableListOf<MachineAgent>()
        val ordered = _machines.value.sortedWith(
            compareByDescending<MachineStatus> { it.reachable == true }.thenByDescending { it.active },
        )
        for (m in ordered) {
            for (a in m.agents) {
                if (seen.add(a.id.lowercase())) out += MachineAgent(m.machine.id, m.name, a, m.reachable)
            }
        }
        _allMellows.value = out
    }

    /** The views that belong to one machine start again from its cache. */
    private fun resetActiveViews() {
        val id = activeId
        _home.value = null
        _homeError.value = null
        _network.value = null
        _whoami.value = null
        _agents.value = id?.let { store.loadAgentsCache(it) }.orEmpty()
        _reachable.value = _machines.value.firstOrNull { it.machine.id == id }?.reachable
    }

    /** Use another paired machine for the Plaza, boards and to-dos. */
    fun switchMachine(id: String) {
        if (id == activeId || _machines.value.none { it.machine.id == id }) return
        store.activeMachineId = id
        stopLive()
        activate(id)
        resetActiveViews()
        if (foreground) startLive()
        scope.launch { refreshHome() }
    }

    /** Forget one machine (its key on this phone). The others keep working. */
    fun removeMachine(id: String) {
        val wasActive = id == activeId
        val left = store.removeMachine(id)
        val keep = _outbox.value.filterNot { (it.machineId ?: activeId) == id }
        _outbox.value = keep
        store.saveOutbox(keep)
        if (left.isEmpty()) {
            disconnectAll()
            return
        }
        syncMachines()
        if (wasActive) {
            stopLive()
            activate(store.activeMachineId)
            resetActiveViews()
            if (foreground) startLive()
            scope.launch { refreshHome() }
        }
    }

    // ---- pairing -----------------------------------------------------------------

    /**
     * Check the machine and the key, then add it to the paired machines
     * (pairing never replaces another one). Only the first machine is put in
     * use; an added one waits until the person picks it, so a pairing link
     * can never silently redirect what they send. Returns who we are there. Older hubs without `/whoami` are accepted when an
     * authenticated read works.
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
            val label = runCatching { probe.platforms() }.getOrNull()
                ?.let { p -> p.computer.ifBlank { p.machine } }
                ?.takeIf { it.isNotBlank() }
                ?.let { Machines.displayLabel(it, baseUrl) }
                ?: Machines.labelFromUrl(baseUrl)
            val first = store.loadMachines().isEmpty()
            val saved = store.addMachine(baseUrl, token, effectivePrincipal, name, label, makeActive = first)
            withContext(Dispatchers.Main) {
                syncMachines()
                // The first machine, or a fresh key for the one in use.
                if (first || saved.id == activeId) {
                    stopLive()
                    activate(saved.id)
                    resetActiveViews()
                    _whoami.value = who
                    if (foreground) startLive()
                }
                markReachable(saved.id, true)
            }
            who
        }

    /** Remove the machine in use from this phone. */
    fun disconnect() {
        activeId?.let { removeMachine(it) } ?: disconnectAll()
    }

    private fun disconnectAll() {
        stopLive()
        client = null
        activeId = null
        clients.clear()
        store.clearConfig()
        _config.value = null
        _machines.value = emptyList()
        _allMellows.value = emptyList()
        _everywhere.value = null
        _everywhereOn.value = false
        _whoami.value = null
        _home.value = null
        _network.value = null
        _agents.value = emptyList()
        _outbox.value = emptyList()
        _reachable.value = null
    }

    // ---- home --------------------------------------------------------------------

    /** Reload everything the Plaza needs in one round of parallel requests. */
    suspend fun refreshHome() {
        val c = client ?: return
        val id = activeId ?: return
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
                        if (c !== client) return@withContext
                        if (w != null) _whoami.value = w
                        if (n != null) _network.value = n
                        _agents.value = a
                        updateStatus(id) { it.copy(agents = a) }
                    }
                    store.saveAgentsCache(id, a)
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
            if (c !== client) return
            _home.value = model
            _homeError.value = null
            markReachable(id, true)
            flushOutbox()
        } catch (e: Exception) {
            if (c !== client) return
            val label = _machines.value.firstOrNull { it.machine.id == id }?.name.orEmpty()
            // Which read failed is unknown: a missing route on an older hub
            // or one bad endpoint is not the whole machine. Its Mellows list
            // (every hub has one) tells whether the machine itself is in trouble.
            val kind = (e as? MeshException)?.kind
            val failure: Throwable? = if (kind == MeshException.Kind.NETWORK || kind == MeshException.Kind.UNAUTHORIZED) {
                e
            } else {
                withContext(Dispatchers.IO) { runCatching { c.agents() } }.exceptionOrNull()
            }
            _homeError.value = when (failure?.let { markFailure(id, it) }) {
                MachineHealth.ADDRESS_LOST -> "The address of $label no longer leads to your hub: the tunnel may have closed. Pair it again, or remove it."
                MachineHealth.KEY_REFUSED -> "$label no longer knows this phone's key. Pair it again."
                else -> e.friendly()
            }
        }
        // While we are at it: how are the other machines?
        scope.launch { if (_everywhereOn.value) refreshEverywhere() else refreshOtherMachines() }
    }

    /**
     * Ask every paired machine, in parallel, for what its Plaza needs, and
     * build the Everywhere view. A machine that fails only changes its own
     * line: its Mellows stay listed from the phone's memory, asleep.
     */
    suspend fun refreshEverywhere() {
        val statuses = _machines.value
        if (statuses.isEmpty()) return
        val homes = coroutineScope {
            statuses.map { st ->
                async(Dispatchers.IO) {
                    val id = st.machine.id
                    val meThere = st.machine.principal ?: me
                    val result = run {
                        runCatching {
                            val c = clients[id] ?: throw MeshException(MeshException.Kind.UNAUTHORIZED, 401, "not paired")
                            coroutineScope {
                                val who = async { runCatching { c.whoami() }.getOrNull() }
                                val agents = async { c.agents() }
                                val tasks = async { runCatching { c.tasks() }.getOrDefault(emptyList()) }
                                val decisions = async { runCatching { c.decisions() }.getOrDefault(emptyList()) }
                                val history = async { runCatching { c.history(null, 200) }.getOrDefault(emptyList()) }
                                val net = async { runCatching { c.network() }.getOrNull() }
                                val a = agents.await()
                                val n = net.await()
                                val w = who.await()
                                Triple(
                                    a,
                                    n,
                                    HomeModelBuilder.build(
                                        HomeInputs(
                                            me = w?.principal ?: meThere,
                                            agents = a,
                                            tasks = tasks.await(),
                                            decisions = decisions.await(),
                                            history = history.await(),
                                            network = n,
                                            readMarkers = store.readMarkers(),
                                            nowMs = System.currentTimeMillis(),
                                            hour = LocalTime.now().hour,
                                        ),
                                    ),
                                )
                            }
                        }
                    }
                    result.onSuccess { (a, _, _) -> store.saveAgentsCache(id, a) }
                    withContext(Dispatchers.Main) {
                        result
                            .onSuccess { (a, n, _) ->
                                updateStatus(id) { s ->
                                    val name = n?.network?.name?.takeIf { it.isNotBlank() }
                                    s.copy(agents = a, machine = if (name != null) s.machine.copy(networkName = name) else s.machine)
                                }
                                markReachable(id, true)
                            }
                            .onFailure { markFailure(id, it) }
                    }
                    val now = withContext(Dispatchers.Main) { _machines.value.firstOrNull { it.machine.id == id } ?: st }
                    MachineHome(
                        machineId = id,
                        label = now.name,
                        networkName = result.getOrNull()?.second?.network?.name ?: now.machine.networkName,
                        health = now.health,
                        home = result.getOrNull()?.third,
                        cachedAgents = now.agents,
                        me = meThere,
                    )
                }
            }.awaitAll()
        }
        _everywhere.value = Everywhere.build(homes)
        _allConversations.value = homes.flatMap { h ->
            val key = ChatSpace.segmentKey(h.networkName, h.label)
            h.home?.conversations.orEmpty().map { PlacedConversation(it, h.machineId, key) }
        }
        flushOutbox()
    }

    /** Ask every other paired machine for its Mellows (in parallel). */
    suspend fun refreshOtherMachines() {
        val others = _machines.value.filter { !it.active }
        if (others.isEmpty()) return
        coroutineScope {
            others.map { st ->
                async(Dispatchers.IO) {
                    val id = st.machine.id
                    val c = clients[id] ?: return@async
                    val result = runCatching { c.agents() }
                    result.onSuccess { store.saveAgentsCache(id, it) }
                    withContext(Dispatchers.Main) {
                        result
                            .onSuccess { a ->
                                updateStatus(id) { it.copy(agents = a) }
                                markReachable(id, true)
                            }
                            .onFailure { e -> markFailure(id, e) }
                    }
                }
            }.awaitAll()
        }
        flushOutbox()
    }

    // ---- deferred messages -------------------------------------------------------

    /**
     * Send now through [machineId] (default: the machine in use), or — when
     * that machine is out of reach — keep the message on the phone and send
     * it when it answers. Returns the sent message, or null when it waits.
     * Errors other than "cannot reach it" are thrown as usual.
     */
    suspend fun sendOrQueue(conversationId: String, text: String, machineId: String? = null): ChatMessage? {
        val body = text.trim()
        require(body.isNotEmpty())
        val target = machineId ?: activeId
            ?: throw MeshException(MeshException.Kind.UNAUTHORIZED, 401, "not paired")
        val reach = _machines.value.firstOrNull { it.machine.id == target }?.reachable
        val waiting = _outbox.value.any { (it.machineId ?: activeId) == target }
        if (reach != false && !waiting) {
            try {
                val sent = sendVia(target, conversationId, body)
                markReachable(target, true)
                return sent
            } catch (e: MeshException) {
                if (e.kind != MeshException.Kind.NETWORK) throw e
                markReachable(target, false)
            }
        }
        val pending = PendingMessage(
            id = "pending_" + java.util.UUID.randomUUID().toString(),
            conversationId = conversationId,
            text = body,
            createdAtMs = System.currentTimeMillis(),
            machineId = target,
        )
        val next = _outbox.value + pending
        _outbox.value = next
        store.saveOutbox(next)
        return null
    }

    private suspend fun sendVia(machineId: String, conversationId: String, text: String): ChatMessage {
        if (machineId == activeId) return send(conversationId, text)
        val c = clients[machineId] ?: throw MeshException(MeshException.Kind.UNAUTHORIZED, 401, "not paired")
        val dir = directory
        val meThere = _machines.value.firstOrNull { it.machine.id == machineId }?.machine?.principal ?: me
        return withContext(Dispatchers.IO) { Chat.toChat(c.sendText(conversationId, text), meThere, dir) }
    }

    /**
     * Send what waited, in order per machine; a machine still out of reach
     * keeps its messages, the others' leave.
     */
    suspend fun flushOutbox() {
        if (_outbox.value.isEmpty()) return
        flushing.withLock {
            val blocked = HashSet<String>()
            for (p in _outbox.value.toList()) {
                val target = p.machineId ?: activeId ?: continue
                if (target in blocked) continue
                val c = clients[target]
                if (c != null) {
                    try {
                        withContext(Dispatchers.IO) { c.sendText(p.conversationId, p.text) }
                        markReachable(target, true)
                    } catch (e: MeshException) {
                        if (e.kind == MeshException.Kind.NETWORK) {
                            markReachable(target, false)
                            blocked += target
                            continue
                        }
                        // Refused for good (e.g. not allowed): do not retry forever.
                    } catch (_: Exception) {
                        blocked += target
                        continue
                    }
                }
                val rest = _outbox.value.filterNot { it.id == p.id }
                _outbox.value = rest
                store.saveOutbox(rest)
            }
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

    /**
     * Live messages of one machine matching [pattern], for as long as the
     * flow is collected (a call to a Mellow on any machine).
     */
    fun liveFrom(machineId: String, pattern: String): Flow<Message> = callbackFlow {
        val c = clients[machineId]
        if (c == null) {
            close()
            return@callbackFlow
        }
        val sub = c.subscribe(pattern, object : LiveListener {
            override fun onOpen() = markReachable(machineId, true)
            override fun onMessage(message: Message) {
                trySend(message)
            }
            override fun onClosed(error: Throwable?) {
                close()
            }
        })
        awaitClose { sub.close() }
    }.flowOn(Dispatchers.IO)

    // ---- decisions, agents, tasks ------------------------------------------------

    suspend fun decisions(): List<Decision> = io { it.decisions() }

    /** Only call after BiometricPrompt succeeded. Never queued: a live round trip or an error. */
    suspend fun respond(decisionId: String, optionId: String) = io { it.respondDecision(decisionId, optionId) }

    suspend fun loadAgents(): List<AgentRegistration> {
        val a = io { it.agents() }
        _agents.value = a
        activeId?.let { id ->
            store.saveAgentsCache(id, a)
            updateStatus(id) { it.copy(agents = a) }
        }
        return a
    }

    suspend fun taskGraph(scope: GraphScope): TaskGraph = io { it.taskGraph(scope) }

    /** AI platforms spotted on the hub's computer (owner only). */
    suspend fun platforms(): io.mellowmesh.core.PlatformsSnapshot = io { it.platforms() }

    /** One-tap connect of an AI app on the hub's computer. */
    suspend fun connectPlatform(platformId: String): io.mellowmesh.core.ConnectResult = io { it.connectPlatform(platformId) }

    /** Make a Mellow from a platform, then refresh the directory so it shows everywhere. */
    suspend fun createMellow(platformId: String, name: String, purpose: String): io.mellowmesh.core.MellowCreated {
        val made = io { it.createMellow(platformId, name, purpose) }
        runCatching { loadAgents() }
        return made
    }

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

    // ---- notes between AIs ---------------------------------------------------------

    suspend fun agentMail(): io.mellowmesh.core.MailPolicyView = io { it.agentMail() }

    suspend fun email(): io.mellowmesh.core.EmailView = io { it.email() }

    suspend fun inviteAi(request: io.mellowmesh.core.InviteRequest): io.mellowmesh.core.InviteLink = io { it.inviteAi(request) }

    suspend fun setEmail(patch: io.mellowmesh.core.EmailPatch): io.mellowmesh.core.EmailView = io { it.setEmail(patch) }

    suspend fun checkEmail(): io.mellowmesh.core.EmailCheck = io { it.checkEmail() }

    suspend fun setEmailPerson(change: io.mellowmesh.core.EmailPersonChange): io.mellowmesh.core.EmailView =
        io { it.setEmailPerson(change) }

    suspend fun setAgentMail(change: io.mellowmesh.core.MailChange): io.mellowmesh.core.MailPolicyView =
        io { it.setAgentMail(change) }

    /**
     * Set the same network rule on every other machine (each keeps its own
     * per-AI rules). Returns the names of the machines it could not reach.
     */
    suspend fun setNetworkMailEverywhere(rule: io.mellowmesh.core.MailRule): List<String> {
        val others = _machines.value.filter { !it.active }
        return coroutineScope {
            others.map { st ->
                async(Dispatchers.IO) {
                    val c = clients[st.machine.id] ?: return@async st.machine.label
                    runCatching { c.setAgentMail(io.mellowmesh.core.MailChange(rule = rule)) }
                        .fold({ null }, { st.machine.label })
                }
            }.awaitAll().filterNotNull()
        }
    }

    private fun requireClient(): MeshClient =
        client ?: throw MeshException(MeshException.Kind.UNAUTHORIZED, 401, "not paired")

    private suspend fun <T> io(block: (MeshClient) -> T): T = withContext(Dispatchers.IO) { block(requireClient()) }

    // ---- live --------------------------------------------------------------------

    /** Called by the process lifecycle: the WebSocket runs only in the foreground. */
    fun setForeground(value: Boolean) {
        foreground = value
        if (value) {
            startLive()
            startWatching()
            scope.launch { refreshHome() }
        } else {
            stopLive()
            watchJob?.cancel()
            watchJob = null
        }
    }

    /** Check the other machines every 30 s while the app is on screen. */
    private fun startWatching() {
        if (watchJob?.isActive == true) return
        watchJob = scope.launch {
            while (isActive) {
                delay(30_000)
                if (_machines.value.size > 1) refreshOtherMachines()
            }
        }
    }

    private fun startLive() {
        if (liveJob?.isActive == true || client == null) return
        liveJob = scope.launch {
            val backoff = Backoff()
            while (isActive) {
                val c = client ?: break
                val id = activeId ?: break
                val closed = CompletableDeferred<Throwable?>()
                val sub = withContext(Dispatchers.IO) {
                    c.subscribe("**", object : LiveListener {
                        override fun onOpen() {
                            backoff.reset()
                            _connected.value = true
                            markReachable(id, true)
                            scope.launch { flushOutbox() }
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
