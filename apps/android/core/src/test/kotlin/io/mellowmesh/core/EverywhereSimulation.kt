package io.mellowmesh.core

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
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
 * Persona run of the Everywhere view against real hubs (driven by a script
 * that starts them): reads each machine with the app's own client, the way
 * the app's repository does, builds the view and writes what a person would
 * see. Skipped unless `MM_SIM` names the machines file.
 */
class EverywhereSimulation {
    @Test
    fun everyMachineAtOnce() {
        val path = System.getenv("MM_SIM")
        assumeTrue(path != null, "persona run only")
        val machines = MeshJson.parseToJsonElement(File(path!!).readText()).jsonArray
        val homes = machines.map { el ->
            val m = el.jsonObject
            val id = m["id"]!!.jsonPrimitive.content
            val url = m["url"]!!.jsonPrimitive.content
            val token = m["token"]?.jsonPrimitive?.content
            val me = m["principal"]?.jsonPrimitive?.content ?: "human://me"
            val label = Machines.displayLabel(m["label"]?.jsonPrimitive?.content ?: "", url)
            val c = MeshClient(MeshConfig(url, token, me))
            val result = runCatching {
                val agents = c.agents()
                val net = runCatching { c.network() }.getOrNull()
                net to HomeModelBuilder.build(
                    HomeInputs(
                        me = me,
                        agents = agents,
                        tasks = runCatching { c.tasks() }.getOrDefault(emptyList()),
                        decisions = runCatching { c.decisions() }.getOrDefault(emptyList()),
                        history = runCatching { c.history(null, 200) }.getOrDefault(emptyList()),
                        network = net,
                        nowMs = System.currentTimeMillis(),
                    ),
                )
            }
            MachineHome(
                machineId = id,
                label = label,
                networkName = result.getOrNull()?.first?.network?.name,
                health = result.fold({ MachineHealth.ANSWERS }, { MachineHealth.ofFailure(it) ?: MachineHealth.UNKNOWN }),
                home = result.getOrNull()?.second,
                cachedAgents = (m["cached"] as? JsonArray).orEmpty().map { a ->
                    val o = a.jsonObject
                    AgentRegistration(id = o["id"]!!.jsonPrimitive.content, name = o["name"]?.jsonPrimitive?.content ?: "", owner = me)
                },
                me = me,
            )
        }
        val model = Everywhere.build(homes)
        val report: JsonObject = buildJsonObject {
            put("needs_you", model.needsYou)
            put("show_networks", model.showNetworks)
            put("needs_you_on", buildJsonArray { model.needsYouOn.forEach { add(JsonPrimitive(it.label)) } })
            put("machines", buildJsonArray {
                model.machines.forEach { s ->
                    add(buildJsonObject {
                        put("id", s.machineId)
                        put("label", s.label)
                        put("health", s.health.name)
                        put("mellows", s.mellows)
                        put("needs_you", s.needsYou)
                        s.sameAs?.let { put("same_as", it) }
                    })
                }
            })
            put("helpers", buildJsonArray {
                model.helpers.forEach { h ->
                    add(buildJsonObject {
                        put("name", h.card.name)
                        put("machine", h.machineId)
                        put("status", h.card.statusLine)
                        put("mine", h.card.isMine)
                        put("tag", h.tag(model.showNetworks))
                    })
                }
            })
        }
        File(System.getenv("MM_SIM_OUT") ?: "$path.out").writeText(report.toString())
    }
}
