package io.mellowmesh.app.ui.chats

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.mellowmesh.app.R
import io.mellowmesh.app.meshApp
import io.mellowmesh.app.ui.components.CountBadge
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.HelperBlob
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.components.Mello
import io.mellowmesh.app.ui.components.NameWithTag
import io.mellowmesh.app.ui.components.ParticipantAvatar
import io.mellowmesh.app.ui.components.PersonAvatar
import io.mellowmesh.app.ui.components.TabHeader
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.ConversationKind
import io.mellowmesh.core.ConversationSummary
import io.mellowmesh.core.Identity
import io.mellowmesh.core.Mood
import io.mellowmesh.core.Time

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

@Composable
fun ChatsScreen(onHelp: () -> Unit, onOpen: (String) -> Unit, onFindHelpers: () -> Unit = {}) {
    val app = LocalContext.current.meshApp
    val home by app.repo.home.collectAsStateWithLifecycle()
    LaunchedEffect(Unit) { app.repo.refreshHome() }
    val conversations = home?.conversations.orEmpty()
    val moods = home?.helpers.orEmpty().associate { it.id.lowercase() to it.mood }

    Column(Modifier.fillMaxSize().background(Mesh.Paper)) {
        TabHeader(title = stringResource(R.string.tab_chats), onHelp = onHelp)
        if (conversations.isEmpty()) {
            Column(Modifier.fillMaxSize().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Gap(40.dp)
                Mello(size = 110.dp)
                Gap()
                Text(stringResource(R.string.no_chats), color = Mesh.Ink2, style = MaterialTheme.typography.bodyLarge)
                Gap(20.dp)
                io.mellowmesh.app.ui.components.InkButton(
                    text = stringResource(R.string.meet_helpers),
                    onClick = onFindHelpers,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        } else {
            LazyColumn(
                contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                items(conversations, key = { it.id }) { c ->
                    val mood = c.agentId?.let { moods[it.lowercase()] } ?: Mood.IDLE
                    ConversationRow(c, mood, onClick = { onOpen(c.id) })
                }
            }
        }
    }
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
private fun ConversationRow(c: ConversationSummary, mood: Mood, onClick: () -> Unit) {
    val now = System.currentTimeMillis()
    MeshCard(Modifier.fillMaxWidth().clickable(onClick = onClick), contentPadding = 12.dp) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            ConversationAvatar(c, mood, 52.dp)
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    NameWithTag(c.title, isHelper = c.kind == ConversationKind.AGENT, modifier = Modifier.weight(1f))
                    if (c.lastActivityMs > 0) {
                        Text(Time.ago(now, c.lastActivityMs), style = MaterialTheme.typography.labelSmall, color = Mesh.Ink3)
                    }
                }
                Text(
                    c.lastMessagePreview ?: kindLabel(c.kind),
                    style = MaterialTheme.typography.bodyMedium,
                    color = Mesh.Ink2,
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
