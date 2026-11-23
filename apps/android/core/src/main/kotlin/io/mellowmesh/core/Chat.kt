package io.mellowmesh.core

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

/** A reply produced by tapping a MeshUI action. */
data class UiActionReply(val cardMessageId: String, val actionId: String)

/** A message ready to render: sanitized, with its validated card. */
data class ChatMessage(
    val id: String,
    val topic: String,
    val from: String,
    val fromKind: PrincipalKind,
    val fromDisplay: String,
    val body: String,
    val contentType: String,
    val tsMs: Long,
    val isMine: Boolean,
    val parentId: String?,
    val ui: MeshUiCard?,
    val uiAction: UiActionReply?,
) {
    val isHelper: Boolean get() = fromKind == PrincipalKind.AGENT
}

data class Participant(
    val id: String,
    val kind: PrincipalKind,
    val displayName: String,
    val accent: Accent,
) {
    val isHelper: Boolean get() = kind == PrincipalKind.AGENT
}

data class ConversationSummary(
    val id: String,
    val kind: ConversationKind,
    val title: String,
    val participants: List<Participant>,
    val lastMessagePreview: String?,
    /** 0 when there is no message in the fetched window. */
    val lastActivityMs: Long,
    val unread: Int,
    /** The helper for Agent conversations (drawn as a clay blob). */
    val agentId: String? = null,
)

object Chat {
    private const val PREVIEW_CHARS = 140
    private const val MAX_PARTICIPANTS = 16

    fun toChat(msg: Message, me: String, dir: Directory): ChatMessage {
        val payload = msg.payload as? JsonObject
        val ui = MeshUi.parseOrNull(payload?.get(MeshUi.PAYLOAD_KEY))
        val action = (payload?.get(MeshUi.ACTION_KEY) as? JsonObject)?.let { a ->
            val card = (a["card_message_id"] as? JsonPrimitive)?.takeIf { it.isString }?.content
            val act = (a["action_id"] as? JsonPrimitive)?.takeIf { it.isString }?.content
            if (card != null && act != null && MeshUi.isValidActionId(act) && card.length <= 128) {
                UiActionReply(Sanitize.line(card), act)
            } else {
                null
            }
        }
        return ChatMessage(
            id = msg.id,
            topic = msg.topic,
            from = msg.from,
            fromKind = PrincipalKind.of(msg.from),
            fromDisplay = dir.nameOf(msg.from),
            body = Sanitize.text(msg.body),
            contentType = Sanitize.line(msg.contentType),
            tsMs = msg.timestampMs,
            isMine = Identity.samePrincipal(msg.from, me),
            parentId = msg.parentId,
            ui = ui,
            uiAction = action,
        )
    }

    fun preview(m: ChatMessage): String {
        val text = if (m.body.isBlank()) m.ui?.title ?: "" else m.body
        val line = text.split(Regex("\\s+")).filter { it.isNotEmpty() }.joinToString(" ")
        return if (line.length > PREVIEW_CHARS) line.take(PREVIEW_CHARS) + "…" else line
    }

    fun participant(id: String, dir: Directory): Participant =
        Participant(id, PrincipalKind.of(id), dir.nameOf(id), Identity.accentOf(id))

    /** Messages of one conversation, chat topics only, de-duplicated, oldest first. */
    fun thread(conversationId: String, history: List<Message>, me: String, dir: Directory): List<ChatMessage> {
        val conv = Conversations.parseId(conversationId) ?: return emptyList()
        val seen = HashSet<String>()
        val group = GroupChat.isGroupTraffic(history)
        return history
            .filter { Conversations.isChatTopic(conv, it.topic) && !group(it) }
            .filter { it.id.isEmpty() || seen.add(it.id) }
            .map { toChat(it, me, dir) }
            .sortedWith(compareBy<ChatMessage> { it.tsMs }.thenBy { it.id })
    }

    /**
     * Group recent history into conversation summaries, most recent first.
     * Every one of my helpers and every team I see gets a conversation even
     * before the first message.
     */
    fun summaries(
        history: List<Message>,
        me: String,
        dir: Directory,
        myAgentIds: List<String> = emptyList(),
        teams: List<Team> = emptyList(),
        readMarkers: Map<String, Long> = emptyMap(),
    ): List<ConversationSummary> {
        val groups = LinkedHashMap<String, Pair<ConversationRef, MutableList<ChatMessage>>>()
        val seen = HashSet<String>()
        val group = GroupChat.isGroupTraffic(history)
        for (m in history) {
            if (group(m)) continue
            val conv = Conversations.classify(m.topic) ?: continue
            if (m.id.isNotEmpty() && !seen.add(m.id)) continue
            groups.getOrPut(conv.id) { conv to mutableListOf() }.second.add(toChat(m, me, dir))
        }
        for (a in myAgentIds) {
            val id = Conversations.agentConversationId(a) ?: continue
            val conv = Conversations.parseId(id) ?: continue
            groups.getOrPut(conv.id) { conv to mutableListOf() }
        }
        for (t in teams) {
            val conv = Conversations.parseId(Conversations.teamConversationId(t.id)) ?: continue
            groups.getOrPut(conv.id) { conv to mutableListOf() }
        }
        val teamNames = teams.associate { it.id.lowercase() to it.name }
        return groups.values.map { (conv, msgs) ->
            msgs.sortWith(compareBy<ChatMessage> { it.tsMs }.thenBy { it.id })
            val participants = ArrayList<Participant>()
            val pSeen = HashSet<String>()
            fun add(id: String) {
                if (participants.size < MAX_PARTICIPANTS && pSeen.add(id.lowercase())) participants.add(participant(id, dir))
            }
            conv.agentId?.let { add(it) }
            conv.dmHandles.forEach { add("human://$it") }
            msgs.asReversed().forEach { add(it.from) }
            val marker = readMarkers[conv.id] ?: 0L
            val last = msgs.lastOrNull()
            ConversationSummary(
                id = conv.id,
                kind = Conversations.effectiveKind(conv, msgs.map { it.from }, me),
                title = Conversations.title(conv, me, dir, teamNames),
                participants = participants,
                lastMessagePreview = last?.let { preview(it) },
                lastActivityMs = last?.tsMs ?: 0L,
                unread = msgs.count { !it.isMine && it.tsMs > marker },
                agentId = conv.agentId,
            )
        }.sortedWith(
            compareByDescending<ConversationSummary> { it.lastActivityMs }
                .thenBy { it.title.lowercase() }
                .thenBy { it.id },
        )
    }
}
