package io.mellowmesh.core

import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class HomeModelTest {
    private val me = "human://yannick"
    private val now = Time.parseMs("2026-11-14T12:00:00Z")!!
    private fun at(minAgo: Long) = Time.formatMs(now - minAgo * 60_000)

    private val agents = listOf(
        AgentRegistration("agent://yannick/coder", "Coder", me, "autonomous", listOf("code", "review")),
        AgentRegistration("agent://yannick/writer", "Writer", me, "autonomous", listOf("writing", "email")),
        AgentRegistration("agent://yannick/tutor", "Tutor", me, "autonomous", listOf("tutor")),
        AgentRegistration("agent://yannick/ops", "Ops", me, "autonomous", listOf("deploy")),
        AgentRegistration("agent://yannick/sleepy", "Sleepy", me, "autonomous", emptyList()),
        AgentRegistration("agent://zoe/helper", "Zoe's helper", "human://zoe", "autonomous", listOf("budget")),
    )

    private fun inputs(
        tasks: List<Task> = emptyList(),
        decisions: List<Decision> = emptyList(),
        history: List<Message> = emptyList(),
    ) = HomeInputs(
        me = me, agents = agents, tasks = tasks, decisions = decisions, history = history,
        network = NetworkOverview(
            Network(name = "Yannick's mesh", defaultAccess = Access.CLOSED),
            spaces = listOf(Space("family", access = Access.CLOSED), Space("public", access = Access.OPEN)),
            teams = listOf(Team("home", "Home")),
        ),
        nowMs = now, hour = 9,
    )

    @Test
    fun moodsFollowActivity() {
        val tasks = listOf(
            Task("t1", "Fix login", status = TaskStatus.IN_PROGRESS, claimedBy = "agent://yannick/coder"),
            Task("t2", "Draft letter", status = TaskStatus.COMPLETED, claimedBy = "agent://yannick/writer", completedAt = at(3)),
        )
        val decisions = listOf(
            Decision("d1", "Deploy?", "Deploy now?", "agent://yannick/ops", me, "requested",
                listOf(DecisionOption("y", "Yes", outcome = DecisionOutcome.APPROVE))),
            Decision("d2", "Old", "", "agent://yannick/ops", me, "approved"),
        )
        val history = listOf(
            Message(id = "h1", topic = "_agent.yannick.writer.outbox", from = "agent://yannick/writer", timestamp = at(3)),
            Message(id = "h2", topic = "_agent.yannick.tutor.inbox", from = me, timestamp = at(1), body = "quiz me"),
            Message(id = "h3", topic = "_agent.yannick.tutor.heartbeat", from = "agent://yannick/tutor", timestamp = at(2)),
            Message(id = "h4", topic = "_task.t1.progress", from = "agent://yannick/coder", timestamp = at(1),
                payload = buildJsonObject { put("percent", 40) }),
            Message(id = "h5", topic = "_agent.yannick.ops.outbox", from = "agent://yannick/ops", timestamp = at(1), body = "Need an OK"),
        )
        val home = HomeModelBuilder.build(inputs(tasks, decisions, history))
        val byName = home.helpers.associateBy { it.name }
        assertEquals(Mood.WORKING, byName.getValue("Coder").mood)
        assertEquals(0.4f, byName.getValue("Coder").progress!!, 0.001f)
        assertEquals("Fix login", byName.getValue("Coder").currentTask)
        assertEquals(Mood.DONE, byName.getValue("Writer").mood)
        assertEquals(Mood.THINKING, byName.getValue("Tutor").mood)
        assertEquals(Mood.WAITING, byName.getValue("Ops").mood)
        assertEquals(Mood.ASLEEP, byName.getValue("Sleepy").mood)
        assertFalse(byName.getValue("Zoe's helper").isMine)
        // Mine first, the one that needs me at the top.
        assertEquals("Ops", home.helpers.first().name)
        assertEquals("Zoe's helper", home.helpers.last().name)

        assertEquals(1, home.needsYou)
        assertEquals(Place.TODO_DESK, home.nextStop)
        assertEquals(1, home.badges[Place.TODO_DESK])
        assertEquals("Good morning, Yannick", home.greeting)
        val net = home.network!!
        assertEquals("Yannick's mesh", net.name)
        assertFalse(net.isOpen)
        assertEquals(2, net.spaces)
        assertEquals(1, net.teams)
        assertEquals(1, net.openSpaces)
        assertTrue(home.conversations.any { it.kind == ConversationKind.TEAM })
        assertEquals(1, home.activeTasks.size)
    }

    @Test
    fun nextStopWithoutRequests() {
        val unread = listOf(Message(id = "x", topic = "_agent.yannick.coder.outbox", from = "agent://yannick/coder", timestamp = at(1), body = "hi"))
        assertEquals(Place.HELPERS, HomeModelBuilder.build(inputs(history = unread)).nextStop)
        val noHelpers = inputs().copy(agents = emptyList())
        assertEquals(Place.SHOPS, HomeModelBuilder.build(noHelpers).nextStop)
    }

    @Test
    fun greetings() {
        assertEquals("Good evening", HomeModelBuilder.greeting(22, null))
        assertEquals("Good afternoon, Nadia", HomeModelBuilder.greeting(14, "nadia"))
    }

    @Test
    fun decisionPreviewInPlainWords() {
        val d = Decision("d", "Send report", "Send it?", "agent://y/writer", "human://y", "requested", listOf(
            DecisionOption("send", "Send the report", pros = listOf("Your client gets it today"), cons = listOf("It cannot be unsent"), outcome = DecisionOutcome.APPROVE),
            DecisionOption("wait", "Not now", outcome = DecisionOutcome.REJECT),
        ))
        val p = HomeModelBuilder.preview(d, "Writer")
        assertEquals("Writer goes ahead with “Send the report”. Your client gets it today.", p.ifYes)
        assertTrue(p.ifNotNow.contains("waits"))
        assertEquals("Keep in mind: It cannot be unsent.", p.undo)
        val plain = HomeModelBuilder.preview(d.copy(options = listOf(DecisionOption("ok", "OK"))), "Ops")
        assertEquals("You can ask Ops to undo it in the chat.", plain.undo)
    }

    @Test
    fun presenceRules() {
        assertEquals(Presence.OFFLINE, PresenceRules.derive(now, null, false, null))
        assertEquals(Presence.ONLINE, PresenceRules.derive(now, now - 1000, false, null))
        assertEquals(Presence.IDLE, PresenceRules.derive(now, now - 10 * 60_000, false, null))
        assertEquals(Presence.BUSY, PresenceRules.derive(now, null, true, null))
        assertEquals(Presence.OFFLINE, PresenceRules.derive(now, now - 1000, true, Presence.OFFLINE to now))
    }

    @Test
    fun shopsFloors() {
        val floors = Shops.floors(agents, me)
        val names = floors.map { it.floor }
        assertEquals(Floor.POPULAR, names.first())
        assertEquals(Floor.EVERYTHING, names.last())
        val study = floors.first { it.floor == Floor.STUDY }.shops.map { it.name }
        assertEquals(listOf("Tutor"), study)
        val writing = floors.first { it.floor == Floor.WRITING }.shops.map { it.name }
        assertTrue("Writer" in writing)
        val money = floors.first { it.floor == Floor.MONEY }.shops.map { it.name }
        assertEquals(listOf("Zoe's helper"), money)
        assertTrue(floors.first { it.floor == Floor.WORK }.shops.any { it.name == "Coder" })
        assertEquals(6, floors.first { it.floor == Floor.EVERYTHING }.shops.size)
        assertEquals("Coder", floors.first { it.floor == Floor.POPULAR }.shops.first().name)

        val plain = Shops.floors(listOf(AgentRegistration("agent://a/x", "X", "human://a")), null)
        assertEquals(listOf(Floor.POPULAR, Floor.EVERYTHING), plain.map { it.floor })
        assertEquals(listOf("What can you do for me?"), plain.last().shops.single().trySaying)
        assertEquals("_agent.a.x", plain.last().shops.single().conversationId)
        assertNull(Conversations.agentConversationId("human://a"))
    }

    @Test
    fun identityMatchesAppCore() {
        assertEquals(0xcbf29ce484222325UL, Identity.stableHash(""))
        assertEquals(Identity.stableHash("agent://yannick/coder"), Identity.stableHash("AGENT://Yannick/Coder"))
        val seen = (0 until 64).map { Identity.accentOf("agent://u/a$it") }.toSet()
        assertEquals(5, seen.size)
        assertEquals("coder", Identity.shortName("agent://yannick/coder"))
        assertEquals("AM", Identity.initials("alice-martin"))
        assertEquals("ZO", Identity.initials("zoe"))
        assertEquals(PrincipalKind.INTERFACE, PrincipalKind.of("telegram://1"))
    }

    @Test
    fun sanitizer() {
        assertEquals("hello\n\tworld", Sanitize.text("hello\n\tworld"))
        assertEquals("red", Sanitize.text("\u001b[31mred\u001b[0m"))
        assertEquals("link", Sanitize.text("\u001b]8;;http://x\u001b\\link\u001b]8;;\u001b\\"))
        assertEquals("ab", Sanitize.text("a‮b"))
        assertEquals("a b", Sanitize.line("a\nb"))
    }
}
