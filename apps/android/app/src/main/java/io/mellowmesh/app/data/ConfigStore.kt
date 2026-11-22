package io.mellowmesh.app.data

import android.content.Context
import android.content.SharedPreferences
import android.os.Build
import android.util.Log
import androidx.security.crypto.EncryptedSharedPreferences
import androidx.security.crypto.MasterKeys
import io.mellowmesh.core.LadderSettings
import io.mellowmesh.core.MeshConfig
import io.mellowmesh.core.MeshJson

/**
 * Where the app keeps its settings. The hub key (token) lives in
 * EncryptedSharedPreferences backed by the Android Keystore; if the Keystore
 * is broken on a device we fall back to app-private storage (MODE_PRIVATE,
 * excluded from backups). The token is never logged.
 */
class ConfigStore(context: Context) {
    private val app = context.applicationContext

    private val secure: SharedPreferences = try {
        val masterKeyAlias = MasterKeys.getOrCreate(MasterKeys.AES256_GCM_SPEC)
        EncryptedSharedPreferences.create(
            "mellowmesh_secure",
            masterKeyAlias,
            app,
            EncryptedSharedPreferences.PrefKeyEncryptionScheme.AES256_SIV,
            EncryptedSharedPreferences.PrefValueEncryptionScheme.AES256_GCM,
        )
    } catch (e: Exception) {
        Log.w(TAG, "Keystore unavailable; using private storage (${e.javaClass.simpleName})")
        app.getSharedPreferences("mellowmesh_private", Context.MODE_PRIVATE)
    }

    private val plain: SharedPreferences = app.getSharedPreferences("mellowmesh_prefs", Context.MODE_PRIVATE)

    // ---- machines ------------------------------------------------------------------
    // A network can be spread over several computers; the phone keeps a key
    // for each one it was paired with. One of them is "in use" (Plaza,
    // boards); the others stay reachable for calls and deferred messages.

    private val machineList = kotlinx.serialization.builtins.ListSerializer(io.mellowmesh.core.SavedMachine.serializer())

    /** Every paired machine, oldest first (migrates the single-hub settings once). */
    fun loadMachines(): List<io.mellowmesh.core.SavedMachine> {
        val raw = secure.getString(KEY_MACHINES, null)
        if (raw != null) {
            return runCatching { MeshJson.decodeFromString(machineList, raw) }.getOrDefault(emptyList())
        }
        val url = secure.getString(KEY_URL, null) ?: return emptyList()
        val first = io.mellowmesh.core.SavedMachine(
            id = newMachineId(),
            baseUrl = url,
            token = secure.getString(KEY_TOKEN, null),
            principal = secure.getString(KEY_PRINCIPAL, null),
            networkName = secure.getString(KEY_NETWORK_NAME, null),
            label = io.mellowmesh.core.Machines.labelFromUrl(url),
            addedAtMs = System.currentTimeMillis(),
        )
        // The old single-hub cache belongs to that first machine.
        val legacyAgents = secure.getString(KEY_AGENTS_CACHE, null)
        val e = secure.edit()
            .putString(KEY_MACHINES, MeshJson.encodeToString(machineList, listOf(first)))
            .putString(KEY_ACTIVE_MACHINE, first.id)
            .remove(KEY_URL).remove(KEY_TOKEN).remove(KEY_PRINCIPAL).remove(KEY_NETWORK_NAME)
            .remove(KEY_AGENTS_CACHE)
        if (legacyAgents != null) e.putString(agentsKey(first.id), legacyAgents)
        e.commit()
        return listOf(first)
    }

    private fun saveMachines(list: List<io.mellowmesh.core.SavedMachine>) {
        secure.edit().putString(KEY_MACHINES, MeshJson.encodeToString(machineList, list)).apply()
    }

    var activeMachineId: String?
        get() = secure.getString(KEY_ACTIVE_MACHINE, null)
            ?.takeIf { id -> loadMachines().any { it.id == id } }
            ?: loadMachines().firstOrNull()?.id
        set(v) = secure.edit().putString(KEY_ACTIVE_MACHINE, v).apply()

    fun activeMachine(): io.mellowmesh.core.SavedMachine? {
        val id = activeMachineId ?: return null
        return loadMachines().firstOrNull { it.id == id }
    }

    fun configOf(m: io.mellowmesh.core.SavedMachine) = MeshConfig(
        baseUrl = m.baseUrl,
        token = m.token,
        principal = m.principal,
        deviceName = deviceName(),
    )

    /** The machine in use, as a client config. */
    fun loadConfig(): MeshConfig? = activeMachine()?.let { configOf(it) }

    fun networkName(): String? = activeMachine()?.networkName

    /**
     * Remember a machine just paired (or a fresh key for one already known),
     * putting it in use only when asked (the first machine). Returns it,
     * with its stable id.
     */
    fun addMachine(
        baseUrl: String,
        token: String?,
        principal: String?,
        networkName: String?,
        label: String,
        makeActive: Boolean,
    ): io.mellowmesh.core.SavedMachine {
        val incoming = io.mellowmesh.core.SavedMachine(
            id = newMachineId(),
            baseUrl = baseUrl,
            token = token,
            principal = principal,
            networkName = networkName,
            label = label,
            addedAtMs = System.currentTimeMillis(),
        )
        val list = io.mellowmesh.core.Machines.upsert(loadMachines(), incoming)
        saveMachines(list)
        val saved = list.first { io.mellowmesh.core.Machines.sameAddress(it.baseUrl, baseUrl) }
        if (makeActive) activeMachineId = saved.id
        return saved
    }

    /** Forget one machine (its key and caches). Returns the machines left. */
    fun removeMachine(id: String): List<io.mellowmesh.core.SavedMachine> {
        val left = loadMachines().filterNot { it.id == id }
        if (left.isEmpty()) {
            clearConfig()
            return emptyList()
        }
        saveMachines(left)
        secure.edit().remove(agentsKey(id)).apply()
        if (secure.getString(KEY_ACTIVE_MACHINE, null) == id) activeMachineId = left.first().id
        return left
    }

    fun clearConfig() {
        secure.edit().clear().apply()
        plain.edit().remove(KEY_READ_MARKERS).remove(KEY_DIGEST_COUNT).apply()
    }

    private fun newMachineId() = "m_" + java.util.UUID.randomUUID().toString().take(8)
    private fun agentsKey(machineId: String) = KEY_AGENTS_CACHE + "_" + machineId

    /**
     * The Plaza shows one machine (false) or every machine (true). Null:
     * never chosen — Everywhere as soon as there are two machines.
     */
    var everywhere: Boolean?
        get() = if (plain.contains(KEY_EVERYWHERE)) plain.getBoolean(KEY_EVERYWHERE, true) else null
        set(v) = plain.edit().apply { if (v == null) remove(KEY_EVERYWHERE) else putBoolean(KEY_EVERYWHERE, v) }.apply()

    /** The person's groups of Mellows (names and ids only: no key, kept in plain settings). */
    fun loadGroups(): List<io.mellowmesh.core.MellowGroup> = runCatching {
        val raw = plain.getString(KEY_GROUPS, null) ?: return emptyList()
        MeshJson.decodeFromString(kotlinx.serialization.builtins.ListSerializer(io.mellowmesh.core.MellowGroup.serializer()), raw)
    }.getOrDefault(emptyList())

    fun saveGroups(list: List<io.mellowmesh.core.MellowGroup>) {
        plain.edit().putString(
            KEY_GROUPS,
            MeshJson.encodeToString(kotlinx.serialization.builtins.ListSerializer(io.mellowmesh.core.MellowGroup.serializer()), list),
        ).apply()
    }

    fun loadRoutines(): List<io.mellowmesh.core.Routine> = runCatching {
        val raw = plain.getString(KEY_ROUTINES, null) ?: return emptyList()
        MeshJson.decodeFromString(kotlinx.serialization.builtins.ListSerializer(io.mellowmesh.core.Routine.serializer()), raw)
    }.getOrDefault(emptyList())

    fun saveRoutines(list: List<io.mellowmesh.core.Routine>) {
        plain.edit().putString(
            KEY_ROUTINES,
            MeshJson.encodeToString(kotlinx.serialization.builtins.ListSerializer(io.mellowmesh.core.Routine.serializer()), list),
        ).apply()
    }

    /** Last count of finished steps per journey (the step-done chime plays once). */
    var journeyDone: Map<String, Int>
        get() = runCatching {
            val raw = plain.getString(KEY_JOURNEY_DONE, null) ?: return emptyMap()
            MeshJson.decodeFromString(kotlinx.serialization.serializer<Map<String, Int>>(), raw)
        }.getOrDefault(emptyMap())
        set(v) = plain.edit().putString(KEY_JOURNEY_DONE, MeshJson.encodeToString(kotlinx.serialization.serializer<Map<String, Int>>(), v)).apply()

    /** Soft sounds for sent, received, joined (on by default; silent when the phone is). */
    var sounds: Boolean
        get() = plain.getBoolean(KEY_SOUNDS, true)
        set(v) = plain.edit().putBoolean(KEY_SOUNDS, v).apply()

    var tourDone: Boolean
        get() = plain.getBoolean(KEY_TOUR, false)
        set(v) = plain.edit().putBoolean(KEY_TOUR, v).apply()

    var ladder: LadderSettings
        get() = plain.getString(KEY_LADDER, null)?.let {
            try {
                MeshJson.decodeFromString(LadderSettings.serializer(), it)
            } catch (_: Exception) {
                null
            }
        } ?: LadderSettings()
        set(v) = plain.edit().putString(KEY_LADDER, MeshJson.encodeToString(LadderSettings.serializer(), v)).apply()

    var lastDigestDay: Long?
        get() = plain.getLong(KEY_DIGEST_DAY, -1L).takeIf { it >= 0 }
        set(v) = plain.edit().putLong(KEY_DIGEST_DAY, v ?: -1L).apply()

    var digestCount: Int
        get() = plain.getInt(KEY_DIGEST_COUNT, 0)
        set(v) = plain.edit().putInt(KEY_DIGEST_COUNT, v).apply()

    fun readMarkers(): Map<String, Long> {
        val raw = plain.getString(KEY_READ_MARKERS, null) ?: return emptyMap()
        return raw.split('\n').mapNotNull { line ->
            val i = line.lastIndexOf('=')
            if (i <= 0) null else line.substring(i + 1).toLongOrNull()?.let { line.substring(0, i) to it }
        }.toMap()
    }

    fun markRead(conversationId: String, upToMs: Long) {
        val m = readMarkers().toMutableMap()
        if ((m[conversationId] ?: 0L) >= upToMs) return
        m[conversationId] = upToMs
        plain.edit().putString(KEY_READ_MARKERS, m.entries.joinToString("\n") { "${it.key}=${it.value}" }).apply()
    }

    /**
     * The Mellows (agents) this phone last saw, so they stay on screen —
     * and callable — while the network is out of reach. Kept with the
     * other secrets: who you work with is private.
     */
    fun loadAgentsCache(machineId: String): List<io.mellowmesh.core.AgentRegistration> = runCatching {
        val raw = secure.getString(agentsKey(machineId), null) ?: return emptyList()
        io.mellowmesh.core.MeshJson.decodeFromString(
            kotlinx.serialization.builtins.ListSerializer(io.mellowmesh.core.AgentRegistration.serializer()),
            raw,
        )
    }.getOrDefault(emptyList())

    fun saveAgentsCache(machineId: String, agents: List<io.mellowmesh.core.AgentRegistration>) {
        val raw = io.mellowmesh.core.MeshJson.encodeToString(
            kotlinx.serialization.builtins.ListSerializer(io.mellowmesh.core.AgentRegistration.serializer()),
            agents,
        )
        secure.edit().putString(agentsKey(machineId), raw).apply()
    }

    /** Messages waiting for the network, oldest first (encrypted at rest). */
    fun loadOutbox(): List<io.mellowmesh.core.PendingMessage> = runCatching {
        val raw = secure.getString(KEY_OUTBOX, null) ?: return emptyList()
        io.mellowmesh.core.MeshJson.decodeFromString(
            kotlinx.serialization.builtins.ListSerializer(io.mellowmesh.core.PendingMessage.serializer()),
            raw,
        )
    }.getOrDefault(emptyList())

    fun saveOutbox(items: List<io.mellowmesh.core.PendingMessage>) {
        val raw = io.mellowmesh.core.MeshJson.encodeToString(
            kotlinx.serialization.builtins.ListSerializer(io.mellowmesh.core.PendingMessage.serializer()),
            items,
        )
        secure.edit().putString(KEY_OUTBOX, raw).apply()
    }

    private fun deviceName(): String = (Build.MODEL ?: "Android").take(64)

    companion object {
        private const val TAG = "MellowMesh"
        private const val KEY_URL = "hub_url"
        private const val KEY_TOKEN = "hub_token"
        private const val KEY_PRINCIPAL = "principal"
        private const val KEY_NETWORK_NAME = "network_name"
        private const val KEY_TOUR = "tour_done"
        private const val KEY_LADDER = "ladder"
        private const val KEY_EVERYWHERE = "plaza_everywhere"
        private const val KEY_GROUPS = "mellow_groups"
        private const val KEY_SOUNDS = "sounds"
        private const val KEY_ROUTINES = "routines"
        private const val KEY_JOURNEY_DONE = "journey_done"
        private const val KEY_DIGEST_DAY = "digest_day"
        private const val KEY_DIGEST_COUNT = "digest_count"
        private const val KEY_READ_MARKERS = "read_markers"
        private const val KEY_AGENTS_CACHE = "agents_cache"
        private const val KEY_OUTBOX = "outbox"
        private const val KEY_MACHINES = "machines"
        private const val KEY_ACTIVE_MACHINE = "active_machine"
    }
}
