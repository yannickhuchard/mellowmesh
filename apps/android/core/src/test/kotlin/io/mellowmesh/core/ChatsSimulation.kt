package io.mellowmesh.core

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
 * Persona run of the new Chats against real hubs with Mellows that answer
 * (driven by a script): every Mellow of every network in one space, a
 * group across networks, a segment filter, and talking to two Mellows on
 * two machines without switching anything. Skipped unless `MM_SIM_CHATS`
 * names the machines file. Writes what a person would see, step by step.
 */
class ChatsSimulation {
    private class Machine(val id: String, val label: String, val client: MeshClient, val me: String)

    private fun read(ms: List<Machine>): Pair<EverywhereModel, List<PlacedConversation>> {
        val homes = ms.map { m ->
            val r = runCatching {
                val agents = m.client.agents()
                val net = runCatching { m.client.network() }.getOrNull()
                net to HomeModelBuilder.build(
                    HomeInputs(
                        me = m.me, agents = agents,
                        tasks = runCatching { m.client.tasks() }.getOrDefault(emptyList()),
                        decisions = runCatching { m.client.decisions() }.getOrDefault(emptyList()),
                        history = runCatching { m.client.history(null, 200) }.getOrDefault(emptyList()),
                        network = net, nowMs = System.currentTimeMillis(),
                    ),
                )
            }
            MachineHome(
                m.id, m.label, r.getOrNull()?.first?.network?.name,
                r.fold({ MachineHealth.ANSWERS }, { MachineHealth.ofFailure(it) ?: MachineHealth.UNKNOWN }),
                r.getOrNull()?.second, emptyList(), m.me,
            )
        }
        val convs = homes.flatMap { h ->
            val key = ChatSpace.segmentKey(h.networkName, h.label)
            h.home?.conversations.orEmpty().map { PlacedConversation(it, h.machineId, key) }
        }
        return Everywhere.build(homes) to convs
    }

    @Test
    fun talkToMellowsLikePeople() {
        val path = System.getenv("MM_SIM_CHATS")
        assumeTrue(path != null, "persona run only")
        val ms = MeshJson.parseToJsonElement(File(path!!).readText()).jsonArray.map { el ->
            val o = el.jsonObject
            val url = o["url"]!!.jsonPrimitive.content
            val me = o["principal"]!!.jsonPrimitive.content
            Machine(o["id"]!!.jsonPrimitive.content, o["label"]!!.jsonPrimitive.content, MeshClient(MeshConfig(url, o["token"]!!.jsonPrimitive.content, me)), me)
        }
        val steps = mutableListOf<Pair<String, String>>()

        // T26 Marc / first open: nobody was written to yet, yet everyone is there.
        var (ew, convs) = read(ms)
        var space = ChatSpace.build(ew, convs, emptyList())
        steps += "first_open_everyone" to space.everyone.joinToString(",") { it.card.name }
        steps += "first_open_conversations" to space.conversations.size.toString()
        steps += "segments" to space.segments.joinToString(",") { "${it.name}:${it.mellows}" }

        // T27 Léa: write to Coder (Laptop) then Ops (Desktop), from the same list.
        val placed = space.everyone.associateBy { it.card.name }
        val byId = ms.associateBy { it.id }
        for ((name, text) in listOf("Coder" to "Can we ship the login page tonight?", "Ops" to "Is production healthy?")) {
            val h = placed.getValue(name)
            byId.getValue(h.machineId).client.sendText(h.card.conversationId!!, text)
        }
        // The Mellows answer (the script's fake agents); wait for both.
        val deadline = System.currentTimeMillis() + 15_000
        while (System.currentTimeMillis() < deadline) {
            val (e2, c2) = read(ms)
            ew = e2; convs = c2
            space = ChatSpace.build(ew, convs, emptyList())
            val previews = space.conversations.map { it.summary.title to (it.summary.lastMessagePreview ?: "") }
            if (previews.count { (_, p) -> p.startsWith("Re:") } >= 2) break
            Thread.sleep(400)
        }
        steps += "after_talk_conversations" to space.conversations.joinToString(" | ") {
            "${it.summary.title}@${it.machineId}: ${it.summary.lastMessagePreview}"
        }

        // T28 Karim: a group across two networks.
        var crew = MellowGroup("g1", "Coding crew")
        for (n in listOf("Coder", "Reviewer", "Ops")) crew = ChatSpace.add(crew, placed.getValue(n).card.id)
        space = ChatSpace.build(ew, convs, listOf(crew))
        val g = space.groups.first()
        steps += "group" to "${g.group.name}: " + g.members.joinToString(",") { "${it.card.name}@${it.machineId}" }
        steps += "group_networks" to g.members.mapNotNull { it.networkName }.toSet().sorted().joinToString(",")

        // T29 Sofia: only the team network, then all again.
        val team = space.segments.first { it.name == "Platform team" }
        val only = ChatSpace.build(ew, convs, listOf(crew), selected = team.key)
        steps += "team_only" to only.everyone.joinToString(",") { it.card.name }
        steps += "all_again" to ChatSpace.build(ew, convs, listOf(crew), selected = null).everyone.size.toString()

        val report = buildJsonObject {
            put("steps", buildJsonArray {
                steps.forEach { (k, v) -> add(buildJsonObject { put("step", k); put("value", v) }) }
            })
        }
        File(System.getenv("MM_SIM_CHATS_OUT") ?: "$path.out").writeText(report.toString())
    }
}
