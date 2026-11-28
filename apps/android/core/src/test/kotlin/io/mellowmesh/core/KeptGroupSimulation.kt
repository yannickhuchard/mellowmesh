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
 * Persona run of a group kept by a hub, against real hubs (driven by a
 * script). The phone's side is the app's own code, in the order the app
 * runs it (MeshRepository.keepGroup / sayInKept / keptThread):
 *
 * - `keep`: Karim's phone gives "Move team" to his home hub, with a pass for
 *   Movers on his business machine, says one line, and goes to sleep;
 * - `say`: a line to one member, picked;
 * - `read`: later, the phone (or any app) reads the one thread.
 *
 * Skipped unless `MM_SIM_KEPT` names the personas file.
 */
class KeptGroupSimulation {
    private class Machine(val id: String, val label: String, val client: MeshClient, val me: String, val url: String)

    @Test
    fun aGroupThatGoesOnWhileThePhoneSleeps() {
        val path = System.getenv("MM_SIM_KEPT")
        assumeTrue(path != null, "persona run only")
        val step = System.getenv("MM_SIM_KEPT_STEP") ?: "keep"
        val out = File(System.getenv("MM_SIM_KEPT_OUT") ?: "kept-$step.json")
        val root = MeshJson.parseToJsonElement(File(path!!).readText()).jsonObject
        val machines = root["karim"]!!.jsonArray.map { el ->
            val o = el.jsonObject
            val me = o["principal"]!!.jsonPrimitive.content
            val url = o["url"]!!.jsonPrimitive.content
            Machine(o["id"]!!.jsonPrimitive.content, o["label"]!!.jsonPrimitive.content, MeshClient(MeshConfig(url, o["token"]!!.jsonPrimitive.content, me)), me, url)
        }
        val byId = machines.associateBy { it.id }
        val names = HashMap<String, String>()
        val members = machines.flatMap { m ->
            // A machine that sleeps lists nothing (the app keeps its last list).
            runCatching { m.client.agents() }.getOrDefault(emptyList()).filter { "mellow" in it.capabilities }.map { a ->
                names[a.id] = a.name
                GroupMember(a.id, a.name, m.id, Conversations.agentConversationId(a.id)!!, m.me)
            }
        }
        val dir = Directory(names)
        val group = MellowGroup("g_move", "Move team", members.map { it.id }, keptOn = "home")
        val keeper = byId.getValue("home")
        val steps = mutableListOf<Pair<String, String>>()

        when (step) {
            "keep" -> {
                val t0 = System.currentTimeMillis()
                val owned = machines.filter { it.client.whoami().isOwner }.map { it.id }
                val chosen = GroupHub.chooseKeeper(members, owned, "home")
                steps += "keeper" to (chosen ?: "none")
                val held = keeper.client.groups().firstOrNull { it.id.equals(group.id, true) }
                val kept = members.map { m ->
                    val there = byId.getValue(m.machineId)
                    when {
                        m.machineId == "home" -> KeptMember(m, null, null, null)
                        GroupHub.holds(held, m.id) -> KeptMember(m, null, null, there.label)
                        else -> KeptMember(m, there.url, there.client.createPass(m.me, m.conversationId, "Group Move team: kept on ${keeper.label}"), there.label)
                    }
                }
                steps += "passes" to kept.count { it.passToken != null }.toString()
                val hg = keeper.client.keepGroup(GroupHub.keepBody(group, kept))
                steps += "kept_members" to hg.members.joinToString(",") { it.name + (if (it.here) "" else "@" + (it.machine ?: "?")) }
                keeper.client.groupSay(group.id, "Hello team: who books the van for Saturday?", emptyList())
                steps += "keep_and_say_ms" to (System.currentTimeMillis() - t0).toString()
            }
            "say" -> {
                val to = members.filter { it.name == System.getenv("MM_SIM_KEPT_TO") }.map { it.id }
                keeper.client.groupSay(group.id, System.getenv("MM_SIM_KEPT_TEXT") ?: "Hello", to)
                steps += "said_to" to to.joinToString(",")
            }
            else -> {
                val t = keeper.client.groupThread(group.id)
                val items = GroupHub.items(t, members, keeper.me, dir)
                steps += "thread" to items.joinToString(" / ") { i ->
                    when (i) {
                        is GroupSaid -> (i.member?.name ?: "Karim") + ": " + i.message.body.lineSequence().first() +
                            (if (i.toNames.isNotEmpty()) " [to " + i.toNames.joinToString(",") + "]" else "")
                        is GroupHandOver -> "${i.fromName} -> ${i.toName}"
                        is GroupNote -> "(" + i.text + ")"
                    }
                }
                steps += "karim_lines" to items.count { it is GroupSaid && it.member == null }.toString()
                steps += "handovers" to items.count { it is GroupHandOver }.toString()
                steps += "thinking" to GroupHub.thinking(t, members).joinToString(",") { it.name }
                steps += "away" to GroupHub.away(t, members).joinToString(",") { it.member.name + "@" + (it.machine ?: "") }
            }
        }
        out.writeText(
            buildJsonObject {
                put("step", step)
                put("steps", buildJsonArray { steps.forEach { (k, v) -> add(buildJsonObject { put("step", k); put("value", v) }) } })
            }.toString(),
        )
    }
}
