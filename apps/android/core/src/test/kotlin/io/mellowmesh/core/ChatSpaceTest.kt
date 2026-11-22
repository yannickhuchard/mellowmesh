package io.mellowmesh.core

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class ChatSpaceTest {
    private val me = "human://lea"
    private val now = 1_800_000_000_000L

    private fun agent(slug: String) = AgentRegistration(id = "agent://lea/$slug", name = slug.replaceFirstChar { it.uppercase() }, owner = me)
    private fun home(vararg a: AgentRegistration) = HomeModelBuilder.build(
        HomeInputs(me = me, agents = a.toList(), tasks = emptyList(), decisions = emptyList(), history = emptyList(), nowMs = now),
    )

    private fun conv(slug: String, at: Long) = ConversationSummary(
        id = Conversations.agentConversationId("agent://lea/$slug")!!,
        kind = ConversationKind.AGENT,
        title = slug,
        participants = emptyList(),
        lastMessagePreview = "hi",
        lastActivityMs = at,
        unread = 0,
        agentId = "agent://lea/$slug",
    )

    private val space = Everywhere.build(
        listOf(
            MachineHome("d", "Desktop", "Léa's mesh", MachineHealth.ANSWERS, home(agent("coder"), agent("ops")), emptyList(), me),
            MachineHome("l", "Laptop", "Léa's mesh", MachineHealth.ANSWERS, home(agent("writer")), emptyList(), me),
            MachineHome("t", "Team hub", "Platform team", MachineHealth.ANSWERS, home(agent("reviewer")), emptyList(), me),
            MachineHome("h", "Home", null, MachineHealth.RESTING, null, listOf(agent("chef")), me),
        ),
    )

    @Test
    fun everyNetworkIsASegmentAndEveryMellowIsThere() {
        val m = ChatSpace.build(space, emptyList(), emptyList())
        assertEquals(listOf("Léa's mesh", "Platform team", "Home"), m.segments.map { it.name })
        assertEquals(listOf(3, 1, 1), m.segments.map { it.mellows })
        assertEquals(5, m.everyone.size)
        assertEquals(5, m.total)
        // A segment whose machines all rest is not live.
        assertTrue(!m.segments.last().live)
    }

    @Test
    fun aSegmentNarrowsWhoIsThereAndTheConversations() {
        val convs = listOf(
            PlacedConversation(conv("coder", 3), "d", ChatSpace.segmentKey("Léa's mesh", "Desktop")),
            PlacedConversation(conv("reviewer", 9), "t", ChatSpace.segmentKey("Platform team", "Team hub")),
            PlacedConversation(conv("writer", 5), "l", ChatSpace.segmentKey("Léa's mesh", "Laptop")),
        )
        val all = ChatSpace.build(space, convs, emptyList())
        assertEquals(listOf("reviewer", "writer", "coder"), all.conversations.map { it.summary.title })
        val team = ChatSpace.build(space, convs, emptyList(), selected = "platform team")
        assertEquals(listOf("Reviewer"), team.everyone.map { it.card.name })
        assertEquals(listOf("reviewer"), team.conversations.map { it.summary.title })
        // An unknown segment shows everything rather than nothing.
        assertEquals(5, ChatSpace.build(space, convs, emptyList(), selected = "gone").everyone.size)
    }

    @Test
    fun theSameConversationThroughTwoAddressesIsListedOnce() {
        val key = ChatSpace.segmentKey("Léa's mesh", "Desktop")
        val convs = listOf(PlacedConversation(conv("coder", 3), "d", key), PlacedConversation(conv("coder", 3), "d2", key))
        assertEquals(1, ChatSpace.build(space, convs, emptyList()).conversations.size)
    }

    @Test
    fun aConversationWithNoMessageIsNotListedTwice() {
        val key = ChatSpace.segmentKey("Léa's mesh", "Laptop")
        val convs = listOf(PlacedConversation(conv("writer", 0), "l", key), PlacedConversation(conv("coder", 4), "d", key))
        val m = ChatSpace.build(space, convs, emptyList())
        assertEquals(listOf("coder"), m.conversations.map { it.summary.title })
        assertTrue(m.everyone.any { it.card.name == "Writer" })
    }

    @Test
    fun groupsAreThePersonsAcrossNetworks() {
        var g = MellowGroup("g1", "Coding crew")
        g = ChatSpace.add(g, "agent://lea/coder")
        g = ChatSpace.add(g, "agent://lea/reviewer")
        g = ChatSpace.add(g, "AGENT://lea/coder")
        assertEquals(2, g.members.size)
        val m = ChatSpace.build(space, emptyList(), listOf(g, MellowGroup("g2", "Empty")))
        val crew = m.groups.first()
        assertEquals(listOf("Coder", "Reviewer"), crew.members.map { it.card.name })
        assertEquals(setOf("d", "t"), crew.members.map { it.machineId }.toSet())
        assertEquals("No Mellow yet", m.groups.last().line)
        assertEquals(1, ChatSpace.remove(g, "agent://lea/reviewer").members.size)
    }

    @Test
    fun groupNamesAreClean() {
        assertEquals("Website launch", ChatSpace.cleanName("  Website launch "))
        assertNull(ChatSpace.cleanName("   "))
        assertEquals(ChatSpace.MAX_GROUP_NAME, ChatSpace.cleanName("x".repeat(80))!!.length)
    }

    @Test
    fun aMachineWithoutANetworkNameIsItsOwnSegment() {
        assertEquals("machine:home", ChatSpace.segmentKey(null, "Home"))
        assertEquals("léa's mesh", ChatSpace.segmentKey(" Léa's mesh ", "Desktop"))
    }
}
