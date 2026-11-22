package io.mellowmesh.core

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class EverywhereTest {
    private val me = "human://lea"
    private val now = 1_800_000_000_000L

    private fun agent(slug: String, owner: String = me) = AgentRegistration(id = "agent://lea/$slug", name = slug.replaceFirstChar { it.uppercase() }, owner = owner)

    private fun home(agents: List<AgentRegistration>, decisions: List<Decision> = emptyList()) = HomeModelBuilder.build(
        HomeInputs(me = me, agents = agents, tasks = emptyList(), decisions = decisions, history = emptyList(), nowMs = now),
    )

    private fun ask(id: String, from: String) = Decision(id = id, title = "Deploy?", createdBy = from, requiredDecider = me)

    @Test
    fun everyMellowOfEveryMachineIsListedOnce() {
        val coder = agent("coder")
        val ops = agent("ops")
        val finance = agent("finance")
        val model = Everywhere.build(
            listOf(
                MachineHome("d", "Desktop", "Léa's mesh", MachineHealth.ANSWERS, home(listOf(coder, ops), listOf(ask("q1", ops.id))), emptyList(), me),
                // The same hub through a tunnel: Coder must not appear twice.
                MachineHome("t", "Tunnel abc", "Léa's mesh", MachineHealth.ANSWERS, home(listOf(coder), listOf(ask("q1", ops.id))), emptyList(), me),
                MachineHome("s", "Server", "Léa's mesh", MachineHealth.RESTING, null, listOf(finance), me),
            ),
        )
        assertEquals(listOf("Ops", "Coder", "Finance"), model.helpers.map { it.card.name })
        assertEquals("d", model.helpers.first { it.card.name == "Coder" }.machineId)
        // A Mellow of a resting machine stays listed, asleep, tagged with it.
        val f = model.helpers.last()
        assertEquals(Mood.ASLEEP, f.card.mood)
        assertEquals("Server", f.tag(withNetwork = false))
        // The same request seen through two addresses is counted once.
        assertEquals(1, model.needsYou)
        assertEquals(listOf("Desktop"), model.needsYouOn.map { it.label })
        assertFalse(model.showNetworks)
        // The tunnel is the Desktop reached another way, and says so.
        assertEquals("Desktop", model.machines.first { it.machineId == "t" }.sameAs)
        assertNull(model.machines.first { it.machineId == "d" }.sameAs)
    }

    @Test
    fun aMachineInTroubleKeepsItsLineAndHidesNobody() {
        val model = Everywhere.build(
            listOf(
                MachineHome("t", "Tunnel qyt9n7", null, MachineHealth.ADDRESS_LOST, null, emptyList(), me),
                MachineHome("l", "Desktop", null, MachineHealth.ANSWERS, home(listOf(agent("a"), agent("b"), agent("c"))), emptyList(), me),
            ),
        )
        assertEquals(3, model.helpers.size)
        assertEquals(listOf(0, 3), model.machines.map { it.mellows })
        assertTrue(model.machines.first().health.needsFix)
        assertEquals(3, model.only("l").size)
        assertTrue(model.only("t").isEmpty())
        assertEquals(3, model.only(null).size)
    }

    @Test
    fun yoursComeFirstThenWhatNeedsYou() {
        val mine = agent("writer")
        val theirs = agent("reviewer", owner = "human://sofia")
        val waiting = agent("ops")
        val model = Everywhere.build(
            listOf(
                MachineHome("a", "Laptop", null, MachineHealth.ANSWERS, home(listOf(theirs, mine, waiting), listOf(ask("q", waiting.id))), emptyList(), me),
            ),
        )
        assertEquals(listOf("Ops", "Writer", "Reviewer"), model.helpers.map { it.card.name })
        assertFalse(model.helpers.last().card.isMine)
    }

    @Test
    fun severalNetworksNameTheNetworkInTheTag() {
        val model = Everywhere.build(
            listOf(
                MachineHome("a", "Desktop", "Léa's mesh", MachineHealth.ANSWERS, home(listOf(agent("coder"))), emptyList(), me),
                MachineHome("b", "Team hub", "Platform team", MachineHealth.ANSWERS, home(listOf(agent("reviewer"))), emptyList(), me),
            ),
        )
        assertTrue(model.showNetworks)
        assertEquals("Platform team · Team hub", model.helpers.first { it.machineId == "b" }.tag(withNetwork = true))
    }

    @Test
    fun aFailureSaysHowTheMachineIs() {
        fun of(kind: MeshException.Kind) = MachineHealth.ofFailure(MeshException(kind, 0, ""))
        assertEquals(MachineHealth.RESTING, of(MeshException.Kind.NETWORK))
        assertEquals(MachineHealth.RESTING, of(MeshException.Kind.SERVER))
        assertEquals(MachineHealth.ADDRESS_LOST, of(MeshException.Kind.NOT_FOUND))
        assertEquals(MachineHealth.ADDRESS_LOST, of(MeshException.Kind.PROTOCOL))
        assertEquals(MachineHealth.KEY_REFUSED, of(MeshException.Kind.UNAUTHORIZED))
        assertNull(of(MeshException.Kind.CONFLICT))
        assertNull(MachineHealth.ofFailure(IllegalStateException()))
    }

    @Test
    fun machinesHaveNamesPeopleRecognize() {
        assertEquals("192.168.1.20", Machines.displayLabel("node://localhost", "http://192.168.1.20:40000"))
        assertEquals("Desktop-lea", Machines.displayLabel("node://desktop-lea", "http://192.168.1.20:40000"))
        assertEquals("Laptop", Machines.displayLabel("Laptop", "http://x"))
        assertEquals("Tunnel qyt9n7w9xgsy", Machines.displayLabel("qyt9n7w9xgsy.hostc.app", "https://qyt9n7w9xgsy.hostc.app"))
        assertEquals("Tunnel qyt9n7w9xgsy", Machines.displayLabel("qyt9n7w9xgsy.hostc.app", "http://127.0.0.1:9"))
        assertEquals("Tunnel qyt9n7w9xgsy", Machines.displayLabel("", "https://qyt9n7w9xgsy.hostc.app/"))
        assertEquals("Tunnel abc-def", Machines.labelFromUrl("https://abc-def.trycloudflare.com"))
    }
}
