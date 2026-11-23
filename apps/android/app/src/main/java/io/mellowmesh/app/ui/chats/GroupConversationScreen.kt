package io.mellowmesh.app.ui.chats

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
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
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.PathMeasure
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
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
import io.mellowmesh.app.ui.components.HelperBlob
import io.mellowmesh.app.ui.components.Mello
import io.mellowmesh.app.ui.components.RiseIn
import io.mellowmesh.app.ui.components.SoundFx
import io.mellowmesh.app.ui.components.meshViewModel
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.app.ui.theme.rememberReduceMotion
import io.mellowmesh.core.ChatSpace
import io.mellowmesh.core.GroupChat
import io.mellowmesh.core.GroupHandOver
import io.mellowmesh.core.GroupItem
import io.mellowmesh.core.GroupMember
import io.mellowmesh.core.GroupSaid
import io.mellowmesh.core.MachineHealth
import io.mellowmesh.core.MellowGroup
import io.mellowmesh.core.Mood
import io.mellowmesh.core.PlacedHelper
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.distinctUntilChanged
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

data class GroupState(
    val group: MellowGroup? = null,
    val members: List<GroupMember> = emptyList(),
    val items: List<GroupItem> = emptyList(),
    val waiting: List<GroupMember> = emptyList(),
    val loading: Boolean = true,
    val sending: Boolean = false,
    val unreachable: List<String> = emptyList(),
    val error: String? = null,
)

/**
 * One conversation with several Mellows, each on its own machine and
 * network: a line is said to each member where it lives, the answers come
 * back as one thread, and an answer naming another member (`@Name`) is
 * handed over to it.
 */
class GroupConversationViewModel(private val app: MellowMeshApp, val groupId: String) : ViewModel() {
    private val _state = MutableStateFlow(GroupState())
    val state: StateFlow<GroupState> = _state.asStateFlow()
    private var liveJob: Job? = null
    private val handing = Mutex()
    private val handed = HashSet<String>()
    private var reloadJob: Job? = null

    init {
        viewModelScope.launch {
            combine(app.repo.groups, app.repo.everywhere) { groups, _ ->
                val g = groups.firstOrNull { it.id == groupId }
                g to (g?.let { app.repo.groupMembers(it) }.orEmpty())
            }.distinctUntilChanged().collect { (g, members) ->
                val changed = members != _state.value.members
                _state.update { it.copy(group = g, members = members, loading = it.loading && g != null) }
                if (g == null) {
                    _state.update { it.copy(loading = false) }
                    return@collect
                }
                if (changed) {
                    listen(members)
                    load()
                }
            }
        }
        // A quiet look every 20 s (a machine whose live line dropped), and the
        // "thinking" rows fade after a while.
        viewModelScope.launch {
            while (true) {
                delay(20_000)
                load()
            }
        }
        viewModelScope.launch { app.repo.refreshEverywhere() }
    }

    private fun listen(members: List<GroupMember>) {
        liveJob?.cancel()
        liveJob = viewModelScope.launch {
            app.repo.groupLive(members).collect { msg ->
                val fromMember = members.any { it.id.equals(msg.from, ignoreCase = true) }
                if (fromMember) SoundFx.play(app, SoundFx.Cue.RECEIVED)
                soonLoad()
            }
        }
    }

    private fun soonLoad() {
        reloadJob?.cancel()
        reloadJob = viewModelScope.launch {
            delay(250)
            load()
        }
    }

    fun load() {
        val g = _state.value.group ?: return
        val members = _state.value.members
        viewModelScope.launch {
            try {
                val read = app.repo.groupRead(members)
                val items = GroupChat.thread(g, members, read, app.repo.directory)
                _state.update {
                    it.copy(
                        items = items,
                        waiting = GroupChat.waitingOn(items, members, System.currentTimeMillis()),
                        loading = false,
                        error = null,
                    )
                }
                items.lastOrNull()?.let { app.repo.markRead("group:$groupId", it.tsMs) }
                handOver(g, members, items)
            } catch (e: Exception) {
                _state.update { it.copy(loading = false, error = e.friendly()) }
            }
        }
    }

    /** Hand over what an answer asked to hand over (once per answer and member). */
    private suspend fun handOver(g: MellowGroup, members: List<GroupMember>, items: List<GroupItem>) {
        val jobs = handing.withLock {
            GroupChat.handOvers(items, members).filter { handed.add(it.answer.message.id + "|" + it.to.id) }
        }
        if (jobs.isEmpty()) return
        for (job in jobs) {
            app.repo.sayInGroup(
                g, members, listOf(job.to), GroupChat.handOverText(job.answer), GroupChat.said(items),
                handoverOf = job.answer.message.id, handoverFrom = job.answer.member,
            )
        }
        SoundFx.play(app, SoundFx.Cue.JOINED)
        soonLoad()
    }

    fun send(text: String, picked: List<String>, onSent: () -> Unit) {
        val g = _state.value.group ?: return
        val members = _state.value.members
        val body = text.trim()
        if (body.isEmpty() || members.isEmpty()) return
        val chosen = members.filter { m -> picked.any { it.equals(m.id, true) } }
        val to = chosen.ifEmpty { GroupChat.addressees(body, members) }
        _state.update { it.copy(sending = true, error = null) }
        viewModelScope.launch {
            try {
                val missed = app.repo.sayInGroup(g, members, to, body, GroupChat.said(_state.value.items))
                _state.update { it.copy(unreachable = missed.map { m -> m.name }) }
                if (missed.size < to.size) {
                    SoundFx.play(app, SoundFx.Cue.SENT)
                    onSent()
                }
                load()
            } catch (e: Exception) {
                _state.update { it.copy(error = e.friendly()) }
            } finally {
                _state.update { it.copy(sending = false) }
            }
        }
    }

    fun remove(agentId: String) = app.repo.removeFromGroup(groupId, agentId)
    fun delete() = app.repo.deleteGroup(groupId)
}

@Composable
fun GroupConversationScreen(groupId: String, onBack: () -> Unit, onTalk: (machineId: String, conversationId: String) -> Unit) {
    val vm = meshViewModel(key = "group-$groupId") { GroupConversationViewModel(it, groupId) }
    val state by vm.state.collectAsStateWithLifecycle()
    val app = androidx.compose.ui.platform.LocalContext.current.meshApp
    val everywhere by app.repo.everywhere.collectAsStateWithLifecycle()
    var input by rememberSaveable { mutableStateOf("") }
    var picked by rememberSaveable { mutableStateOf(listOf<String>()) }
    var managing by remember { mutableStateOf(false) }
    var linkToOpen by remember { mutableStateOf<String?>(null) }
    val listState = rememberLazyListState()
    val openedAt = remember { System.currentTimeMillis() - 1_500 }
    val uriHandler = LocalUriHandler.current

    val g = state.group
    if (g == null) {
        Column(Modifier.fillMaxSize().background(Mesh.Paper)) {
            BackHeader(title = "", onBack = onBack)
            if (!state.loading) LaunchedEffect(Unit) { onBack() }
        }
        return
    }
    val placed: Map<String, PlacedHelper> = everywhere?.helpers.orEmpty().associateBy { it.card.id.lowercase() }
    val segmentOf = everywhere?.machines.orEmpty().associate { it.machineId to ChatSpace.segmentKey(it.networkName, it.label) }
    val segments = everywhere?.machines.orEmpty().filter { it.sameAs == null }
        .map { ChatSpace.segmentKey(it.networkName, it.label) }.distinct()
    val slotOf = segments.withIndex().associate { (i, k) -> k to i % 6 }
    val networks = state.members.map { segmentOf[it.machineId] }.distinct().size
    val waitingIds = state.waiting.map { it.id.lowercase() }.toSet()

    LaunchedEffect(state.items.size, state.waiting.size) {
        val total = state.items.size + state.waiting.size
        if (total > 0) listState.animateScrollToItem(total - 1)
    }

    Column(Modifier.fillMaxSize().background(Mesh.Paper).imePadding()) {
        BackHeader(
            title = g.name,
            subtitle = if (networks > 1) stringResource(R.string.group_networks, state.members.size, networks)
            else stringResource(R.string.group_one_network, state.members.size),
            onBack = onBack,
            leading = {
                Row {
                    state.members.take(3).forEachIndexed { i, m ->
                        val h = placed[m.id.lowercase()]
                        Box(Modifier.offset(x = (-12 * i).dp)) {
                            HelperBlob(
                                h?.let { Mesh.accent(it.card.accent) } ?: Mesh.Ink3,
                                if (m.id.lowercase() in waitingIds) Mood.THINKING else h?.card?.mood ?: Mood.IDLE,
                                34.dp,
                                description = m.name,
                            )
                        }
                    }
                    if (state.members.isEmpty()) Mello(size = 34.dp)
                }
            },
            actions = {
                TextButton(onClick = { managing = true }) { Text(stringResource(R.string.group_members), color = Mesh.Indigo) }
            },
        )

        // Who the next line is for: everyone, or the Mellows tapped here.
        Row(
            Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()).padding(horizontal = 12.dp, vertical = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            SegmentChip(stringResource(R.string.group_to_everyone), null, picked.isEmpty()) { picked = emptyList() }
            for (m in state.members) {
                val h = placed[m.id.lowercase()]
                val on = picked.any { it.equals(m.id, true) }
                val dot = slotOf[segmentOf[m.machineId]]?.takeIf { networks > 1 }?.let(::segmentColor)
                    ?: h?.let { Mesh.accent(it.card.accent) }
                SegmentChip(
                    m.name + if (h != null && h.health != MachineHealth.ANSWERS) " · away" else "",
                    dot,
                    on,
                ) { picked = if (on) picked.filterNot { it.equals(m.id, true) } else picked + m.id }
            }
        }

        Box(Modifier.weight(1f)) {
            if (!state.loading && state.items.isEmpty() && state.error == null) {
                Column(Modifier.fillMaxSize().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    Gap(32.dp)
                    Row {
                        state.members.take(4).forEachIndexed { i, m ->
                            val h = placed[m.id.lowercase()]
                            Box(Modifier.offset(x = (-14 * i).dp)) {
                                HelperBlob(h?.let { Mesh.accent(it.card.accent) } ?: Mesh.Ink3, Mood.IDLE, 64.dp, description = m.name)
                            }
                        }
                        if (state.members.isEmpty()) Mello(size = 80.dp)
                    }
                    Gap()
                    Text(
                        stringResource(if (state.members.isEmpty()) R.string.group_no_members else R.string.group_empty),
                        color = Mesh.Ink2,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
            }
            LazyColumn(
                state = listState,
                contentPadding = PaddingValues(horizontal = 12.dp, vertical = 8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
                modifier = Modifier.fillMaxSize(),
            ) {
                items(state.items, key = { it.key }) { item ->
                    RiseIn(enabled = !state.loading && item.tsMs > openedAt, fromStart = !(item is GroupSaid && item.message.isMine), modifier = Modifier.animateItem()) {
                        when (item) {
                            is GroupSaid -> Column(Modifier.fillMaxWidth()) {
                                MessageBubble(item.message, showSender = true, onAction = {}, onLink = { linkToOpen = it })
                                if (item.member == null && item.toNames.isNotEmpty()) {
                                    Text(
                                        stringResource(R.string.group_to, item.toNames.joinToString(", ")),
                                        style = MaterialTheme.typography.labelSmall,
                                        color = Mesh.Ink3,
                                        modifier = Modifier.align(Alignment.End).padding(end = 8.dp, top = 2.dp),
                                    )
                                }
                            }
                            is GroupHandOver -> HandOverRow(
                                item,
                                from = placed[item.fromId.lowercase()],
                                to = placed[item.toId.lowercase()],
                                animate = item.tsMs > openedAt,
                            )
                        }
                    }
                }
                items(state.waiting, key = { "w-" + it.id }) { m ->
                    val h = placed[m.id.lowercase()]
                    Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.animateItem()) {
                        HelperBlob(h?.let { Mesh.accent(it.card.accent) } ?: Mesh.Ink3, Mood.THINKING, 28.dp, description = m.name)
                        Spacer(Modifier.width(8.dp))
                        Text(stringResource(R.string.group_thinking, m.name), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2)
                    }
                }
            }
        }
        if (state.unreachable.isNotEmpty()) {
            ErrorNote(stringResource(R.string.group_away, state.unreachable.joinToString(", ")), modifier = Modifier.padding(horizontal = 12.dp))
        }
        state.error?.let { ErrorNote(it, modifier = Modifier.padding(horizontal = 12.dp)) }
        Row(
            Modifier.fillMaxWidth().background(Mesh.Card).navigationBarsPadding().padding(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            val names = state.members.filter { m -> picked.any { it.equals(m.id, true) } }.map { it.name }
            OutlinedTextField(
                value = input,
                onValueChange = { input = it },
                placeholder = {
                    Text(
                        if (names.isEmpty()) stringResource(R.string.group_write_everyone)
                        else stringResource(R.string.group_write_to, names.joinToString(", ")),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                },
                modifier = Modifier.weight(1f),
                maxLines = 5,
                shape = RoundedCornerShape(20.dp),
                colors = OutlinedTextFieldDefaults.colors(focusedBorderColor = Mesh.Ink, unfocusedBorderColor = Mesh.Hairline),
            )
            Spacer(Modifier.width(6.dp))
            IconButton(
                onClick = { vm.send(input, picked) { input = "" } },
                enabled = input.isNotBlank() && !state.sending && state.members.isNotEmpty(),
            ) {
                Icon(Icons.AutoMirrored.Filled.Send, contentDescription = stringResource(R.string.send), tint = Mesh.Ink)
            }
        }
    }

    if (managing) {
        var confirmDelete by remember { mutableStateOf(false) }
        AlertDialog(
            onDismissRequest = { managing = false },
            title = { Text(g.name) },
            text = {
                Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    for (m in state.members) {
                        val h = placed[m.id.lowercase()]
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            HelperBlob(h?.let { Mesh.accent(it.card.accent) } ?: Mesh.Ink3, h?.card?.mood ?: Mood.IDLE, 28.dp, description = m.name)
                            Spacer(Modifier.width(8.dp))
                            Column(Modifier.weight(1f)) {
                                Text(m.name, style = MaterialTheme.typography.titleSmall, color = Mesh.Ink)
                                h?.let { Text(it.tag(withNetwork = true), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink3) }
                            }
                            TextButton(onClick = { managing = false; onTalk(m.machineId, m.conversationId) }) {
                                Text(stringResource(R.string.group_talk_alone), color = Mesh.Indigo)
                            }
                            TextButton(onClick = { vm.remove(m.id) }) { Text(stringResource(R.string.group_remove), color = Mesh.Ink2) }
                        }
                    }
                    if (confirmDelete) {
                        Text(stringResource(R.string.chats_delete_group_body), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2)
                    }
                }
            },
            confirmButton = {
                TextButton(onClick = {
                    if (!confirmDelete) confirmDelete = true else {
                        managing = false
                        vm.delete()
                        onBack()
                    }
                }) { Text(stringResource(R.string.chats_delete_group), color = Mesh.Rose) }
            },
            dismissButton = { TextButton(onClick = { managing = false }) { Text(stringResource(R.string.close)) } },
        )
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
                    runCatching { uriHandler.openUri(url) }
                }) { Text(stringResource(R.string.open)) }
            },
            dismissButton = { TextButton(onClick = { linkToOpen = null }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

/** A Mellow passes the word: a line arcs from its blob to the other one's. */
@Composable
private fun HandOverRow(h: GroupHandOver, from: PlacedHelper?, to: PlacedHelper?, animate: Boolean) {
    val still = rememberReduceMotion()
    val p = remember { Animatable(if (animate && !still) 0f else 1f) }
    LaunchedEffect(Unit) { if (p.value < 1f) p.animateTo(1f, tween(700)) }
    val fromColor = from?.let { Mesh.accent(it.card.accent) } ?: Mesh.Ink3
    val toColor = to?.let { Mesh.accent(it.card.accent) } ?: Mesh.Ink3
    Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            HelperBlob(fromColor, Mood.IDLE, 26.dp, description = h.fromName)
            Canvas(Modifier.width(96.dp).height(30.dp)) {
                val path = Path().apply {
                    moveTo(4f, size.height * 0.75f)
                    quadraticTo(size.width / 2, -size.height * 0.35f, size.width - 6f, size.height * 0.75f)
                }
                val measure = PathMeasure().apply { setPath(path, false) }
                val part = Path()
                measure.getSegment(0f, measure.length * p.value, part, true)
                drawPath(
                    part,
                    color = toColor,
                    style = Stroke(width = 2.5.dp.toPx(), cap = StrokeCap.Round, pathEffect = PathEffect.dashPathEffect(floatArrayOf(10f, 8f))),
                )
                if (p.value > 0.95f) {
                    val tip = Offset(size.width - 6f, size.height * 0.75f)
                    drawCircle(toColor, radius = 3.5.dp.toPx(), center = tip)
                }
            }
            HelperBlob(toColor, if (p.value < 1f) Mood.THINKING else Mood.IDLE, 26.dp, description = h.toName)
        }
        Text(
            stringResource(R.string.group_hands_over, h.fromName, h.toName),
            style = MaterialTheme.typography.labelSmall,
            color = Mesh.Ink3,
            modifier = Modifier
                .clip(CircleShape)
                .background(Mesh.Card)
                .border(1.dp, Mesh.Hairline, CircleShape)
                .padding(horizontal = 10.dp, vertical = 3.dp),
        )
    }
}
