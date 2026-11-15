package io.mellowmesh.core

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class ConversationsTest {
    private fun kind(t: String) = Conversations.classify(t)?.kind

    @Test
    fun agentInboxAndOutbox() {
        val c = Conversations.classify("_agent.yannick.coder.inbox")!!
        assertEquals(ConversationKind.AGENT, c.kind)
        assertEquals("_agent.yannick.coder", c.id)
        assertEquals("agent://yannick/coder", c.agentId)
        assertEquals("_agent.yannick.coder", Conversations.classify("_agent.yannick.coder.outbox")!!.id)
        assertNull(kind("_agent.yannick.coder.heartbeat"))
        assertNull(kind("_agent.coder.inbox"))
        assertEquals("_agent.yannick.coder", Conversations.agentConversationId("agent://yannick/coder"))
        assertNull(Conversations.agentConversationId("agent://solo"))
    }

    @Test
    fun squadsPeopleGroupsOrgsTeams() {
        assertEquals(ConversationKind.SQUAD, kind("_project.apollo"))
        assertEquals("_project.apollo", Conversations.classify("_project.apollo.build.logs")!!.id)
        assertEquals(ConversationKind.SQUAD, kind("_forum.general"))
        assertEquals(ConversationKind.GROUP, kind("_chat.group.launch"))
        assertEquals(ConversationKind.ORGANIZATION, kind("_org.acme.announcements"))
        val dm = Conversations.classify("_chat.dm.zoe.adam")!!
        assertEquals(ConversationKind.PERSON, dm.kind)
        assertEquals("_chat.dm.adam.zoe", dm.id)
        assertEquals(listOf("adam", "zoe"), dm.dmHandles)
        assertNull(kind("_chat.dm.adam.adam"))
        assertNull(kind("_chat.dm.a.b.c"))

        val team = Conversations.classify("_team.home.chat")!!
        assertEquals(ConversationKind.TEAM, team.kind)
        assertEquals("_team.home", team.id)
        assertEquals("home", team.teamId)
        assertEquals(ConversationKind.TEAM, kind("_team.home"))
        assertEquals(ConversationKind.TEAM, kind("_team.home.files.x"))
    }

    @Test
    fun nonConversations() {
        for (t in listOf("_task.t1.progress", "_decision.d1.requested", "_system.registry.agent", "builds.main",
            "_project", "_project.*", "_project..x", "", "_team", "_team.**")) {
            assertNull(kind(t), t)
        }
    }

    @Test
    fun caseInsensitiveRoots() {
        assertEquals("_project.apollo", Conversations.classify("_Project.Apollo.Chat")!!.id)
    }

    @Test
    fun routingAndHistory() {
        val a = Conversations.parseId("_agent.yannick.coder")!!
        assertEquals("_agent.yannick.coder.inbox", Conversations.sendTopic(a))
        assertEquals("_agent.yannick.coder.*", Conversations.historyPattern(a))
        assertTrue(Conversations.isChatTopic(a, "_agent.yannick.coder.outbox"))
        assertFalse(Conversations.isChatTopic(a, "_agent.yannick.coder.heartbeat"))

        val p = Conversations.parseId("_project.apollo")!!
        assertEquals("_project.apollo", Conversations.sendTopic(p))
        assertEquals("_project.apollo.**", Conversations.historyPattern(p))

        val t = Conversations.parseId("_team.home")!!
        assertEquals("_team.home.chat", Conversations.sendTopic(t))
        assertEquals("_team.home.**", Conversations.historyPattern(t))

        assertNull(Conversations.parseId("_project.apollo.chat"))
        assertNull(Conversations.parseId("_task.x"))
        assertNull(Conversations.parseId("_chat.dm.zoe.adam"))
        assertEquals(ConversationKind.PERSON, Conversations.parseId("_chat.dm.adam.zoe")!!.kind)
        assertEquals("_chat.dm.adam.zoe", Conversations.historyPattern(Conversations.parseId("_chat.dm.adam.zoe")!!))
    }

    @Test
    fun dmIds() {
        assertEquals("_chat.dm.adam.zoe", Conversations.dmConversationId("human://Zoe", "adam"))
        assertNull(Conversations.dmConversationId("a", "a"))
        assertNull(Conversations.dmConversationId("a.b", "c"))
    }

    @Test
    fun squadBecomesGroupWhenAnotherHumanPosts() {
        val c = Conversations.classify("_project.apollo")!!
        assertEquals(ConversationKind.SQUAD, Conversations.effectiveKind(c, listOf("agent://y/a", "human://me"), "human://me"))
        assertEquals(ConversationKind.GROUP, Conversations.effectiveKind(c, listOf("human://zoe"), "human://me"))
        assertEquals(ConversationKind.GROUP, Conversations.effectiveKind(c, listOf("telegram://42"), "human://me"))
    }

    @Test
    fun titles() {
        val dir = Directory(mapOf("agent://y/coder" to "Coder Bot"))
        assertEquals("Coder Bot", Conversations.title(Conversations.classify("_agent.y.coder.inbox")!!, "human://y", dir))
        assertEquals("Release train", Conversations.title(Conversations.classify("_project.release-train")!!, "human://y", dir))
        assertEquals("zoe", Conversations.title(Conversations.classify("_chat.dm.y.zoe")!!, "human://y", dir))
        assertEquals("Home sweet", Conversations.title(Conversations.classify("_team.home.chat")!!, "human://y", dir, mapOf("home" to "Home sweet")))
        assertEquals("Home", Conversations.title(Conversations.classify("_team.home.chat")!!, "human://y", dir))
    }

    @Test
    fun summariesGroupAndCountUnread() {
        val me = "human://me"
        val history = listOf(
            Message(id = "1", topic = "_agent.me.coder.inbox", from = me, timestamp = "2026-01-01T10:00:00Z", body = "hi"),
            Message(id = "2", topic = "_agent.me.coder.outbox", from = "agent://me/coder", timestamp = "2026-01-01T10:01:00Z", body = "hello!"),
            Message(id = "2", topic = "_agent.me.coder.outbox", from = "agent://me/coder", timestamp = "2026-01-01T10:01:00Z", body = "dup"),
            Message(id = "3", topic = "_project.apollo.chat", from = "human://zoe", timestamp = "2026-01-01T11:00:00Z", body = "  multi\n line  "),
            Message(id = "4", topic = "_task.x.progress", from = "agent://me/coder", timestamp = "2026-01-01T12:00:00Z"),
        )
        val dir = Directory(mapOf("agent://me/coder" to "Coder"))
        val list = Chat.summaries(history, me, dir, myAgentIds = listOf("agent://me/coder", "agent://me/writer"),
            teams = listOf(Team(id = "home", name = "Home")))
        assertEquals(listOf("_project.apollo", "_agent.me.coder"), list.take(2).map { it.id })
        val apollo = list[0]
        assertEquals(ConversationKind.GROUP, apollo.kind)
        assertEquals("multi line", apollo.lastMessagePreview)
        val coder = list[1]
        assertEquals(1, coder.unread)
        assertEquals("Coder", coder.title)
        assertEquals("agent://me/coder", coder.participants.first().id)
        assertTrue(list.any { it.id == "_agent.me.writer" && it.lastActivityMs == 0L })
        assertTrue(list.any { it.id == "_team.home" && it.kind == ConversationKind.TEAM })
    }
}
