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
 * Persona run of a group conversation with Mellows on two networks, against
 * real hubs whose Mellows are hosted by a session (driven by a script):
 * Karim writes once to "Move team" (Reviewer at home, Movers on his
 * business network), both answer, Reviewer hands the van over to Movers,
 * then Karim writes to Reviewer alone. The phone's side is the app's own
 * code (GroupChat, MeshClient). Skipped unless `MM_SIM_GROUPS` names the
 * personas file; writes what the phone shows and how long it took.
 */
class GroupSimulation {
    private class Machine(val id: String, val client: MeshClient, val me: String)

    private fun waitFor(timeoutMs: Long, check: () -> Boolean): Long? {
        val t0 = System.currentTimeMillis()
        while (System.currentTimeMillis() - t0 < timeoutMs) {
            if (check()) return System.currentTimeMillis() - t0
            Thread.sleep(200)
        }
        return null
    }

    @Test
    fun talkToTwoNetworksAtOnce() {
        val path = System.getenv("MM_SIM_GROUPS")
        assumeTrue(path != null, "persona run only")
        val root = MeshJson.parseToJsonElement(File(path!!).readText()).jsonObject
        val steps = mutableListOf<Pair<String, String>>()
        val machines = root["karim"]!!.jsonArray.map { el ->
            val o = el.jsonObject
            val me = o["principal"]!!.jsonPrimitive.content
            Machine(o["id"]!!.jsonPrimitive.content, MeshClient(MeshConfig(o["url"]!!.jsonPrimitive.content, o["token"]!!.jsonPrimitive.content, me)), me)
        }
        val byMachine = machines.associateBy { it.id }

        // The phone knows every Mellow of every machine (Everywhere).
        val names = HashMap<String, String>()
        val members = machines.flatMap { m ->
            m.client.agents().filter { "mellow" in it.capabilities }.map { a ->
                names[a.id] = a.name
                GroupMember(a.id, a.name, m.id, Conversations.agentConversationId(a.id)!!, m.me)
            }
        }
        val dir = Directory(names)
        val group = MellowGroup("g_move", "Move team", members.map { it.id })
        steps += "members" to members.joinToString(" | ") { "${it.name}@${it.machineId}" }

        fun read(): List<GroupItem> = GroupChat.thread(
            group, members,
            members.flatMap { m ->
                val conv = Conversations.parseId(m.conversationId)!!
                byMachine.getValue(m.machineId).client.history(Conversations.historyPattern(conv), 200).map { MemberMessage(m, it) }
            },
            dir,
        )

        fun say(to: List<GroupMember>, text: String, items: List<GroupItem>, handoverOf: String? = null, from: GroupMember? = null) {
            val turn = "t_" + java.util.UUID.randomUUID().toString().replace("-", "").take(20)
            val payload = GroupChat.payload(group, members, turn, to, GroupChat.said(items), handoverOf, from)
            for (m in to) byMachine.getValue(m.machineId).client.sendWithPayload(m.conversationId, text, payload)
        }

        fun line(i: GroupItem) = when (i) {
            is GroupSaid -> (i.member?.name ?: "Karim") + (if (i.toNames.isNotEmpty()) " (to ${i.toNames.joinToString()})" else "") + ": " + i.message.body
            is GroupHandOver -> "${i.fromName} → ${i.toName}"
        }

        val movers = members.first { it.name == "Movers" }
        val reviewer = members.first { it.name == "Reviewer" }

        // A one-to-one line with Movers first: it must stay out of the group.
        byMachine.getValue(movers.machineId).client.sendText(movers.conversationId, "Hi Movers, just us: are you free in March?")
        waitFor(15_000) {
            val c = byMachine.getValue(movers.machineId).client
            c.history(Conversations.historyPattern(Conversations.parseId(movers.conversationId)!!), 50).any { it.from == movers.id }
        }

        // ---- T32 one line, two networks, two answers --------------------------------
        val t0 = System.currentTimeMillis()
        val to = GroupChat.addressees("Hello team, who books the van for Saturday?", members)
        steps += "t32_addressees" to to.joinToString(",") { it.name }
        say(to, "Hello team, who books the van for Saturday?", emptyList())
        steps += "t32_sent_ms" to (System.currentTimeMillis() - t0).toString()
        val waitingNow = GroupChat.waitingOn(read(), members, System.currentTimeMillis())
        steps += "t32_thinking_right_after" to waitingNow.joinToString(",") { it.name }
        val both = waitFor(30_000) { read().count { it is GroupSaid && it.member != null } >= 2 }
        steps += "t32_both_answered_ms" to (both?.toString() ?: "timeout")
        var items = read()
        steps += "t32_thread" to items.joinToString(" / ") { line(it) }
        steps += "t32_karim_lines" to items.count { it is GroupSaid && it.member == null }.toString()

        // ---- Hand-over: Reviewer named @Movers ---------------------------------------
        val jobs = GroupChat.handOvers(items, members)
        steps += "handover_jobs" to jobs.joinToString(",") { "${it.answer.member?.name}->${it.to.name}" }
        val t1 = System.currentTimeMillis()
        for (j in jobs) say(listOf(j.to), GroupChat.handOverText(j.answer), items, j.answer.message.id, j.answer.member)
        val handed = waitFor(30_000) {
            val now = read()
            now.any { it is GroupHandOver } && now.count { it is GroupSaid && it.member?.name == "Movers" } >= 2
        }
        steps += "handover_answered_ms" to (handed?.toString() ?: "timeout")
        items = read()
        steps += "handover_again" to GroupChat.handOvers(items, members).size.toString()
        steps += "after_handover_thread" to items.joinToString(" / ") { line(it) }
        steps += "t32_handover_total_ms" to (System.currentTimeMillis() - t1).toString()

        // ---- T33 a line to one member only --------------------------------------------
        val directed = "@Reviewer can you check the contract before Friday?"
        val only = GroupChat.addressees(directed, members)
        steps += "t33_addressees" to only.joinToString(",") { it.name }
        val moversBefore = items.count { it is GroupSaid && it.member?.name == "Movers" }
        say(only, directed, items)
        val alone = waitFor(30_000) { read().count { it is GroupSaid && it.member?.name == "Reviewer" } >= 2 }
        steps += "t33_answered_ms" to (alone?.toString() ?: "timeout")
        Thread.sleep(1_500)
        items = read()
        steps += "t33_movers_quiet" to (items.count { it is GroupSaid && it.member?.name == "Movers" } == moversBefore).toString()
        steps += "t33_to_label" to ((items.lastOrNull { it is GroupSaid && it.member == null } as? GroupSaid)?.toNames?.joinToString() ?: "-")

        // ---- The one-to-one conversation stays its own ---------------------------------
        val c = byMachine.getValue(movers.machineId).client
        val oneToOne = Chat.thread(movers.conversationId, c.history(Conversations.historyPattern(Conversations.parseId(movers.conversationId)!!), 200), movers.me, dir)
        steps += "one_to_one_movers" to oneToOne.joinToString(" / ") { (if (it.isMine) "Karim" else it.fromDisplay) + ": " + it.body }
        steps += "final_thread" to items.joinToString(" / ") { line(it) }

        System.getenv("MM_SIM_GROUPS_OUT")?.let { out ->
            File(out).writeText(
                buildJsonObject {
                    put("steps", buildJsonArray { steps.forEach { (k, v) -> add(buildJsonObject { put("step", k); put("value", v) }) } })
                }.toString(),
            )
        }
    }
}
