package io.mellowmesh.core

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class InviteTest {
    @Test
    fun readsAnInvitation() {
        val json = """
            {"join_url":"https://hub.example/join/k7q2f-9xmd0-4hrf1-c8tw2-pz3a4",
             "message":"Join my MellowMesh network as my helper \"Grok\": open https://hub.example/join/k7q2f-9xmd0-4hrf1-c8tw2-pz3a4 and follow what it says.",
             "code":"ABC-DEF","url":"https://hub.example","network":"Léa's mesh",
             "principal":"agent://lea/grok","name":"Grok / GrokBot",
             "expires_at":"2026-11-22T15:00:00Z","opens":5,"reachable_from_web":true,"future":1}
        """.trimIndent()
        val l = MeshJson.decodeFromString(InviteLink.serializer(), json)
        assertTrue(l.message.contains(l.joinUrl))
        assertEquals(5, l.opens)
        assertEquals("agent://lea/grok", l.principal)
        assertTrue(l.reachableFromWeb)
    }

    @Test
    fun aRequestNamesAnAppOrAnAi() {
        assertEquals("""{"platform":"grok"}""", MeshJson.encodeToString(InviteRequest.serializer(), InviteRequest(platform = "grok")))
        assertEquals("""{"name":"Buzz"}""", MeshJson.encodeToString(InviteRequest.serializer(), InviteRequest(name = "Buzz")))
        assertEquals(listOf("grok", "chatgpt", "claude-web", "gemini-web", "mistral"), InviteCatalog.web.map { it.platform })
        assertTrue(InviteCatalog.validName("OpenBot"))
        assertFalse(InviteCatalog.validName("  "))
        assertFalse(InviteCatalog.validName("!!!"))
        assertFalse(InviteCatalog.validName("x".repeat(41)))
    }
}
