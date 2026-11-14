package io.mellowmesh.core

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertThrows
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class PairingTest {
    private val ok = "mellowmesh-pair:?v=2&url=http%3A%2F%2F192.168.1.20%3A40000&token=mm_abc123&name=Yannick%27s%20mesh&principal=human%3A%2F%2Fyannick"

    @Test
    fun parsesV2Link() {
        val p = Pairing.parse(ok)
        assertEquals(2, p.version)
        assertEquals("http://192.168.1.20:40000", p.baseUrl)
        assertEquals("mm_abc123", p.token)
        assertEquals("Yannick's mesh", p.networkName)
        assertEquals("human://yannick", p.principal)
        assertFalse(p.toString().contains("mm_abc123"), "token must not leak via toString")
    }

    @Test
    fun optionalFieldsAndCase() {
        val p = Pairing.parse("MellowMesh-Pair:?v=2&url=https://hub.example.com/&token=t0k")
        assertEquals("https://hub.example.com", p.baseUrl)
        assertNull(p.networkName)
        assertNull(p.principal)
        // Unknown parameters are ignored for forward compatibility.
        assertEquals("t0k", Pairing.parse("mellowmesh-pair:?v=2&url=https://h.example&token=t0k&fp=abc").token)
    }

    @Test
    fun rejectsBadLinks() {
        val bad = listOf(
            "https://hub.example.com",
            "mellowmesh-pair:v=2&url=x",
            "mellowmesh-pair://?v=2&url=http://a&token=t",
            "mellowmesh-pair:?url=http://a&token=t",
            "mellowmesh-pair:?v=3&url=http://a&token=t",
            "mellowmesh-pair:?v=2&token=t",
            "mellowmesh-pair:?v=2&url=http://a",
            "mellowmesh-pair:?v=2&url=http://a&token=",
            "mellowmesh-pair:?v=2&url=http://a&token=has%20space",
            "mellowmesh-pair:?v=2&url=ftp://a&token=t",
            "mellowmesh-pair:?v=2&url=http://u:p@a&token=t",
            "mellowmesh-pair:?v=2&url=http://a?x=1&token=t",
            "mellowmesh-pair:?v=2&url=http://a&token=t&token=u",
            "mellowmesh-pair:?v=2&url=http://a&token=t#frag",
            "mellowmesh-pair:?v=2&url=http://a&token=t&principal=nobody",
            "mellowmesh-pair:?relay=https://r.example&hub=h&code=123456&fp=00",
            "mellowmesh-pair:?v=2&url=http://a&token=" + "x".repeat(600),
            "mellowmesh-pair:?v=2&url=%ZZ&token=t",
        )
        for (b in bad) assertThrows(PairingException::class.java, { Pairing.parse(b) }, b)
    }

    @Test
    fun relayLinksGetAClearMessage() {
        val e = assertThrows(PairingException::class.java) {
            Pairing.parse("mellowmesh-pair:?v=1&relay=https://r.example&hub=h&code=123456&fp=00")
        }
        assertTrue(e.message!!.contains("relay"))
    }

    @Test
    fun manualUrls() {
        assertEquals("http://192.168.1.20:40000", Pairing.validateBaseUrl("192.168.1.20:40000"))
        assertEquals("https://x.trycloudflare.com", Pairing.validateBaseUrl(" https://x.trycloudflare.com/ "))
        assertEquals("https://relay.example/hub/abc", Pairing.validateBaseUrl("https://relay.example/hub/abc"))
        assertThrows(PairingException::class.java) { Pairing.validateBaseUrl("") }
        assertThrows(PairingException::class.java) { Pairing.validateBaseUrl("gopher://x") }
    }

    @Test
    fun urlSafety() {
        assertEquals(UrlSafety.SECURE, Pairing.safety("https://hub.example.com"))
        assertEquals(UrlSafety.LOOPBACK, Pairing.safety("http://127.0.0.1:40000"))
        assertEquals(UrlSafety.LOOPBACK, Pairing.safety("http://localhost:40000"))
        for (h in listOf("10.0.0.5", "172.16.3.4", "172.31.255.1", "192.168.1.20", "100.101.1.2", "169.254.0.9", "mypc.local", "desktop", "[fd12::1]")) {
            assertEquals(UrlSafety.PRIVATE_LAN, Pairing.safety("http://$h:40000"), h)
        }
        for (h in listOf("8.8.8.8", "172.32.0.1", "hub.example.com", "100.128.0.1")) {
            assertEquals(UrlSafety.PUBLIC_PLAINTEXT, Pairing.safety("http://$h:40000"), h)
        }
    }
}
