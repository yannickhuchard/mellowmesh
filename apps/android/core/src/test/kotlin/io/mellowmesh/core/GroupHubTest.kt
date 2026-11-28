package io.mellowmesh.core

import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class GroupHubTest {
    private val reviewer = GroupMember("agent://karim/reviewer", "Reviewer", "home", "_agent.karim.reviewer", "human://karim")
    private val movers = GroupMember("agent://karim/movers", "Movers", "biz", "_agent.karim.movers", "human://karim")
    private val dir = Directory(mapOf(reviewer.id to "Reviewer", movers.id to "Movers"))

    private val json = """
        {"group":{"id":"g_move","name":"Move team","topic":"_group.g_move","members":[
            {"id":"agent://karim/reviewer","name":"Reviewer","conversation":"_agent.karim.reviewer","here":true,"reachable":true},
            {"id":"agent://karim/movers","name":"Movers","conversation":"_agent.karim.movers","machine":"Karim Moves","here":false,"reachable":false}]},
         "items":[
            {"id":"a","kind":"said","ts":"2026-11-23T07:40:00Z","from":"human://karim","text":"Who books the van?","turn":"t1","to":[]},
            {"id":"b","kind":"answer","ts":"2026-11-23T07:41:00Z","from":"agent://karim/reviewer","text":"Not mine: @Movers","turn":"t1","member":{"id":"agent://karim/reviewer","name":"Reviewer"}},
            {"id":"c","kind":"handover","ts":"2026-11-23T07:41:01Z","from":"agent://karim/reviewer","text":"Reviewer handed this over to Movers.","turn":"t1","to":[{"id":"agent://karim/movers","name":"Movers"}],"member":{"id":"agent://karim/reviewer","name":"Reviewer"},"of":"b"},
            {"id":"d","kind":"delivered","ts":"2026-11-23T08:31:00Z","from":"node","text":"Delivered to Movers when Karim Moves came back.","turn":"t1","member":{"id":"agent://karim/movers","name":"Movers"}},
            {"id":"e","kind":"said","ts":"2026-11-23T08:40:00Z","from":"human://karim","text":"@Reviewer the contract","turn":"t2","to":[{"id":"agent://karim/reviewer","name":"Reviewer"}]}],
         "pending":[
            {"turn":"t1","member":{"id":"agent://karim/movers","name":"Movers"},"machine":"Karim Moves","state":"thinking"},
            {"turn":"t2","member":{"id":"agent://karim/reviewer","name":"Reviewer"},"state":"waiting","machine":null}]}
    """.trimIndent()

    @Test
    fun theHubThreadReadsAsTheScreenDrawsIt() {
        val t = MeshJson.decodeFromString(HubThread.serializer(), json)
        val items = GroupHub.items(t, listOf(reviewer, movers), "human://karim", dir)
        assertEquals(5, items.size)
        val said = items[0] as GroupSaid
        assertTrue(said.message.isMine)
        assertNull(said.member)
        val answer = items[1] as GroupSaid
        assertEquals("Reviewer", answer.member?.name)
        assertEquals("home", answer.member?.machineId)
        val ho = items[2] as GroupHandOver
        assertEquals("Movers", ho.toName)
        assertEquals("b", ho.ofMessage)
        assertTrue((items[3] as GroupNote).late)
        assertEquals(listOf("Reviewer"), (items[4] as GroupSaid).toNames)
        assertEquals(listOf("Movers"), GroupHub.thinking(t, listOf(reviewer, movers)).map { it.name })
        assertEquals(listOf("Reviewer"), GroupHub.away(t, listOf(reviewer, movers)).map { it.member.name })
    }

    @Test
    fun theKeeperIsTheOwnedMachineWithMostMembers() {
        val second = movers.copy(id = "agent://karim/books", name = "Books")
        assertEquals("biz", GroupHub.chooseKeeper(listOf(reviewer, movers, second), listOf("home", "biz"), "home"))
        assertEquals("home", GroupHub.chooseKeeper(listOf(reviewer, movers), listOf("home", "biz"), "home"))
        assertNull(GroupHub.chooseKeeper(listOf(reviewer), emptyList(), "home"))
    }

    @Test
    fun aPassGoesOnlyWithAMemberOnAnotherMachine() {
        val body = GroupHub.keepBody(
            MellowGroup("g_Move", "Move team", listOf(reviewer.id, movers.id)),
            listOf(
                KeptMember(reviewer, null, null, null),
                KeptMember(movers, "https://relay.example/hub/biz", "mm_pass", "Karim Moves"),
            ),
        )
        assertEquals("g_move", body["id"]!!.jsonPrimitive.content)
        val members = body["members"]!!.jsonArray
        assertFalse(members[0].jsonObject.containsKey("pass"))
        assertEquals("https://relay.example/hub/biz", members[1].jsonObject["pass"]!!.jsonObject["url"]!!.jsonPrimitive.content)
        assertEquals("_agent.karim.movers", members[1].jsonObject["conversation"]!!.jsonPrimitive.content)
        assertEquals("_group.g_move", GroupHub.topic("g_Move"))
    }
}
