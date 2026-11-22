package io.mellowmesh.core

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class MailPolicyTest {
    private val ownerView = """
        {"network":{"mode":"any"},
         "per_ai":{"agent://y/grok":{"mode":"only","boxes":["agent://y/coder"]}},
         "invited":[{"id":"agent://y/grok","name":"Grok"},{"id":"agent://y/claude_code","name":"Claude Code"}],
         "mailboxes":[{"id":"agent://y/coder","name":"Coder"},{"id":"agent://y/finance","name":"Finance"}],
         "future_field":1}
    """.trimIndent()

    @Test
    fun readsTheOwnersViewAndWhichRuleEachAiFollows() {
        val v = MeshJson.decodeFromString(MailPolicyView.serializer(), ownerView)
        assertEquals(MailMode.ANY, v.network.kind)
        val (grok, own) = v.ruleFor("agent://y/grok")
        assertTrue(own)
        assertEquals(MailMode.ONLY, grok.kind)
        assertEquals(listOf("agent://y/coder"), grok.boxes)
        val (claude, ownClaude) = v.ruleFor("agent://y/claude_code")
        assertFalse(ownClaude)
        assertEquals(MailMode.ANY, claude.kind)
        assertEquals("Coder", v.nameOf("agent://y/coder"))
        assertEquals("ghost", v.nameOf("agent://y/ghost"))
    }

    @Test
    fun aRuleKeepsBoxesOnlyForOnly() {
        assertEquals(emptyList<String>(), MailRule.of(MailMode.NONE, listOf("agent://y/coder")).boxes)
        assertEquals(
            listOf("agent://y/a", "agent://y/b"),
            MailRule.of(MailMode.ONLY, listOf("agent://y/b", "agent://y/a", "agent://y/b")).boxes,
        )
        assertEquals(MailMode.ANY, MailRule(mode = "unknown").kind)
    }

    @Test
    fun changesAreSentAsTheHubExpects() {
        val network = MeshJson.encodeToString(MailChange.serializer(), MailChange(rule = MailRule.of(MailMode.NONE)))
        assertEquals("""{"rule":{"mode":"none","boxes":[]}}""", network)
        val follow = MeshJson.encodeToString(MailChange.serializer(), MailChange(forAi = "agent://y/grok"))
        assertEquals("""{"for":"agent://y/grok"}""", follow)
    }
}
