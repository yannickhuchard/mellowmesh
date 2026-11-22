package io.mellowmesh.core

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class MachinesTest {
    private fun m(id: String, url: String, token: String = "mm_$id") =
        SavedMachine(id = id, baseUrl = url, token = token, label = id, addedAtMs = 1)

    @Test
    fun pairingAnotherMachineAddsItAndKeepsTheFirst() {
        val desktop = m("desk", "http://192.168.1.20:40000")
        val laptop = m("lap", "https://words.trycloudflare.com")
        val list = Machines.upsert(listOf(desktop), laptop)
        assertEquals(listOf("desk", "lap"), list.map { it.id })
    }

    @Test
    fun pairingTheSameMachineAgainRefreshesItsKeyInPlace() {
        val desktop = m("desk", "http://192.168.1.20:40000/")
        val laptop = m("lap", "https://words.trycloudflare.com")
        val again = m("new", "http://192.168.1.20:40000", token = "mm_fresh").copy(addedAtMs = 9)
        val list = Machines.upsert(listOf(desktop, laptop), again)
        assertEquals(2, list.size)
        assertEquals("desk", list[0].id)
        assertEquals("mm_fresh", list[0].token)
        assertEquals(1, list[0].addedAtMs)
    }

    @Test
    fun labelsAreShortAndHuman() {
        assertEquals("192.168.1.20", Machines.labelFromUrl("http://192.168.1.20:40000"))
        assertTrue(Machines.labelFromUrl("https://occasion-beds-lung-defining.trycloudflare.com").startsWith("Tunnel "))
    }

    @Test
    fun oldOutboxEntriesStillDecode() {
        val old = """[{"id":"p1","conversationId":"c","text":"hi","createdAtMs":5}]"""
        val list = MeshJson.decodeFromString(
            kotlinx.serialization.builtins.ListSerializer(PendingMessage.serializer()),
            old,
        )
        assertEquals(null, list.single().machineId)
    }
}
