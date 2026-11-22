package io.mellowmesh.core

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import org.junit.jupiter.api.Assumptions.assumeTrue
import org.junit.jupiter.api.Test
import java.io.File

/**
 * Persona run of Journeys and of the per-network doors, against real hubs
 * whose Mellows work on their own (driven by a script). Nadia follows her
 * *Move house* journey across her hub and the movers' network, answers the
 * question waiting on a step, and turns what repeats into a routine; Aïcha
 * administers the right network among three. Skipped unless
 * `MM_SIM_JOURNEYS` names the personas file. Writes what the phone shows
 * and how long it took, step by step.
 */
class JourneysSimulation {
    private class Machine(val id: String, val label: String, val url: String, val client: MeshClient, val me: String)

    private fun machines(arr: JsonArray) = arr.map { el ->
        val o = el.jsonObject
        val url = o["url"]!!.jsonPrimitive.content
        val me = o["principal"]!!.jsonPrimitive.content
        Machine(o["id"]!!.jsonPrimitive.content, o["label"]!!.jsonPrimitive.content, url, MeshClient(MeshConfig(url, o["token"]!!.jsonPrimitive.content, me)), me)
    }

    /** What the phone reads on every refresh: the tasks and pending decisions of every machine. */
    private fun read(ms: List<Machine>): Triple<List<PlacedTask>, Set<String>, Map<String, String>> {
        val tasks = ArrayList<PlacedTask>()
        val pending = HashSet<String>()
        val names = HashMap<String, String>()
        for (m in ms) {
            val net = runCatching { m.client.network() }.getOrNull()
            val key = ChatSpace.segmentKey(net?.network?.name, m.label)
            runCatching { m.client.tasks() }.getOrDefault(emptyList()).forEach { tasks += PlacedTask(it, m.id, key) }
            runCatching { m.client.decisions() }.getOrDefault(emptyList()).filter { it.isPending }.forEach { pending += it.id }
            runCatching { m.client.agents() }.getOrDefault(emptyList()).forEach { names[it.id.lowercase()] = it.name }
        }
        return Triple(tasks, pending, names)
    }

    private fun journey(ms: List<Machine>): Journey? {
        val (t, p, n) = read(ms)
        return Journeys.build(t, p, n).firstOrNull { it.slug == "move-house" }
    }

    private fun waitFor(timeoutMs: Long, check: () -> Boolean): Long? {
        val t0 = System.currentTimeMillis()
        while (System.currentTimeMillis() - t0 < timeoutMs) {
            if (check()) return System.currentTimeMillis() - t0
            Thread.sleep(250)
        }
        return null
    }

    @Test
    fun followTheJourneyAndOpenTheRightDoor() {
        val path = System.getenv("MM_SIM_JOURNEYS")
        assumeTrue(path != null, "persona run only")
        val root = MeshJson.parseToJsonElement(File(path!!).readText()).jsonObject
        val steps = mutableListOf<Pair<String, String>>()

        // ---- T30 Nadia: where is the move, what comes next, who helps -----------
        val nadia = machines(root["nadia"]!!.jsonArray)
        val t0 = System.currentTimeMillis()
        var j = journey(nadia)!!
        steps += "t30_read_ms" to (System.currentTimeMillis() - t0).toString()
        steps += "t30_stations" to j.stations.joinToString(" > ") { "${it.title} [${it.state.name.lowercase()}${it.whoName?.let { w -> ", $w" } ?: ""}]" }
        steps += "t30_line" to j.line
        steps += "t30_next" to (j.next?.let { "${it.title} · ${it.whoName}" } ?: "-")
        steps += "t30_networks" to j.segmentKeys.sorted().joinToString(",")
        steps += "t30_repeat" to (j.repeat?.let { "${it.steps.joinToString(" > ")} x${it.times}" } ?: "-")
        steps += "t30_needs_you" to j.needsYou.toString()

        // She answers the question waiting on the step (from To do).
        val asking = j.stations.first { it.state == StationState.NEEDS_YOU }
        val home = nadia.first { it.id == asking.machineId }
        val decision = home.client.decisions().first { it.id == asking.decisions.first() }
        home.client.respondDecision(decision.id, decision.options.first().id)
        val doneBefore = j.done
        val chimeMs = waitFor(20_000) { journey(nadia)!!.done > doneBefore }
        steps += "step_done_after_answer_ms" to (chimeMs?.toString() ?: "timeout")
        j = journey(nadia)!!
        steps += "after_answer_line" to j.line

        // The movers' Mellow picks up its step on the other network, then the path arrives.
        val arrivedMs = waitFor(25_000) { journey(nadia)!!.finished }
        j = journey(nadia)!!
        steps += "arrived_ms" to (arrivedMs?.toString() ?: "timeout")
        steps += "arrived_line" to j.line

        // What repeats becomes a routine; one more round runs on its own.
        val routine = Journeys.routineOf(j, "r_move")!!
        steps += "routine" to routine.steps.joinToString(" > ") { "${it.title} (${it.who?.let(Identity::shortName)})" }
        var n = 0
        val round = Journeys.nextRound(routine) { "task_sim_round_${++n}" }
        val scoutHome = nadia.first { m -> runCatching { m.client.agents() }.getOrDefault(emptyList()).any { it.id == routine.steps.first().who } }
        val r0 = System.currentTimeMillis()
        round.forEach { scoutHome.client.createTask(it) }
        steps += "routine_created_ms" to (System.currentTimeMillis() - r0).toString()
        val roundMs = waitFor(30_000) { (journey(nadia)!!.repeat?.times ?: 0) >= 4 }
        j = journey(nadia)!!
        steps += "routine_round_done_ms" to (roundMs?.toString() ?: "timeout")
        steps += "repeat_after_round" to (j.repeat?.let { "${it.steps.joinToString(" > ")} x${it.times}" } ?: "-")
        steps += "stations_after_round" to j.stations.size.toString()

        // ---- T31 Aïcha: invite a colleague's AI to the company, not home -------
        val aicha = machines(root["aicha"]!!.jsonArray)
        val owner = aicha.associate { m -> m.id to (runCatching { m.client.whoami().isOwner }.getOrDefault(false)) }
        val netName = aicha.associate { m -> m.id to (runCatching { m.client.network().network.name }.getOrNull() ?: m.label) }
        val segments = aicha.groupBy { ChatSpace.segmentKey(netName[it.id], it.label) }
        steps += "t31_networks" to segments.values.joinToString(" | ") { ms ->
            val name = netName.getValue(ms.first().id)
            name + ":" + (if (ms.any { owner[it.id] == true }) "you run it" else "member")
        }
        val company = segments.values.first { ms -> netName.getValue(ms.first().id) == "Acme Corp" }
        val via = company.first { owner[it.id] == true }
        steps += "t31_admin_via" to via.label
        val invite = via.client.inviteAi(InviteRequest(name = "Colleague AI"))
        steps += "t31_invite_network" to invite.network
        steps += "t31_invite_link_on_company_hub" to (invite.joinUrl.startsWith(via.url) || invite.joinUrl.contains("/join/")).toString()
        steps += "t31_member_admin_offered" to segments.values.filter { ms -> ms.none { owner[it.id] == true } }
            .joinToString(",") { netName.getValue(it.first().id) + ":no" }

        val report = buildJsonObject {
            put("steps", buildJsonArray {
                steps.forEach { (k, v) -> add(buildJsonObject { put("step", k); put("value", v) }) }
            })
        }
        File(System.getenv("MM_SIM_JOURNEYS_OUT") ?: "$path.out").writeText(report.toString())
    }
}
