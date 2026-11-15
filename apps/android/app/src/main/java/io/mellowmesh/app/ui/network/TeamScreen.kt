package io.mellowmesh.app.ui.network

import androidx.compose.foundation.background
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
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.mellowmesh.app.R
import io.mellowmesh.app.ui.board.FilterPill
import io.mellowmesh.app.ui.components.BackHeader
import io.mellowmesh.app.ui.components.ErrorNote
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.InkButton
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.components.NameWithTag
import io.mellowmesh.app.ui.components.Pill
import io.mellowmesh.app.ui.components.PrincipalAvatar
import io.mellowmesh.app.ui.components.SectionTitle
import io.mellowmesh.app.ui.components.meshViewModel
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.Conversations
import io.mellowmesh.core.Identity
import io.mellowmesh.core.PrincipalKind
import io.mellowmesh.core.TeamRole

/** A team is a meeting room: its member row (people as initials, helpers as blobs), room chat and board. */
@Composable
fun TeamScreen(teamId: String, onBack: () -> Unit, onChat: (String) -> Unit, onBoard: () -> Unit) {
    val vm = meshViewModel(key = "network") { NetworkViewModel(it) }
    val state by vm.state.collectAsStateWithLifecycle()
    val team = state.overview?.teams?.firstOrNull { it.id == teamId }
    val owner = state.isOwner
    var adding by remember { mutableStateOf(false) }
    var confirmDelete by remember { mutableStateOf(false) }
    val title = team?.name?.ifBlank { null } ?: teamId

    Column(Modifier.fillMaxSize().background(Mesh.Paper)) {
        BackHeader(
            title = title,
            onBack = onBack,
            leading = {
                Box(Modifier.size(36.dp).clip(RoundedCornerShape(12.dp)).background(Mesh.parseHex(team?.color)))
            },
        )
        LazyColumn(
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            state.error?.let { e -> item { ErrorNote(e, onRetry = vm::load) } }
            item {
                // The member row.
                LazyRow(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    items(team?.members.orEmpty(), key = { it.principal }) { m ->
                        Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.width(64.dp)) {
                            PrincipalAvatar(m.principal, Identity.shortName(m.principal), 48.dp)
                            Text(Identity.shortName(m.principal), style = MaterialTheme.typography.labelSmall, color = Mesh.Ink, maxLines = 1)
                        }
                    }
                }
            }
            item {
                InkButton(
                    text = stringResource(R.string.team_room),
                    onClick = { onChat(Conversations.teamConversationId(teamId)) },
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            item {
                OutlinedButton(onClick = onBoard, modifier = Modifier.fillMaxWidth()) {
                    Text(stringResource(R.string.team_board), color = Mesh.Ink)
                }
            }
            item {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    SectionTitle(stringResource(R.string.members), Modifier.weight(1f))
                    if (owner) TextButton(onClick = { adding = true }) { Text(stringResource(R.string.add_member), color = Mesh.Indigo) }
                }
            }
            items(team?.members.orEmpty(), key = { "m-" + it.principal }) { m ->
                MeshCard(Modifier.fillMaxWidth(), contentPadding = 12.dp) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        PrincipalAvatar(m.principal, Identity.shortName(m.principal), 40.dp)
                        Spacer(Modifier.width(10.dp))
                        Column(Modifier.weight(1f)) {
                            NameWithTag(Identity.shortName(m.principal), isHelper = PrincipalKind.of(m.principal) == PrincipalKind.AGENT)
                            Text(m.principal, style = MaterialTheme.typography.labelSmall, color = Mesh.Ink3, maxLines = 1)
                        }
                        Pill(m.role, Mesh.Paper, Mesh.Ink2)
                        if (owner) {
                            TextButton(onClick = { vm.removeMember(teamId, m.principal) }) {
                                Text(stringResource(R.string.remove), color = Mesh.Rose)
                            }
                        }
                    }
                }
            }
            if (owner && team != null) {
                item {
                    TextButton(onClick = { confirmDelete = true }, modifier = Modifier.fillMaxWidth()) {
                        Text(stringResource(R.string.delete), color = Mesh.Rose)
                    }
                }
            }
        }
    }

    if (adding) {
        var principal by remember { mutableStateOf("") }
        var role by remember { mutableStateOf(TeamRole.MEMBER) }
        AlertDialog(
            onDismissRequest = { adding = false },
            title = { Text(stringResource(R.string.add_member)) },
            text = {
                Column {
                    OutlinedTextField(
                        value = principal,
                        onValueChange = { principal = it.trim() },
                        label = { Text(stringResource(R.string.member_principal)) },
                        singleLine = true,
                    )
                    Gap()
                    Text(stringResource(R.string.role), style = MaterialTheme.typography.labelLarge)
                    Gap(6.dp)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        TeamRole.ALL.forEach { r -> FilterPill(r, r == role) { role = r } }
                    }
                }
            },
            confirmButton = {
                TextButton(enabled = principal.contains("://"), onClick = {
                    vm.addMember(teamId, principal, role)
                    adding = false
                }) { Text(stringResource(R.string.save)) }
            },
            dismissButton = { TextButton(onClick = { adding = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }

    if (confirmDelete) {
        AlertDialog(
            onDismissRequest = { confirmDelete = false },
            text = { Text(stringResource(R.string.delete_confirm, title)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmDelete = false
                    vm.deleteTeam(teamId, onBack)
                }) { Text(stringResource(R.string.delete), color = Mesh.Rose) }
            },
            dismissButton = { TextButton(onClick = { confirmDelete = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}
