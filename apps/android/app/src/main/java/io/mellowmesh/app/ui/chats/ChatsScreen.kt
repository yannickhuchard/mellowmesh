package io.mellowmesh.app.ui.chats

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
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
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.mellowmesh.app.R
import io.mellowmesh.app.meshApp
import io.mellowmesh.app.ui.components.CountBadge
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.HelperBlob
import io.mellowmesh.app.ui.components.HopIn
import io.mellowmesh.app.ui.components.InkButton
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.components.Mello
import io.mellowmesh.app.ui.components.NameWithTag
import io.mellowmesh.app.ui.components.ParticipantAvatar
import io.mellowmesh.app.ui.components.PersonAvatar
import io.mellowmesh.app.ui.components.SoundFx
import io.mellowmesh.app.ui.components.TabHeader
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.Accent
import io.mellowmesh.core.ChatSpace
import io.mellowmesh.core.ConversationKind
import io.mellowmesh.core.ConversationSummary
import io.mellowmesh.core.GroupView
import io.mellowmesh.core.Identity
import io.mellowmesh.core.MachineHealth
import io.mellowmesh.core.Mood
import io.mellowmesh.core.PlacedConversation
import io.mellowmesh.core.PlacedHelper
import io.mellowmesh.core.NetworkSegment
import io.mellowmesh.core.Time
import kotlinx.coroutines.delay

@Composable
fun kindLabel(kind: ConversationKind): String = stringResource(
    when (kind) {
        ConversationKind.AGENT -> R.string.kind_agent
        ConversationKind.SQUAD -> R.string.kind_squad
        ConversationKind.GROUP -> R.string.kind_group
        ConversationKind.PERSON -> R.string.kind_person
        ConversationKind.TEAM -> R.string.kind_team
        ConversationKind.ORGANIZATION -> R.string.kind_org
    },
)

/** The colour of a network segment (stable slot). */
fun segmentColor(slot: Int): Color = Mesh.accent(Accent.entries[slot % Accent.entries.size])

/**
 * Chats: one conversation space over every network. A strip of network
 * segments, the person's own groups, everyone (every Mellow, one tap to
 * talk — never an empty Chats), then the conversations of every machine.
 */
@Composable
fun ChatsScreen(onHelp: () -> Unit, onOpen: (String) -> Unit, onFindHelpers: () -> Unit = {}, onNetworks: () -> Unit = {}) {
    val app = LocalContext.current.meshApp
    val everywhere by app.repo.everywhere.collectAsStateWithLifecycle()
    val conversations by app.repo.allConversations.collectAsStateWithLifecycle()
    val groups by app.repo.groups.collectAsStateWithLifecycle()
    var segment by rememberSaveable { mutableStateOf<String?>(null) }
    var openGroup by rememberSaveable { mutableStateOf<String?>(null) }
    var newGroup by remember { mutableStateOf(false) }
    var adding by remember { mutableStateOf<PlacedHelper?>(null) }

    // Every machine, now and quietly every 30 seconds while Chats is shown.
    LaunchedEffect(Unit) {
        while (true) {
            app.repo.refreshEverywhere()
            delay(30_000)
        }
    }

    val talk: (machineId: String, conversationId: String?) -> Unit = { machineId, id ->
        if (id != null) {
            app.repo.switchMachine(machineId)
            onOpen(id)
        }
    }

    Column(Modifier.fillMaxSize().background(Mesh.Paper)) {
        TabHeader(
            title = stringResource(R.string.tab_chats),
            onHelp = onHelp,
            actions = {
                Text(
                    stringResource(R.string.chats_new_group),
                    style = MaterialTheme.typography.labelLarge,
                    color = Mesh.Card,
                    modifier = Modifier
                        .clip(CircleShape)
                        .background(Mesh.Ink)
                        .clickable { newGroup = true }
                        .padding(horizontal = 14.dp, vertical = 9.dp),
                )
            },
        )
        val ew = everywhere
        if (ew == null) {
            Column(Modifier.fillMaxSize().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Gap(40.dp)
                Mello(size = 110.dp, mood = Mood.THINKING)
                Gap()
                Text(stringResource(R.string.everywhere_loading), color = Mesh.Ink2)
            }
            return@Column
        }
        val model = ChatSpace.build(ew, conversations, groups, segment)
        val group = model.groups.firstOrNull { it.group.id == openGroup }
        val slotOf = model.segments.associate { it.key to it.slot }
        val keyOfMachine = ew.machines.associate { it.machineId to ChatSpace.segmentKey(it.networkName, it.label) }
        val everyone = group?.members ?: model.everyone
        val convs = if (group == null) model.conversations else {
            val ids = group.members.map { it.card.id.lowercase() }.toSet()
            model.conversations.filter { it.summary.agentId?.lowercase() in ids }
        }

        if (model.total == 0 && model.conversations.isEmpty()) {
            Column(Modifier.fillMaxSize().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Gap(40.dp)
                Mello(size = 110.dp)
                Gap()
                Text(stringResource(R.string.no_chats), color = Mesh.Ink2, style = MaterialTheme.typography.bodyLarge)
                Gap(20.dp)
                InkButton(stringResource(R.string.meet_helpers), onClick = onFindHelpers, modifier = Modifier.fillMaxWidth())
            }
            return@Column
        }

        LazyColumn(
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
            modifier = Modifier.fillMaxSize(),
        ) {
            if (group == null) {
                item(key = "segments") {
                    SegmentStrip(model.segments, model.total, segment, onMore = onNetworks) { segment = it }
                }
            }

            if (group != null) {
                item(key = "group-head") { GroupHead(group, onClose = { openGroup = null }, onDelete = {
                    app.repo.deleteGroup(group.group.id)
                    openGroup = null
                }) }
            } else {
                item(key = "groups-title") { SectionLabel(stringResource(R.string.chats_your_groups)) }
                item(key = "groups") {
                    Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        model.groups.forEachIndexed { i, g ->
                            HopIn(order = i) { GroupCard(g) { openGroup = g.group.id } }
                        }
                        Box(
                            Modifier
                                .size(width = 64.dp, height = 96.dp)
                                .clip(RoundedCornerShape(20.dp))
                                .border(2.dp, Mesh.Hairline, RoundedCornerShape(20.dp))
                                .clickable { newGroup = true },
                            contentAlignment = Alignment.Center,
                        ) { Text("+", style = MaterialTheme.typography.headlineSmall, color = Mesh.Ink3) }
                    }
                }
            }

            item(key = "everyone-title") {
                Row(verticalAlignment = Alignment.Bottom) {
                    SectionLabel(stringResource(if (group == null) R.string.chats_everyone else R.string.chats_in_group), Modifier.weight(1f))
                    if (group == null) Text(stringResource(R.string.chats_hold_hint), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink3)
                }
            }
            item(key = "everyone-" + (group?.group?.id ?: segment ?: "all")) {
                if (everyone.isEmpty()) {
                    Text(stringResource(R.string.everywhere_no_mellows), color = Mesh.Ink2, style = MaterialTheme.typography.bodyMedium)
                } else {
                    Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        everyone.forEachIndexed { i, h ->
                            HopIn(order = i) {
                                RosterBlob(
                                    h,
                                    dot = slotOf[keyOfMachine[h.machineId]]?.takeIf { model.segments.size > 1 }?.let(::segmentColor),
                                    onTalk = { talk(h.machineId, h.card.conversationId) },
                                    onHold = { adding = h },
                                )
                            }
                        }
                        if (group == null) NewMellowBlob(onFindHelpers)
                    }
                }
            }

            item(key = "conv-title") { SectionLabel(stringResource(R.string.chats_conversations)) }
            if (convs.isEmpty()) {
                item(key = "conv-empty") {
                    Text(stringResource(R.string.chats_no_conversation_yet), color = Mesh.Ink2, style = MaterialTheme.typography.bodyMedium)
                }
            }
            items(convs, key = { "c-" + it.machineId + it.summary.id }) { pc ->
                val mood = pc.summary.agentId?.let { a -> ew.helpers.firstOrNull { it.card.id.equals(a, true) }?.card?.mood } ?: Mood.IDLE
                ConversationRow(
                    pc.summary,
                    mood,
                    dot = slotOf[pc.segmentKey]?.takeIf { model.segments.size > 1 }?.let(::segmentColor),
                    modifier = Modifier.animateItem(),
                    onClick = { talk(pc.machineId, pc.summary.id) },
                )
            }
        }
    }

    if (newGroup) {
        NewGroupDialog(
            everyone = everywhere?.helpers.orEmpty(),
            onDismiss = { newGroup = false },
            onCreate = { name, members ->
                app.repo.createGroup(name, members)?.let {
                    SoundFx.play(app, SoundFx.Cue.JOINED)
                    newGroup = false
                }
            },
        )
    }
    adding?.let { h ->
        AddToGroupDialog(
            h,
            groups.map { g -> g to g.members.any { it.equals(h.card.id, true) } },
            onDismiss = { adding = null },
            onToggle = { g, inIt ->
                if (inIt) app.repo.removeFromGroup(g.id, h.card.id) else {
                    app.repo.addToGroup(g.id, h.card.id)
                    SoundFx.play(app, SoundFx.Cue.JOINED)
                }
            },
            onNew = {
                adding = null
                newGroup = true
            },
        )
    }
}

@Composable
internal fun SectionLabel(text: String, modifier: Modifier = Modifier) {
    Text(text.uppercase(), style = MaterialTheme.typography.labelMedium, color = Mesh.Ink3, modifier = modifier.padding(top = 4.dp))
}

/** All · each network segment with its colour and count. */
@Composable
internal fun SegmentStrip(
    segments: List<NetworkSegment>,
    total: Int,
    selected: String?,
    onMore: (() -> Unit)? = null,
    onPick: (String?) -> Unit,
) {
    Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        // One network: nothing to filter, only the door to the networks.
        if (segments.size > 1) {
            SegmentChip(stringResource(R.string.everywhere_all, total), null, selected == null) { onPick(null) }
            for (s in segments) {
                SegmentChip("${s.name} ${s.mellows}", segmentColor(s.slot).takeIf { s.live } ?: Mesh.Ink3, selected == s.key) {
                    onPick(if (selected == s.key) null else s.key)
                }
            }
        }
        if (onMore != null) {
            SegmentChip(
                if (segments.size > 1) "+" else stringResource(R.string.networks_open, segments.firstOrNull()?.name ?: ""),
                null,
                false,
                onClick = onMore,
            )
        }
    }
}

@Composable
internal fun SegmentChip(text: String, dot: Color?, selected: Boolean, onClick: () -> Unit) {
    Row(
        Modifier
            .clip(RoundedCornerShape(16.dp))
            .background(if (selected) Mesh.Ink else Mesh.Card)
            .clickable(onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (dot != null) {
            Box(Modifier.size(8.dp).clip(CircleShape).background(dot))
            Spacer(Modifier.width(6.dp))
        }
        Text(text, style = MaterialTheme.typography.labelLarge, color = if (selected) Mesh.Card else Mesh.Ink, maxLines = 1)
    }
}

@Composable
private fun GroupCard(g: GroupView, onClick: () -> Unit) {
    MeshCard(Modifier.widthIn(min = 150.dp, max = 190.dp).clickable(onClick = onClick), contentPadding = 12.dp) {
        Row {
            g.members.take(4).forEachIndexed { i, m ->
                Box(Modifier.offset(x = (-9 * i).dp)) {
                    HelperBlob(Mesh.accent(m.card.accent), m.card.mood, 30.dp, description = m.card.name)
                }
            }
            if (g.members.isEmpty()) Mello(size = 30.dp)
        }
        Gap(6.dp)
        Text(g.group.name, style = MaterialTheme.typography.titleSmall, color = Mesh.Ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
        Text(
            g.line,
            style = MaterialTheme.typography.bodySmall,
            color = if (g.members.any { it.card.mood == Mood.WAITING }) Mesh.Amber else Mesh.Ink2,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun GroupHead(g: GroupView, onClose: () -> Unit, onDelete: () -> Unit) {
    var confirm by remember { mutableStateOf(false) }
    MeshCard(Modifier.fillMaxWidth(), color = Mesh.IndigoTint) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(g.group.name, style = MaterialTheme.typography.titleLarge, color = Mesh.Ink)
                Text(stringResource(R.string.chats_group_count, g.members.size), style = MaterialTheme.typography.bodyMedium, color = Mesh.Ink2)
            }
            TextButton(onClick = onClose) { Text(stringResource(R.string.chats_all_chats), color = Mesh.Indigo) }
        }
        TextButton(onClick = { confirm = true }) { Text(stringResource(R.string.chats_delete_group), color = Mesh.Rose) }
    }
    if (confirm) {
        AlertDialog(
            onDismissRequest = { confirm = false },
            title = { Text(stringResource(R.string.chats_delete_group_title, g.group.name)) },
            text = { Text(stringResource(R.string.chats_delete_group_body)) },
            confirmButton = { TextButton(onClick = { confirm = false; onDelete() }) { Text(stringResource(R.string.chats_delete_group), color = Mesh.Rose) } },
            dismissButton = { TextButton(onClick = { confirm = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

/** A Mellow in the roster: its blob shows its mood; tap to talk, hold to group. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun RosterBlob(h: PlacedHelper, dot: Color?, onTalk: () -> Unit, onHold: () -> Unit) {
    val away = h.health != MachineHealth.ANSWERS
    Column(
        Modifier
            .width(66.dp)
            .clip(RoundedCornerShape(16.dp))
            .combinedClickable(onClick = onTalk, onLongClick = onHold)
            .padding(vertical = 6.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box {
            HelperBlob(
                color = if (away) Mesh.Ink3 else Mesh.accent(h.card.accent),
                mood = h.card.mood,
                size = 50.dp,
                progress = h.card.progress,
                description = "AI helper ${h.card.name} on ${h.machineLabel}, ${h.card.statusLine}",
            )
            if (dot != null) {
                Box(
                    Modifier
                        .align(Alignment.BottomEnd)
                        .size(13.dp)
                        .clip(CircleShape)
                        .background(Mesh.Paper)
                        .padding(2.dp)
                        .clip(CircleShape)
                        .background(dot),
                )
            }
        }
        Gap(4.dp)
        Text(
            h.card.name,
            style = MaterialTheme.typography.labelMedium,
            color = if (away) Mesh.Ink3 else Mesh.Ink,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            textAlign = TextAlign.Center,
        )
    }
}

/** The last place of the roster: find or make a new Mellow (the shops live here now). */
@Composable
private fun NewMellowBlob(onClick: () -> Unit) {
    Column(
        Modifier
            .width(66.dp)
            .clip(RoundedCornerShape(16.dp))
            .clickable(onClick = onClick)
            .padding(vertical = 6.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(
            Modifier.size(50.dp).clip(CircleShape).border(2.dp, Mesh.Hairline, CircleShape),
            contentAlignment = Alignment.Center,
        ) { Text("+", style = MaterialTheme.typography.headlineSmall, color = Mesh.Ink3) }
        Gap(4.dp)
        Text(stringResource(R.string.chats_new_mellow), style = MaterialTheme.typography.labelMedium, color = Mesh.Ink2, maxLines = 1)
    }
}

@Composable
private fun NewGroupDialog(everyone: List<PlacedHelper>, onDismiss: () -> Unit, onCreate: (String, List<String>) -> Unit) {
    var name by remember { mutableStateOf("") }
    var picked by remember { mutableStateOf(setOf<String>()) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.chats_new_group_title)) },
        text = {
            Column {
                OutlinedTextField(
                    value = name,
                    onValueChange = { name = it.take(ChatSpace.MAX_GROUP_NAME) },
                    placeholder = { Text(stringResource(R.string.chats_group_name_hint)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                Gap(8.dp)
                LazyColumn(Modifier.height(260.dp)) {
                    items(everyone, key = { it.card.id }) { h ->
                        val on = h.card.id in picked
                        Row(
                            Modifier.fillMaxWidth().clickable { picked = if (on) picked - h.card.id else picked + h.card.id },
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Checkbox(checked = on, onCheckedChange = { picked = if (on) picked - h.card.id else picked + h.card.id })
                            HelperBlob(Mesh.accent(h.card.accent), h.card.mood, 28.dp, description = h.card.name)
                            Spacer(Modifier.width(8.dp))
                            Column {
                                Text(h.card.name, style = MaterialTheme.typography.bodyLarge, color = Mesh.Ink)
                                Text(h.tag(true), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink3)
                            }
                        }
                    }
                }
            }
        },
        confirmButton = {
            TextButton(enabled = ChatSpace.cleanName(name) != null, onClick = { onCreate(name, everyone.map { it.card.id }.filter { it in picked }) }) {
                Text(stringResource(R.string.chats_create_group))
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

@Composable
private fun AddToGroupDialog(
    h: PlacedHelper,
    groups: List<Pair<io.mellowmesh.core.MellowGroup, Boolean>>,
    onDismiss: () -> Unit,
    onToggle: (io.mellowmesh.core.MellowGroup, Boolean) -> Unit,
    onNew: () -> Unit,
) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.chats_add_to_group, h.card.name)) },
        text = {
            Column {
                for ((g, inIt) in groups) {
                    Row(Modifier.fillMaxWidth().clickable { onToggle(g, inIt) }, verticalAlignment = Alignment.CenterVertically) {
                        Checkbox(checked = inIt, onCheckedChange = { onToggle(g, inIt) })
                        Text(g.name, style = MaterialTheme.typography.bodyLarge, color = Mesh.Ink)
                    }
                }
                TextButton(onClick = onNew) { Text(stringResource(R.string.chats_new_group_title), color = Mesh.Indigo) }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.done)) } },
    )
}

/** Avatar of a conversation: helper blob, person initials, or a little stack for groups. */
@Composable
fun ConversationAvatar(c: ConversationSummary, mood: Mood, size: Dp) {
    when (c.kind) {
        ConversationKind.AGENT -> {
            val p = c.participants.firstOrNull()
            HelperBlob(
                color = Mesh.accent(p?.accent ?: Identity.accentOf(c.agentId ?: c.id)),
                mood = mood,
                size = size,
                description = "AI helper ${c.title}",
            )
        }
        ConversationKind.PERSON -> PersonAvatar(c.title, size)
        ConversationKind.TEAM, ConversationKind.ORGANIZATION -> Box(
            Modifier
                .size(size)
                .clip(RoundedCornerShape(size / 3))
                .background(if (c.kind == ConversationKind.TEAM) Mesh.GreenTint else Mesh.IndigoTint),
            contentAlignment = Alignment.Center,
        ) {
            Text(Identity.initials(c.title), style = MaterialTheme.typography.titleSmall, color = Mesh.Ink)
        }
        else -> {
            // Squad / group: the two most recent participants, overlapped.
            val ps = c.participants.take(2)
            Box(Modifier.size(size)) {
                ps.forEachIndexed { i, p ->
                    ParticipantAvatar(
                        p,
                        size * 0.7f,
                        Modifier.align(if (i == 0) Alignment.TopStart else Alignment.BottomEnd),
                    )
                }
                if (ps.isEmpty()) PersonAvatar(c.title, size)
            }
        }
    }
}

@Composable
private fun ConversationRow(c: ConversationSummary, mood: Mood, dot: Color?, modifier: Modifier = Modifier, onClick: () -> Unit) {
    val now = System.currentTimeMillis()
    MeshCard(modifier.fillMaxWidth().clickable(onClick = onClick), contentPadding = 12.dp) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            ConversationAvatar(c, mood, 52.dp)
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    NameWithTag(c.title, isHelper = c.kind == ConversationKind.AGENT, modifier = Modifier.weight(1f, fill = false))
                    if (dot != null) {
                        Spacer(Modifier.width(6.dp))
                        Box(Modifier.size(8.dp).clip(CircleShape).background(dot))
                    }
                    Spacer(Modifier.weight(1f))
                    if (c.lastActivityMs > 0) {
                        Text(Time.ago(now, c.lastActivityMs), style = MaterialTheme.typography.labelSmall, color = Mesh.Ink3)
                    }
                }
                Text(
                    c.lastMessagePreview ?: kindLabel(c.kind),
                    style = MaterialTheme.typography.bodyMedium,
                    color = if (mood == Mood.WAITING) Mesh.Amber else Mesh.Ink2,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
            if (c.unread > 0) {
                Spacer(Modifier.width(8.dp))
                CountBadge(c.unread)
            }
        }
    }
}
