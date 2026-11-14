package io.mellowmesh.core

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNotNull
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertThrows
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class MeshUiTest {
    private val full = """{
      "version": 1,
      "title": "Nightly build",
      "blocks": [
        {"type": "text", "markdown": "**Green** on `main`"},
        {"type": "key_value", "pairs": [{"key": "commit", "value": "7330242"}]},
        {"type": "progress", "label": "Tests", "value": 0.75},
        {"type": "metric", "label": "Coverage", "value": "82.4%", "delta": "+1.2%"},
        {"type": "list", "items": ["lint ok", "unit ok"]},
        {"type": "chart", "kind": "line", "series": [1, 2.5, 3], "labels": ["a", "b", "c"]},
        {"type": "image", "url": "https://example.com/graph.png", "alt": "graph"},
        {"type": "actions", "actions": [
          {"id": "retry", "label": "Retry", "style": "primary"},
          {"id": "drop.cache", "label": "Drop cache", "style": "destructive"},
          {"id": "details", "label": "Details"}
        ]}
      ]
    }"""

    private fun cardWith(block: String) = """{"version":1,"title":"t","blocks":[$block]}"""

    private fun rejects(json: String) {
        assertThrows(MeshUiException::class.java, { MeshUi.parse(json) }, json)
    }

    @Test
    fun parsesEveryBlockType() {
        val card = MeshUi.parse(full)
        assertEquals(1, card.version)
        assertEquals("Nightly build", card.title)
        assertEquals(8, card.blocks.size)
        val chart = card.blocks[5] as UiBlock.Chart
        assertEquals(ChartKind.LINE, chart.kind)
        assertEquals(3, chart.series.size)
        val actions = (card.blocks[7] as UiBlock.Actions).actions
        assertEquals(UiActionStyle.PRIMARY, actions[0].style)
        assertEquals(UiActionStyle.DESTRUCTIVE, actions[1].style)
        assertEquals(UiActionStyle.SECONDARY, actions[2].style)
        assertEquals(0.75, (card.blocks[2] as UiBlock.Progress).value)
        assertEquals("+1.2%", (card.blocks[3] as UiBlock.Metric).delta)
    }

    @Test
    fun rejectsStructuralProblems() {
        rejects("not json")
        rejects("""{"version":2,"title":"t","blocks":[{"type":"text","markdown":"x"}]}""")
        rejects("""{"version":1,"title":"t","blocks":[]}""")
        rejects("""{"version":1,"title":"t","blocks":[{"type":"text","markdown":"x"}],"extra":1}""")
        rejects(cardWith("""{"type":"html","html":"<b>x</b>"}"""))
        rejects(cardWith("""{"type":"text","markdown":"x","onclick":"y"}"""))
        rejects(cardWith("""{"type":"text","markdown":5}"""))
        val many = List(MeshUi.MAX_BLOCKS + 1) { """{"type":"text","markdown":"x"}""" }.joinToString(",")
        rejects("""{"version":1,"title":"t","blocks":[$many]}""")
        val okBlocks = List(MeshUi.MAX_BLOCKS) { """{"type":"text","markdown":"x"}""" }.joinToString(",")
        assertNotNull(MeshUi.parse("""{"version":1,"title":"t","blocks":[$okBlocks]}"""))
        rejects("x".repeat(MeshUi.MAX_CARD_JSON_BYTES + 1))
    }

    @Test
    fun rejectsScriptAndUnsafeUrls() {
        rejects(cardWith("""{"type":"text","markdown":"<script>alert(1)</script>"}"""))
        rejects(cardWith("""{"type":"text","markdown":"[x](javascript:alert(1))"}"""))
        rejects(cardWith("""{"type":"text","markdown":"<img src=x onerror=alert(1)>"}"""))
        rejects(cardWith("""{"type":"list","items":["<iframe src=x>"]}"""))
        rejects(cardWith("""{"type":"image","url":"http://example.com/a.png","alt":"a"}"""))
        rejects(cardWith("""{"type":"image","url":"data:image/png;base64,AAAA","alt":"a"}"""))
        rejects(cardWith("""{"type":"image","url":"https://u:p@example.com/a.png","alt":"a"}"""))
        rejects(cardWith("""{"type":"image","url":"https://example.com/a.png","alt":""}"""))
        rejects(cardWith("""{"type":"text","markdown":"<b>bold</b>"}"""))
        rejects(cardWith("""{"type":"text","markdown":"<!-- hidden -->"}"""))
        assertNotNull(MeshUi.parse(cardWith("""{"type":"text","markdown":"Turn on = enable, see a < b and <https://example.com>"}""")))
    }

    @Test
    fun rejectsOutOfRangeValues() {
        rejects(cardWith("""{"type":"progress","label":"p","value":1.5}"""))
        rejects(cardWith("""{"type":"progress","label":"p","value":-0.1}"""))
        rejects(cardWith("""{"type":"chart","kind":"pie","series":[1]}"""))
        rejects(cardWith("""{"type":"chart","kind":"bar","series":[]}"""))
        rejects(cardWith("""{"type":"chart","kind":"bar","series":[1,2],"labels":["a"]}"""))
        rejects(cardWith("""{"type":"metric","label":"m","value":""}"""))
        rejects(cardWith("""{"type":"text","markdown":"${"x".repeat(4001)}"}"""))
        rejects(cardWith("""{"type":"key_value","pairs":[]}"""))
        rejects(cardWith("""{"type":"list","items":[]}"""))
    }

    @Test
    fun rejectsBadActions() {
        rejects(cardWith("""{"type":"actions","actions":[]}"""))
        rejects(cardWith("""{"type":"actions","actions":[{"id":"a","label":"A"},{"id":"a","label":"B"}]}"""))
        rejects(cardWith("""{"type":"actions","actions":[{"id":"a b","label":"A"}]}"""))
        rejects(cardWith("""{"type":"actions","actions":[{"id":"a","label":"A","style":"blink"}]}"""))
        rejects("""{"version":1,"title":"t","blocks":[
              {"type":"actions","actions":[{"id":"a","label":"A"}]},
              {"type":"actions","actions":[{"id":"b","label":"B"}]}]}""")
    }

    @Test
    fun stripsControlCharacters() {
        val card = MeshUi.parse(cardWith("""{"type":"text","markdown":"ok\u001b[31m red"}"""))
        assertEquals(UiBlock.Text("ok red"), card.blocks[0])
    }

    @Test
    fun invalidCardIsDroppedButMessageKept() {
        val good = Message(
            id = "m1", topic = "_agent.y.coder.outbox", from = "agent://y/coder", body = "done \u001b[2J‮",
            payload = MeshJson.parseToJsonElement("""{"mesh_ui":{"version":1,"title":"Build","blocks":[{"type":"text","markdown":"ok"}]}}"""),
        )
        val c = Chat.toChat(good, "human://y", Directory())
        assertEquals("done ", c.body)
        assertEquals("Build", c.ui!!.title)
        assertEquals("coder", c.fromDisplay)
        assertTrue(c.isHelper)

        val bad = good.copy(payload = MeshJson.parseToJsonElement("""{"mesh_ui":{"version":1,"blocks":[{"type":"text","markdown":"<script>"}]}}"""))
        assertNull(Chat.toChat(bad, "human://y", Directory()).ui)

        val reply = good.copy(payload = MeshJson.parseToJsonElement("""{"ui_action":{"card_message_id":"msg_9","action_id":"retry"}}"""))
        assertEquals(UiActionReply("msg_9", "retry"), Chat.toChat(reply, "human://y", Directory()).uiAction)
    }
}
