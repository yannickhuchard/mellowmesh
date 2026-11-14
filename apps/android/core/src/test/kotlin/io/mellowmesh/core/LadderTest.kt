package io.mellowmesh.core

import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class LadderTest {
    private val noon = 12 * 60
    private val night = 23 * 60
    private val early = 7 * 60 + 30
    private val defaults = LadderSettings()

    @Test
    fun quietHoursWrapMidnight() {
        assertTrue(Ladder.isQuiet(defaults, 21 * 60))
        assertTrue(Ladder.isQuiet(defaults, night))
        assertTrue(Ladder.isQuiet(defaults, early))
        assertFalse(Ladder.isQuiet(defaults, 8 * 60))
        assertFalse(Ladder.isQuiet(defaults, noon))
        val day = defaults.copy(quietStartMinute = 13 * 60, quietEndMinute = 14 * 60)
        assertTrue(Ladder.isQuiet(day, 13 * 60 + 30))
        assertFalse(Ladder.isQuiet(day, 14 * 60))
        assertFalse(Ladder.isQuiet(defaults.copy(quietHoursEnabled = false), night))
    }

    @Test
    fun knockNotifiesInDaytime() {
        val o = Ladder.decide(Level.KNOCK, defaults, noon, 0)
        assertEquals(Level.KNOCK, o.level)
        assertTrue(o.notify)
        assertFalse(o.toDigest)
    }

    @Test
    fun quietHoursLetOnlyAlarmsThrough() {
        val knock = Ladder.decide(Level.KNOCK, defaults, night, 0)
        assertEquals(Level.GLANCE, knock.level)
        assertFalse(knock.notify)
        assertTrue(knock.toDigest)
        assertEquals("quiet hours", knock.reason)

        // Alarm needs the user's explicit OK (max level Alarm) …
        val capped = Ladder.decide(Level.ALARM, defaults, night, 0)
        assertEquals(Level.GLANCE, capped.level)
        // … and then rings even at night.
        val alarm = Ladder.decide(Level.ALARM, defaults.copy(maxLevel = Level.ALARM), night, 0)
        assertEquals(Level.ALARM, alarm.level)
        assertTrue(alarm.notify)
    }

    @Test
    fun maxLevelCapsAndDigestCollectsBelowKnock() {
        val s = defaults.copy(maxLevel = Level.WHISPER)
        val o = Ladder.decide(Level.KNOCK, s, noon, 0)
        assertEquals(Level.WHISPER, o.level)
        assertFalse(o.notify)
        assertTrue(o.toDigest)
        val w = Ladder.decide(Level.WHISPER, defaults, noon, 0)
        assertEquals(Level.WHISPER, w.level)
        assertTrue(w.toDigest)
        assertFalse(Ladder.decide(Level.WHISPER, defaults.copy(digestEnabled = false), noon, 0).toDigest)
    }

    @Test
    fun snoozeSilencesUntilLater() {
        val s = defaults.copy(snoozedUntilMs = 10_000)
        assertEquals(Level.GLANCE, Ladder.decide(Level.KNOCK, s, noon, 5_000).level)
        assertEquals(Level.KNOCK, Ladder.decide(Level.KNOCK, s, noon, 10_000).level)
    }

    @Test
    fun digestDueOncePerDayAfterItsTime() {
        assertFalse(Ladder.digestDue(defaults, 100, 17 * 60, null, 3))
        assertTrue(Ladder.digestDue(defaults, 100, 18 * 60, null, 3))
        assertTrue(Ladder.digestDue(defaults, 100, 19 * 60, 99, 3))
        assertFalse(Ladder.digestDue(defaults, 100, 19 * 60, 100, 3))
        assertFalse(Ladder.digestDue(defaults, 100, 19 * 60, 99, 0))
    }

    @Test
    fun classifiesLiveMessages() {
        val me = "human://me"
        val dir = Directory(mapOf("agent://me/ops" to "Ops"))
        val decision = Message(
            id = "e1", topic = "_decision.d42.requested", from = "agent://me/ops",
            payload = buildJsonObject { put("id", "d42"); put("title", "Deploy now?"); put("created_by", "agent://me/ops") },
        )
        val i = Ladder.classify(decision, me, dir)!!
        assertEquals(Level.KNOCK, i.requested)
        assertEquals("Ops needs your OK", i.title)
        assertEquals("Deploy now?", i.body)
        assertEquals("d42", i.decisionId)
        assertNull(Ladder.classify(decision.copy(topic = "_decision.d42.responded"), me, dir))

        val reply = Message(id = "m1", topic = "_agent.me.ops.outbox", from = "agent://me/ops", body = "Done!")
        assertEquals(Level.WHISPER, Ladder.classify(reply, me, dir)!!.requested)
        assertEquals("_agent.me.ops", Ladder.classify(reply, me, dir)!!.conversationId)

        val mine = reply.copy(from = me)
        assertNull(Ladder.classify(mine, me, dir))

        val group = Message(id = "m2", topic = "_project.x", from = "agent://me/ops", body = "fyi")
        assertEquals(Level.GLANCE, Ladder.classify(group, me, dir)!!.requested)
        val mention = group.copy(headers = mapOf("x-mentions" to "human://me"))
        assertEquals(Level.TAP, Ladder.classify(mention, me, dir)!!.requested)
        val shouting = group.copy(headers = mapOf("x-mellowmesh-urgency" to "alarm"))
        assertEquals(Level.KNOCK, Ladder.classify(shouting, me, dir)!!.requested)

        val done = Message(id = "t", topic = "_task.t1.completed", from = "agent://me/ops", payload = buildJsonObject { put("id", "t1"); put("title", "Report") })
        assertEquals("Finished: Report", Ladder.classify(done, me, dir)!!.title)
        assertNull(Ladder.classify(Message(topic = "_agent.me.ops.heartbeat", from = "agent://me/ops"), me, dir))
    }
}
