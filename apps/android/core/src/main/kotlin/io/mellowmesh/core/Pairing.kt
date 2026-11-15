package io.mellowmesh.core

import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import java.net.URLDecoder

/**
 * Pairing link v2 (docs/network.md):
 *
 * ```
 * mellowmesh-pair:?v=2&url=<percent-encoded base url>&token=<token>&name=<network name>&principal=<percent-encoded principal>
 * ```
 *
 * `url` and `token` are required; `name` and `principal` are optional. Each
 * parameter may appear once. Unknown parameters are ignored (forward
 * compatible); the relay form (v1: `relay`, `hub`, `code`, `fp`) is recognized
 * and refused with a clear message because the app talks to the hub directly.
 */
data class PairingInfo(
    val version: Int,
    /** Hub base URL without trailing slash. */
    val baseUrl: String,
    val token: String,
    val networkName: String?,
    val principal: String?,
) {
    /** Never print the token. */
    override fun toString(): String =
        "PairingInfo(version=$version, baseUrl=$baseUrl, token=***, networkName=$networkName, principal=$principal)"
}

class PairingException(message: String) : IllegalArgumentException(message)

/** How safe a hub URL is to use with a bearer token. */
enum class UrlSafety {
    /** https: encrypted. */
    SECURE,
    /** http to this device (loopback). */
    LOOPBACK,
    /** http on a private / home network (Wi-Fi). Acceptable at home. */
    PRIVATE_LAN,
    /** http over the internet: the token travels in clear. Warn loudly. */
    PUBLIC_PLAINTEXT,
}

object Pairing {
    const val SCHEME = "mellowmesh-pair"
    private const val MAX_URI = 2048
    private const val MAX_URL = 512
    private const val MAX_TOKEN = 512
    private const val MAX_NAME = 64
    private const val MAX_PRINCIPAL = 256

    private fun bad(msg: String): Nothing = throw PairingException(msg)

    /** Parse and validate a pairing link. Throws [PairingException] with a plain-language message. */
    fun parse(link: String): PairingInfo {
        val uri = link.trim()
        if (uri.length > MAX_URI) bad("This link is too long to be a pairing link.")
        val colon = uri.indexOf(':')
        if (colon <= 0) bad("This is not a MellowMesh pairing link.")
        val scheme = uri.substring(0, colon)
        if (!scheme.equals(SCHEME, ignoreCase = true)) bad("This is not a MellowMesh pairing link.")
        val rest = uri.substring(colon + 1)
        if (!rest.startsWith("?")) bad("The pairing link is damaged (expected 'mellowmesh-pair:?…').")
        val query = rest.substring(1)
        if ('#' in query) bad("The pairing link is damaged (it has a '#').")

        val params = HashMap<String, String>()
        for (pair in query.split('&')) {
            if (pair.isEmpty()) continue
            val eq = pair.indexOf('=')
            val rawKey = if (eq >= 0) pair.substring(0, eq) else pair
            val rawValue = if (eq >= 0) pair.substring(eq + 1) else ""
            val key = decode(rawKey)
            val value = decode(rawValue)
            if (params.containsKey(key)) bad("The pairing link is damaged ('$key' appears twice).")
            params[key] = value
        }

        val v = params["v"]
        if (v == null && ("relay" in params || "hub" in params || "code" in params)) {
            bad("This link pairs through a relay, which this app does not support yet. Ask for a direct pairing link (mellowmesh pair --url …).")
        }
        val version = when (v) {
            "2" -> 2
            "1" -> bad("This link pairs through a relay, which this app does not support yet. Ask for a direct pairing link (mellowmesh pair --url …).")
            null -> bad("The pairing link has no version.")
            else -> bad("This pairing link is from a newer version of MellowMesh. Please update the app.")
        }

        val baseUrl = validateBaseUrl(params["url"] ?: bad("The pairing link has no address ('url')."))
        val token = params["token"]?.trim() ?: bad("The pairing link has no key ('token').")
        validateToken(token)
        val name = params["name"]?.let { n ->
            val clean = Sanitize.line(n.trim())
            if (clean.isEmpty()) null else if (clean.length > MAX_NAME) clean.take(MAX_NAME) else clean
        }
        val principal = params["principal"]?.trim()?.takeIf { it.isNotEmpty() }?.let { p ->
            if (p.length > MAX_PRINCIPAL || !p.contains("://") || p.any { it.isWhitespace() || it.isISOControl() }) {
                bad("The pairing link has an invalid identity ('principal').")
            }
            p
        }
        return PairingInfo(version, baseUrl, token, name, principal)
    }

    private fun decode(s: String): String = try {
        URLDecoder.decode(s, "UTF-8")
    } catch (e: IllegalArgumentException) {
        bad("The pairing link is damaged (bad % escape).")
    }

    fun validateToken(token: String) {
        if (token.isEmpty()) bad("The key (token) is empty.")
        if (token.length > MAX_TOKEN) bad("The key (token) is too long.")
        if (token.any { it.isWhitespace() || it.isISOControl() || it.code > 126 }) {
            bad("The key (token) contains characters that are not allowed.")
        }
    }

    /**
     * Normalize and validate a hub base URL typed by hand or read from a link:
     * http(s) only, a host, no credentials, query or fragment. A missing scheme
     * means http (home Wi-Fi); a missing port stays as typed.
     */
    fun validateBaseUrl(raw: String): String {
        var s = raw.trim()
        if (s.isEmpty()) bad("Please enter the address of your hub.")
        if (s.length > MAX_URL) bad("The address is too long.")
        if (!s.contains("://")) s = "http://$s"
        val lowerScheme = s.substringBefore("://").lowercase()
        if (lowerScheme != "http" && lowerScheme != "https") bad("The address must start with http:// or https://.")
        if (s.substringAfter("://").substringBefore('/').contains('@')) bad("The address must not contain a user name or password.")
        if ('?' in s || '#' in s) bad("The address must not contain '?' or '#'.")
        val url: HttpUrl = s.toHttpUrlOrNull() ?: bad("This address does not look right.")
        if (url.host.isEmpty()) bad("The address has no host.")
        return url.toString().trimEnd('/')
    }

    /** Classify how safe it is to send a token to `baseUrl`. */
    fun safety(baseUrl: String): UrlSafety {
        val url = baseUrl.toHttpUrlOrNull() ?: return UrlSafety.PUBLIC_PLAINTEXT
        if (url.isHttps) return UrlSafety.SECURE
        val host = url.host.lowercase().trim('[', ']')
        return when {
            isLoopback(host) -> UrlSafety.LOOPBACK
            isPrivate(host) -> UrlSafety.PRIVATE_LAN
            else -> UrlSafety.PUBLIC_PLAINTEXT
        }
    }

    private fun isLoopback(host: String): Boolean =
        host == "localhost" || host.endsWith(".localhost") || host == "::1" || host.startsWith("127.")

    private fun ipv4(host: String): IntArray? {
        val parts = host.split('.')
        if (parts.size != 4) return null
        val out = IntArray(4)
        for ((i, p) in parts.withIndex()) {
            if (p.isEmpty() || p.length > 3 || !p.all { it.isDigit() }) return null
            val n = p.toInt()
            if (n > 255) return null
            out[i] = n
        }
        return out
    }

    /** RFC 1918, link-local, CGNAT / Tailscale (100.64/10), IPv6 ULA / link-local, and home names. */
    fun isPrivate(host: String): Boolean {
        val h = host.lowercase().trim('[', ']')
        ipv4(h)?.let { ip ->
            return ip[0] == 10 ||
                (ip[0] == 172 && ip[1] in 16..31) ||
                (ip[0] == 192 && ip[1] == 168) ||
                (ip[0] == 169 && ip[1] == 254) ||
                (ip[0] == 100 && ip[1] in 64..127)
        }
        if (':' in h) {
            return h.startsWith("fc") || h.startsWith("fd") || h.startsWith("fe8") ||
                h.startsWith("fe9") || h.startsWith("fea") || h.startsWith("feb")
        }
        return h.endsWith(".local") || h.endsWith(".lan") || h.endsWith(".home.arpa") || h.endsWith(".internal") ||
            !h.contains('.')
    }
}
