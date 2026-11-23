package io.mellowmesh.app.ui.chats

import androidx.compose.animation.core.animateFloat
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import io.mellowmesh.app.MellowMeshApp
import io.mellowmesh.app.R
import io.mellowmesh.app.meshApp
import io.mellowmesh.app.data.friendly
import io.mellowmesh.app.ui.components.BackHeader
import io.mellowmesh.app.ui.components.ErrorNote
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.components.Mello
import io.mellowmesh.app.ui.components.NameWithTag
import io.mellowmesh.app.ui.components.Pill
import io.mellowmesh.app.ui.components.PrincipalAvatar
import io.mellowmesh.app.ui.components.meshViewModel
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.Chat
import io.mellowmesh.core.ChatMessage
import io.mellowmesh.core.ConversationKind
import io.mellowmesh.core.ConversationRef
import io.mellowmesh.core.ConversationSummary
import io.mellowmesh.core.Conversations
import io.mellowmesh.core.Mood
import io.mellowmesh.core.UiAction
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class ConversationState(
    val title: String = "",
    val kind: ConversationKind = ConversationKind.GROUP,
    val messages: List<ChatMessage> = emptyList(),
    val loading: Boolean = true,
    val sending: Boolean = false,
    val error: String? = null,
)

class ConversationViewModel(private val app: MellowMeshApp, val conversationId: String) : ViewModel() {
    val ref: ConversationRef? = Conversations.parseId(conversationId)
    private val _state = MutableStateFlow(ConversationState())
    val state: StateFlow<ConversationState> = _state.asStateFlow()

    init {
        val conv = ref
        if (conv != null) {
            val teams = app.repo.network.value?.teams.orEmpty().associate { it.id.lowercase() to it.name }
            _state.update {
                it.copy(title = Conversations.title(conv, app.repo.me, app.repo.directory, teams), kind = conv.kind)
            }
            load()
            // A message that waited on the phone has left: show it as sent.
            viewModelScope.launch {
                var waiting = app.repo.outbox.value.count { it.conversationId == conversationId }
                app.repo.outbox.collect { list ->
                    val now = list.count { it.conversationId == conversationId }
                    if (now < waiting) load()
                    waiting = now
                }
            }
            viewModelScope.launch {
                app.repo.live.collect { msg ->
                    if (Conversations.isChatTopic(conv, msg.topic)) {
                        // A line said in a group, or an answer to a line this
                        // thread does not show: it belongs to a group thread.
                        if (io.mellowmesh.core.GroupChat.tagOf(msg) != null) return@collect
                        val parent = msg.parentId
                        if (parent != null && _state.value.messages.none { it.id == parent }) {
                            load()
                            return@collect
                        }
                        val m = Chat.toChat(msg, app.repo.me, app.repo.directory)
                        val fresh = _state.value.messages.none { it.id == m.id }
                        add(m)
                        if (fresh && !m.isMine && m.isHelper) {
                            io.mellowmesh.app.ui.components.SoundFx.play(app, io.mellowmesh.app.ui.components.SoundFx.Cue.RECEIVED)
                        }
                    }
                }
            }
        } else {
            _state.update { it.copy(loading = false, error = "This conversation cannot be opened.") }
        }
    }

    fun load() {
        viewModelScope.launch {
            try {
                val msgs = app.repo.conversation(conversationId)
                _state.update { it.copy(messages = msgs, loading = false, error = null) }
                markRead()
            } catch (e: Exception) {
                _state.update { it.copy(loading = false, error = e.friendly()) }
            }
        }
    }

    private fun add(m: ChatMessage) {
        _state.update { s ->
            if (m.id.isNotEmpty() && s.messages.any { it.id == m.id }) s
            else s.copy(messages = (s.messages + m).sortedWith(compareBy<ChatMessage> { it.tsMs }.thenBy { it.id }))
        }
        markRead()
    }

    private fun markRead() {
        val last = _state.value.messages.lastOrNull() ?: return
        app.repo.markRead(conversationId, last.tsMs)
    }

    fun send(text: String, onSent: () -> Unit) {
        if (text.isBlank()) return
        _state.update { it.copy(sending = true, error = null) }
        viewModelScope.launch {
            try {
                app.repo.sendOrQueue(conversationId, text.trim())?.let { add(it) }
                io.mellowmesh.app.ui.components.SoundFx.play(app, io.mellowmesh.app.ui.components.SoundFx.Cue.SENT)
                onSent()
            } catch (e: Exception) {
                _state.update { it.copy(error = e.friendly()) }
            } finally {
                _state.update { it.copy(sending = false) }
            }
        }
    }

    fun action(card: ChatMessage, action: UiAction) {
        viewModelScope.launch {
            try {
                add(app.repo.sendUiAction(conversationId, card.id, action.id, action.label))
            } catch (e: Exception) {
                _state.update { it.copy(error = e.friendly()) }
            }
        }
    }
}

/** A message kept on the phone until the network answers again. */
@Composable
private fun PendingBubble(text: String) {
    val t = androidx.compose.animation.core.rememberInfiniteTransition(label = "pending")
    val still = io.mellowmesh.app.ui.theme.rememberReduceMotion()
    val pulse by t.animateFloat(
        0.55f, 1f,
        androidx.compose.animation.core.infiniteRepeatable(
            androidx.compose.animation.core.tween(1100),
            androidx.compose.animation.core.RepeatMode.Reverse,
        ),
        label = "pulse",
    )
    Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.End) {
        Text(
            text,
            color = Mesh.Ink,
            modifier = Modifier
                .clip(RoundedCornerShape(18.dp))
                .background(Mesh.IndigoTint.copy(alpha = if (still) 1f else pulse))
                .border(1.dp, Mesh.Indigo.copy(alpha = 0.4f), RoundedCornerShape(18.dp))
                .padding(horizontal = 14.dp, vertical = 10.dp),
        )
        Text(
            stringResource(R.string.pending_bubble),
            color = Mesh.Ink3,
            style = MaterialTheme.typography.labelSmall,
            modifier = Modifier.padding(top = 2.dp, end = 6.dp),
        )
    }
}

@Composable
fun ConversationScreen(
    conversationId: String,
    onBack: () -> Unit,
    onTeam: (String) -> Unit,
    onHelper: (String) -> Unit,
    onOpenJourney: ((String) -> Unit)? = null,
) {
    val vm = meshViewModel(key = "conv-$conversationId") { ConversationViewModel(it, conversationId) }
    val state by vm.state.collectAsStateWithLifecycle()
    var input by rememberSaveable { mutableStateOf("") }
    var linkToOpen by remember { mutableStateOf<String?>(null) }
    val listState = rememberLazyListState()
    // What was already there when the conversation opened appears at once;
    // only what arrives afterwards rises in.
    val openedAt = remember { System.currentTimeMillis() - 1_500 }
    val uriHandler = LocalUriHandler.current
    val app = LocalContext.current.meshApp
    val outbox by app.repo.outbox.collectAsStateWithLifecycle()
    val waiting = outbox.filter { it.conversationId == conversationId }

    LaunchedEffect(state.messages.size, waiting.size) {
        val total = state.messages.size + waiting.size
        if (total > 0) listState.animateScrollToItem(total - 1)
    }

    val ref = vm.ref
    val summary = ConversationSummary(
        id = conversationId, kind = state.kind, title = state.title,
        participants = emptyList(), lastMessagePreview = null, lastActivityMs = 0, unread = 0, agentId = ref?.agentId,
    )

    Column(
        Modifier
            .fillMaxSize()
            .background(Mesh.Paper)
            .imePadding(),
    ) {
        BackHeader(
            title = state.title,
            subtitle = kindLabel(state.kind),
            onBack = onBack,
            leading = {
                Box(
                    Modifier.clickable {
                        val teamId = ref?.teamId
                        val agentId = ref?.agentId
                        if (teamId != null) onTeam(teamId) else if (agentId != null) onHelper(agentId)
                    },
                ) { ConversationAvatar(summary, Mood.IDLE, 40.dp) }
            },
        )
        // The journey this Mellow is helping along right now, as a small ribbon.
        val tasks by app.repo.allTasks.collectAsStateWithLifecycle()
        val pending by app.repo.pendingDecisions.collectAsStateWithLifecycle()
        val agentId = ref?.agentId
        val onJourney = remember(tasks, pending, agentId) {
            if (agentId == null) null else io.mellowmesh.core.Journeys.build(tasks, pending, emptyMap()).firstOrNull { j ->
                j.stations.any { it.who.equals(agentId, ignoreCase = true) && it.state != io.mellowmesh.core.StationState.DONE }
            }
        }
        androidx.compose.animation.AnimatedVisibility(visible = onJourney != null && onOpenJourney != null) {
            val j = onJourney ?: return@AnimatedVisibility
            Row(
                Modifier
                    .fillMaxWidth()
                    .padding(horizontal = 16.dp, vertical = 2.dp)
                    .clip(androidx.compose.foundation.shape.RoundedCornerShape(12.dp))
                    .background(Mesh.GreenTint)
                    .clickable { onOpenJourney?.invoke(j.slug) }
                    .padding(horizontal = 12.dp, vertical = 6.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    stringResource(R.string.journeys_ribbon, j.name, j.done + 1, j.stations.size),
                    style = MaterialTheme.typography.labelMedium,
                    color = Mesh.Ink,
                    maxLines = 1,
                )
            }
        }
        Box(Modifier.weight(1f)) {
            if (!state.loading && state.messages.isEmpty() && state.error == null) {
                Column(Modifier.fillMaxSize().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    Gap(40.dp)
                    Mello(size = 90.dp)
                    Gap()
                    Text(stringResource(R.string.empty_conversation), color = Mesh.Ink2)
                }
            }
            LazyColumn(
                state = listState,
                contentPadding = PaddingValues(horizontal = 12.dp, vertical = 8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
                modifier = Modifier.fillMaxSize(),
            ) {
                items(state.messages, key = { it.id.ifEmpty { "${it.tsMs}-${it.body.hashCode()}" } }) { m ->
                    // Messages that arrive while the conversation is open rise in
                    // from where they come (theirs from the left, yours from the composer).
                    io.mellowmesh.app.ui.components.RiseIn(
                        enabled = !state.loading && m.tsMs > openedAt,
                        fromStart = !m.isMine,
                        modifier = Modifier.animateItem(),
                    ) {
                        MessageBubble(
                            m = m,
                            showSender = state.kind != ConversationKind.AGENT && state.kind != ConversationKind.PERSON,
                            onAction = { a -> vm.action(m, a) },
                            onLink = { linkToOpen = it },
                        )
                    }
                }
                items(waiting, key = { it.id }) { p -> PendingBubble(p.text) }
            }
        }
        state.error?.let { ErrorNote(it, modifier = Modifier.padding(horizontal = 12.dp)) }
        Row(
            Modifier
                .fillMaxWidth()
                .background(Mesh.Card)
                .navigationBarsPadding()
                .padding(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            OutlinedTextField(
                value = input,
                onValueChange = { input = it },
                placeholder = { Text(stringResource(R.string.type_message)) },
                modifier = Modifier.weight(1f),
                maxLines = 5,
                shape = RoundedCornerShape(20.dp),
                colors = OutlinedTextFieldDefaults.colors(
                    focusedBorderColor = Mesh.Ink,
                    unfocusedBorderColor = Mesh.Hairline,
                ),
            )
            Spacer(Modifier.width(6.dp))
            IconButton(
                onClick = { vm.send(input) { input = "" } },
                enabled = input.isNotBlank() && !state.sending && ref != null,
            ) {
                Icon(Icons.AutoMirrored.Filled.Send, contentDescription = stringResource(R.string.send), tint = Mesh.Ink)
            }
        }
    }

    val url = linkToOpen
    if (url != null) {
        AlertDialog(
            onDismissRequest = { linkToOpen = null },
            title = { Text(stringResource(R.string.open_link_title)) },
            text = { Text(stringResource(R.string.open_link_body, url)) },
            confirmButton = {
                TextButton(onClick = {
                    linkToOpen = null
                    try {
                        uriHandler.openUri(url)
                    } catch (_: Exception) {
                        // No app can open it: nothing else to do.
                    }
                }) { Text(stringResource(R.string.open)) }
            },
            dismissButton = { TextButton(onClick = { linkToOpen = null }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

@Composable
internal fun MessageBubble(
    m: ChatMessage,
    showSender: Boolean,
    onAction: (UiAction) -> Unit,
    onLink: (String) -> Unit,
) {
    val action = m.uiAction
    if (action != null && m.isMine) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
            Pill(stringResource(R.string.action_sent, m.body), Mesh.IndigoTint, Mesh.Indigo)
        }
        return
    }
    Row(
        Modifier.fillMaxWidth(),
        horizontalArrangement = if (m.isMine) Arrangement.End else Arrangement.Start,
        verticalAlignment = Alignment.Bottom,
    ) {
        if (!m.isMine) {
            PrincipalAvatar(m.from, m.fromDisplay, 32.dp)
            Spacer(Modifier.width(8.dp))
        }
        Column(horizontalAlignment = if (m.isMine) Alignment.End else Alignment.Start) {
            if (showSender && !m.isMine) {
                NameWithTag(m.fromDisplay, isHelper = m.isHelper, modifier = Modifier.padding(start = 4.dp, bottom = 2.dp))
            }
            if (m.body.isNotBlank()) {
                Box(
                    Modifier
                        .widthIn(max = 300.dp)
                        .clip(RoundedCornerShape(18.dp))
                        .background(if (m.isMine) Mesh.Ink else Mesh.Card)
                        .padding(horizontal = 14.dp, vertical = 10.dp),
                ) {
                    Text(
                        markdownText(m.body, onLink),
                        color = if (m.isMine) Color.White else Mesh.Ink,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
            }
            val card = m.ui
            if (card != null) {
                Spacer(Modifier.size(6.dp))
                MeshCard(Modifier.widthIn(max = 320.dp)) {
                    MeshUiCardView(card, onAction = onAction, onLink = onLink)
                }
            }
        }
    }
}
