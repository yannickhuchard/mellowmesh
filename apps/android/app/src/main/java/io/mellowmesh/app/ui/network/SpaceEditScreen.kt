package io.mellowmesh.app.ui.network

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
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
import io.mellowmesh.app.ui.components.AccessIcon
import io.mellowmesh.app.ui.components.BackHeader
import io.mellowmesh.app.ui.components.ErrorNote
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.InkButton
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.components.SectionTitle
import io.mellowmesh.app.ui.components.meshViewModel
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.Access
import io.mellowmesh.core.Slug
import io.mellowmesh.core.Space

/** Create or edit a space: topics, access level, members, teams, visitors, peers. */
@Composable
fun SpaceEditScreen(spaceId: String?, onBack: () -> Unit) {
    val vm = meshViewModel(key = "network") { NetworkViewModel(it) }
    val state by vm.state.collectAsStateWithLifecycle()
    val existing = state.overview?.spaces?.firstOrNull { it.id == spaceId }
    val isNew = spaceId == null
    val canEdit = state.isOwner

    var loaded by remember { mutableStateOf(isNew) }
    var name by remember { mutableStateOf("") }
    var id by remember { mutableStateOf("") }
    var patterns by remember { mutableStateOf("") }
    var access by remember { mutableStateOf(Access.CLOSED) }
    var members by remember { mutableStateOf("") }
    var teams by remember { mutableStateOf(listOf<String>()) }
    var visitorWrite by remember { mutableStateOf(false) }
    var peers by remember { mutableStateOf(false) }
    var confirmDelete by remember { mutableStateOf(false) }
    var localError by remember { mutableStateOf<String?>(null) }

    LaunchedEffect(existing) {
        if (!loaded && existing != null) {
            name = existing.name
            id = existing.id
            patterns = existing.patterns.joinToString("\n")
            access = existing.access
            members = existing.members.joinToString("\n")
            teams = existing.teams
            visitorWrite = existing.visitorWrite
            peers = existing.shareWithPeers
            loaded = true
        }
    }

    Column(Modifier.fillMaxSize().background(Mesh.Paper).imePadding()) {
        BackHeader(
            title = if (isNew) stringResource(R.string.new_space) else name.ifBlank { id },
            onBack = onBack,
        )
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp),
        ) {
            state.error?.let { ErrorNote(it) }
            localError?.let { ErrorNote(it) }
            if (!canEdit) {
                MeshCard(Modifier.fillMaxWidth(), color = Mesh.IndigoTint) {
                    Text(stringResource(R.string.read_only_note), color = Mesh.Ink)
                }
                Gap()
            }
            OutlinedTextField(
                value = name,
                onValueChange = {
                    name = it
                    if (isNew) id = Slug.from(it)
                },
                label = { Text(stringResource(R.string.space_name)) },
                enabled = canEdit,
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            Gap(8.dp)
            OutlinedTextField(
                value = id,
                onValueChange = { id = it.lowercase() },
                label = { Text(stringResource(R.string.space_id)) },
                enabled = canEdit && isNew,
                singleLine = true,
                isError = id.isNotEmpty() && !Slug.isValid(id),
                modifier = Modifier.fillMaxWidth(),
            )

            SectionTitle(accessLabel(access))
            for (level in Access.ALL) {
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(14.dp))
                        .background(if (access == level) Mesh.Card else Mesh.Paper)
                        .clickable(enabled = canEdit) { access = level }
                        .padding(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    AccessIcon(level, 26.dp)
                    Spacer(Modifier.width(12.dp))
                    Text(
                        accessLabel(level),
                        style = MaterialTheme.typography.titleSmall,
                        color = if (access == level) Mesh.Ink else Mesh.Ink2,
                    )
                }
            }

            Gap()
            OutlinedTextField(
                value = patterns,
                onValueChange = { patterns = it },
                label = { Text(stringResource(R.string.space_topics)) },
                placeholder = { Text("_chat.group.family.**") },
                enabled = canEdit,
                minLines = 2,
                modifier = Modifier.fillMaxWidth(),
            )
            Gap(8.dp)
            OutlinedTextField(
                value = members,
                onValueChange = { members = it },
                label = { Text(stringResource(R.string.space_members)) },
                enabled = canEdit && access != Access.PRIVATE,
                minLines = 2,
                modifier = Modifier.fillMaxWidth(),
            )

            val allTeams = state.overview?.teams.orEmpty()
            if (allTeams.isNotEmpty()) {
                SectionTitle(stringResource(R.string.space_teams))
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    allTeams.forEach { t ->
                        FilterPill(t.name.ifBlank { t.id }, t.id in teams) {
                            if (canEdit) teams = if (t.id in teams) teams - t.id else teams + t.id
                        }
                    }
                }
            }

            Gap()
            ToggleRow(stringResource(R.string.visitor_write), visitorWrite, canEdit && access == Access.OPEN) { visitorWrite = it }
            ToggleRow(stringResource(R.string.share_with_peers), peers, canEdit) { peers = it }

            Gap(20.dp)
            if (canEdit) {
                InkButton(
                    text = stringResource(R.string.save),
                    enabled = !state.busy && name.isNotBlank() && Slug.isValid(id) && patterns.isNotBlank(),
                    onClick = {
                        val space = Space(
                            id = id,
                            name = name.trim(),
                            description = existing?.description ?: "",
                            patterns = patterns.lines().map { it.trim() }.filter { it.isNotEmpty() },
                            access = access,
                            visitorWrite = visitorWrite && access == Access.OPEN,
                            shareWithPeers = peers,
                            members = if (access == Access.PRIVATE) emptyList() else members.lines().map { it.trim() }.filter { it.isNotEmpty() },
                            teams = if (access == Access.PRIVATE) emptyList() else teams,
                        )
                        val badMember = space.members.firstOrNull { !it.contains("://") }
                        if (badMember != null) {
                            localError = "“$badMember” should look like human://name or agent://owner/name."
                        } else {
                            localError = null
                            vm.saveSpace(space, isNew, onBack)
                        }
                    },
                    modifier = Modifier.fillMaxWidth(),
                )
                if (!isNew) {
                    TextButton(onClick = { confirmDelete = true }, modifier = Modifier.fillMaxWidth()) {
                        Text(stringResource(R.string.delete), color = Mesh.Rose)
                    }
                }
            }
            Gap(32.dp)
        }
    }

    if (confirmDelete && spaceId != null) {
        AlertDialog(
            onDismissRequest = { confirmDelete = false },
            text = { Text(stringResource(R.string.delete_confirm, name.ifBlank { spaceId })) },
            confirmButton = {
                TextButton(onClick = {
                    confirmDelete = false
                    vm.deleteSpace(spaceId, onBack)
                }) { Text(stringResource(R.string.delete), color = Mesh.Rose) }
            },
            dismissButton = { TextButton(onClick = { confirmDelete = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

@Composable
fun ToggleRow(label: String, checked: Boolean, enabled: Boolean, onChange: (Boolean) -> Unit) {
    Row(Modifier.fillMaxWidth().padding(vertical = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, style = MaterialTheme.typography.bodyLarge, color = if (enabled) Mesh.Ink else Mesh.Ink3, modifier = Modifier.weight(1f))
        Switch(checked = checked, onCheckedChange = onChange, enabled = enabled, colors = inkSwitchColors())
    }
}
