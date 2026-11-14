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

    fun loadConfig(): MeshConfig? {
        val url = secure.getString(KEY_URL, null) ?: return null
        return MeshConfig(
            baseUrl = url,
            token = secure.getString(KEY_TOKEN, null),
            principal = secure.getString(KEY_PRINCIPAL, null),
            deviceName = deviceName(),
        )
    }

    fun networkName(): String? = secure.getString(KEY_NETWORK_NAME, null)

    fun saveConfig(baseUrl: String, token: String?, principal: String?, networkName: String?) {
        secure.edit()
            .putString(KEY_URL, baseUrl)
            .putString(KEY_TOKEN, token)
            .putString(KEY_PRINCIPAL, principal)
            .putString(KEY_NETWORK_NAME, networkName)
            .apply()
    }

    fun updatePrincipal(principal: String?) {
        secure.edit().putString(KEY_PRINCIPAL, principal).apply()
    }

    fun clearConfig() {
        secure.edit().clear().apply()
        plain.edit().remove(KEY_READ_MARKERS).remove(KEY_DIGEST_COUNT).apply()
    }

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

    private fun deviceName(): String = (Build.MODEL ?: "Android").take(64)

    companion object {
        private const val TAG = "MellowMesh"
        private const val KEY_URL = "hub_url"
        private const val KEY_TOKEN = "hub_token"
        private const val KEY_PRINCIPAL = "principal"
        private const val KEY_NETWORK_NAME = "network_name"
        private const val KEY_TOUR = "tour_done"
        private const val KEY_LADDER = "ladder"
        private const val KEY_DIGEST_DAY = "digest_day"
        private const val KEY_DIGEST_COUNT = "digest_count"
        private const val KEY_READ_MARKERS = "read_markers"
    }
}
