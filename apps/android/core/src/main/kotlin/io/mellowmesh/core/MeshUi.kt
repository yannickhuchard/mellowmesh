package io.mellowmesh.core

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.intOrNull
import java.net.URI
import java.net.URISyntaxException

/**
 * MeshUI: declarative cards agents attach to messages (`payload.mesh_ui`),
 * rendered natively. Port of app-core `meshui.rs` with the same limits.
 * Data, never code: unknown fields or block types, oversized strings,
 * non-finite numbers, non-https images and HTML/script-like content are rejected,
 * and a message whose card fails validation is shown without the card.
 */
data class MeshUiCard(val version: Int, val title: String, val blocks: List<UiBlock>)

data class KeyValuePair(val key: String, val value: String)

enum class ChartKind { LINE, BAR }

enum class UiActionStyle { PRIMARY, SECONDARY, DESTRUCTIVE }

data class UiAction(val id: String, val label: String, val style: UiActionStyle)

sealed class UiBlock {
    data class Text(val markdown: String) : UiBlock()
    data class KeyValue(val pairs: List<KeyValuePair>) : UiBlock()
    data class Progress(val label: String, val value: Double) : UiBlock()
    data class Metric(val label: String, val value: String, val delta: String?) : UiBlock()
    data class ListBlock(val items: List<String>) : UiBlock()
    data class Chart(val kind: ChartKind, val series: List<Double>, val labels: List<String>) : UiBlock()
    data class Image(val url: String, val alt: String) : UiBlock()
    data class Actions(val actions: List<UiAction>) : UiBlock()
}

class MeshUiException(message: String) : IllegalArgumentException(message)

object MeshUi {
    const val VERSION = 1
    const val MAX_CARD_JSON_BYTES = 64 * 1024
    const val MAX_BLOCKS = 32
    private const val MAX_TITLE = 120
    private const val MAX_MARKDOWN = 4000
    private const val MAX_LABEL = 120
    private const val MAX_KV_PAIRS = 32
    private const val MAX_KEY = 64
    private const val MAX_VALUE = 256
    private const val MAX_METRIC_LABEL = 64
    private const val MAX_METRIC_VALUE = 32
    private const val MAX_LIST_ITEMS = 50
    private const val MAX_LIST_ITEM = 256
    private const val MAX_SERIES = 500
    private const val MAX_CHART_LABEL = 32
    private const val MAX_URL = 2048
    private const val MAX_ALT = 256
    private const val MAX_ACTIONS = 6
    private const val MAX_ACTION_ID = 64
    private const val MAX_ACTION_LABEL = 40

    /** Payload key carrying a card. */
    const val PAYLOAD_KEY = "mesh_ui"
    /** Payload key and content type of an action reply. */
    const val ACTION_KEY = "ui_action"
    const val ACTION_CONTENT_TYPE = "application/vnd.mellowmesh.ui-action+json"

    private val FORBIDDEN = listOf(
        "<script", "</script", "<iframe", "<object", "<embed",
        "javascript:", "vbscript:", "data:text/html", "<svg",
    )

    private fun fail(msg: String): Nothing = throw MeshUiException("MeshUI: $msg")

    /** Parse and strictly validate a card from JSON text. */
    fun parse(json: String): MeshUiCard {
        if (json.toByteArray(Charsets.UTF_8).size > MAX_CARD_JSON_BYTES) fail("card exceeds $MAX_CARD_JSON_BYTES bytes")
        val element = try {
            MeshJson.parseToJsonElement(json)
        } catch (e: Exception) {
            fail("not valid JSON")
        }
        return parse(element)
    }

    /** Parse an already-decoded card, or null when it is invalid. */
    fun parseOrNull(element: JsonElement?): MeshUiCard? = try {
        if (element == null || element is JsonNull) null else parse(element)
    } catch (_: MeshUiException) {
        null
    }

    fun parse(element: JsonElement): MeshUiCard {
        if (element.toString().toByteArray(Charsets.UTF_8).size > MAX_CARD_JSON_BYTES) {
            fail("card exceeds $MAX_CARD_JSON_BYTES bytes")
        }
        val obj = element as? JsonObject ?: fail("a card must be an object")
        allowOnly(obj, "card", setOf("version", "title", "blocks"))
        val version = (obj["version"] as? JsonPrimitive)?.takeIf { !it.isString }?.intOrNull
            ?: fail("missing version")
        if (version != VERSION) fail("unsupported version $version (expected $VERSION)")
        val title = optString(obj, "title") ?: ""
        val blocks = obj["blocks"] as? JsonArray ?: fail("missing blocks")
        if (blocks.isEmpty()) fail("a card needs at least one block")
        if (blocks.size > MAX_BLOCKS) fail("more than $MAX_BLOCKS blocks")
        val checkedTitle = checkText("title", title, MAX_TITLE, allowEmpty = true)
        var actionBlocks = 0
        val out = blocks.map { b ->
            val block = b as? JsonObject ?: fail("a block must be an object")
            val type = (block["type"] as? JsonPrimitive)?.takeIf { it.isString }?.content ?: fail("block without type")
            when (type) {
                "text" -> {
                    allowOnly(block, type, setOf("type", "markdown"))
                    UiBlock.Text(checkText("text.markdown", reqString(block, "markdown"), MAX_MARKDOWN, false))
                }
                "key_value" -> {
                    allowOnly(block, type, setOf("type", "pairs"))
                    val pairs = block["pairs"] as? JsonArray ?: fail("key_value.pairs missing")
                    if (pairs.isEmpty() || pairs.size > MAX_KV_PAIRS) fail("key_value needs 1..=$MAX_KV_PAIRS pairs")
                    UiBlock.KeyValue(pairs.map { p ->
                        val po = p as? JsonObject ?: fail("key_value pair must be an object")
                        allowOnly(po, "key_value pair", setOf("key", "value"))
                        KeyValuePair(
                            checkText("key_value.key", reqString(po, "key"), MAX_KEY, false),
                            checkText("key_value.value", reqString(po, "value"), MAX_VALUE, true),
                        )
                    })
                }
                "progress" -> {
                    allowOnly(block, type, setOf("type", "label", "value"))
                    val v = reqNumber(block, "value")
                    if (!v.isFinite() || v < 0.0 || v > 1.0) fail("progress.value must be within 0..=1")
                    UiBlock.Progress(checkText("progress.label", reqString(block, "label"), MAX_LABEL, true), v)
                }
                "metric" -> {
                    allowOnly(block, type, setOf("type", "label", "value", "delta"))
                    UiBlock.Metric(
                        checkText("metric.label", reqString(block, "label"), MAX_METRIC_LABEL, false),
                        checkText("metric.value", reqString(block, "value"), MAX_METRIC_VALUE, false),
                        optString(block, "delta")?.let { checkText("metric.delta", it, MAX_METRIC_VALUE, true) },
                    )
                }
                "list" -> {
                    allowOnly(block, type, setOf("type", "items"))
                    val items = block["items"] as? JsonArray ?: fail("list.items missing")
                    if (items.isEmpty() || items.size > MAX_LIST_ITEMS) fail("list needs 1..=$MAX_LIST_ITEMS items")
                    UiBlock.ListBlock(items.map { checkText("list.item", stringOf(it, "list.item"), MAX_LIST_ITEM, false) })
                }
                "chart" -> {
                    allowOnly(block, type, setOf("type", "kind", "series", "labels"))
                    val kind = when (val k = reqString(block, "kind")) {
                        "line" -> ChartKind.LINE
                        "bar" -> ChartKind.BAR
                        else -> fail("unknown chart kind '${Sanitize.line(k)}'")
                    }
                    val series = block["series"] as? JsonArray ?: fail("chart.series missing")
                    if (series.isEmpty() || series.size > MAX_SERIES) fail("chart needs 1..=$MAX_SERIES points")
                    val values = series.map { numberOf(it, "chart.series") }
                    if (values.any { !it.isFinite() }) fail("chart values must be finite")
                    val labels = (block["labels"] as? JsonArray) ?: JsonArray(emptyList())
                    if (block["labels"] != null && block["labels"] !is JsonArray) fail("chart.labels must be a list")
                    if (labels.isNotEmpty() && labels.size != values.size) fail("chart labels must match the series length")
                    UiBlock.Chart(kind, values, labels.map { checkText("chart.label", stringOf(it, "chart.label"), MAX_CHART_LABEL, true) })
                }
                "image" -> {
                    allowOnly(block, type, setOf("type", "url", "alt"))
                    UiBlock.Image(
                        checkHttpsUrl(reqString(block, "url")),
                        checkText("image.alt", reqString(block, "alt"), MAX_ALT, false),
                    )
                }
                "actions" -> {
                    allowOnly(block, type, setOf("type", "actions"))
                    actionBlocks++
                    if (actionBlocks > 1) fail("at most one actions block")
                    val actions = block["actions"] as? JsonArray ?: fail("actions.actions missing")
                    if (actions.isEmpty() || actions.size > MAX_ACTIONS) fail("actions needs 1..=$MAX_ACTIONS actions")
                    val ids = HashSet<String>()
                    UiBlock.Actions(actions.map { a ->
                        val ao = a as? JsonObject ?: fail("an action must be an object")
                        allowOnly(ao, "action", setOf("id", "label", "style"))
                        val id = reqString(ao, "id")
                        if (!isValidActionId(id)) fail("invalid action id '${Sanitize.line(id)}'")
                        if (!ids.add(id)) fail("duplicate action id '$id'")
                        val style = when (val s = optString(ao, "style")) {
                            null, "secondary" -> UiActionStyle.SECONDARY
                            "primary" -> UiActionStyle.PRIMARY
                            "destructive" -> UiActionStyle.DESTRUCTIVE
                            else -> fail("unknown action style '${Sanitize.line(s)}'")
                        }
                        UiAction(id, checkText("action.label", reqString(ao, "label"), MAX_ACTION_LABEL, false), style)
                    })
                }
                else -> fail("unknown block type '${Sanitize.line(type)}'")
            }
        }
        return MeshUiCard(version, checkedTitle, out)
    }

    fun isValidActionId(id: String): Boolean =
        id.isNotEmpty() && id.length <= MAX_ACTION_ID &&
            id.all { it in 'a'..'z' || it in 'A'..'Z' || it in '0'..'9' || it == '_' || it == '-' || it == '.' }

    // ---- helpers -------------------------------------------------------------

    private fun allowOnly(obj: JsonObject, what: String, allowed: Set<String>) {
        obj.keys.firstOrNull { it !in allowed }?.let { fail("unknown field '${Sanitize.line(it)}' in $what") }
    }

    private fun stringOf(e: JsonElement?, field: String): String {
        val p = e as? JsonPrimitive
        if (p == null || !p.isString) fail("$field must be a string")
        return p.content
    }

    private fun reqString(obj: JsonObject, key: String): String {
        val e = obj[key] ?: fail("missing '$key'")
        return stringOf(e, key)
    }

    private fun optString(obj: JsonObject, key: String): String? {
        val e = obj[key] ?: return null
        if (e is JsonNull) return null
        return stringOf(e, key)
    }

    private fun numberOf(e: JsonElement?, field: String): Double {
        val p = e as? JsonPrimitive
        if (p == null || p.isString) fail("$field must be a number")
        return p.doubleOrNull ?: fail("$field must be a number")
    }

    private fun reqNumber(obj: JsonObject, key: String): Double = numberOf(obj[key] ?: fail("missing '$key'"), key)

    private fun checkText(field: String, s: String, max: Int, allowEmpty: Boolean): String {
        if (!allowEmpty && s.isBlank()) fail("$field must not be empty")
        if (s.codePointCount(0, s.length) > max) fail("$field longer than $max characters")
        val lower = s.lowercase()
        FORBIDDEN.firstOrNull { it in lower }?.let { fail("$field contains forbidden content '$it'") }
        if (containsHtmlTag(lower)) fail("$field contains raw HTML (not allowed)")
        return Sanitize.text(s)
    }

    /** `<` followed by a letter, `/`, `!` or `?` — except markdown autolinks `<https://…>`. */
    private fun containsHtmlTag(lower: String): Boolean {
        var i = lower.indexOf('<')
        while (i >= 0) {
            val rest = lower.substring(i + 1)
            val c = rest.firstOrNull()
            val startsTag = c != null && ((c in 'a'..'z') || c == '/' || c == '!' || c == '?')
            if (startsTag && !rest.startsWith("https://")) return true
            i = lower.indexOf('<', i + 1)
        }
        return false
    }

    private fun checkHttpsUrl(s: String): String {
        if (s.length > MAX_URL) fail("image url too long")
        val uri = try {
            URI(s)
        } catch (_: URISyntaxException) {
            fail("invalid image url")
        }
        if (!"https".equals(uri.scheme, ignoreCase = true)) fail("image url must be https")
        if (uri.host.isNullOrEmpty()) fail("image url has no host")
        if (uri.rawUserInfo != null) fail("image url must not carry credentials")
        return uri.toString()
    }
}
