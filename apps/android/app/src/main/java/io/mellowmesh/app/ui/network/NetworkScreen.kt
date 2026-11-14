package io.mellowmesh.app.ui.network

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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
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
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import io.mellowmesh.app.MellowMeshApp
import io.mellowmesh.app.R
import io.mellowmesh.app.data.friendly
import io.mellowmesh.app.ui.components.AccessIcon
import io.mellowmesh.app.ui.components.BackHeader
import io.mellowmesh.app.ui.components.ErrorNote
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.components.SectionTitle
import io.mellowmesh.app.ui.components.meshViewModel
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.Access
import io.mellowmesh.core.NetworkOverview
import io.mellowmesh.core.NetworkPatch
import io.mellowmesh.core.Slug
import io.mellowmesh.core.Space
import io.mellowmesh.core.Team
import io.mellowmesh.core.TeamMember
import io.mellowmesh.core.WhoAmI
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class NetworkState(
    val overview: NetworkOverview? = null,
    val whoami: WhoAmI? = null,
    val busy: Boolean = false,
    val error: String? = null,
) {
    val isOwner: Boolean get() = whoami?.isOwner == true
}

/** Network, spaces and teams. Only the owner may change anything (the hub enforces it too). */
class NetworkViewModel(private val app: MellowMeshApp) : ViewModel() {
    private val _state = MutableStateFlow(NetworkState(app.repo.network.value, app.repo.whoami.value))
    val state: StateFlow<NetworkState> = _state.asStateFlow()

    init {
        load()
    }

    fun load() {
        viewModelScope.launch {
            try {
                val n = app.repo.loadNetwork()
                _state.update { it.copy(overview = n, whoami = app.repo.whoami.value, error = null) }
            } catch (e: Exception) {
                _state.update { it.copy(error = e.friendly()) }
            }
        }
    }

    /** Run an admin action, then reload. */
    fun admin(onDone: () -> Unit = {}, action: suspend () -> Unit) {
        _state.update { it.copy(busy = true, error = null) }
        viewModelScope.launch {
            try {
                action()
                load()
                onDone()
            } catch (e: Exception) {
                _state.update { it.copy(error = e.friendly()) }
            } finally {
                _state.update { it.copy(busy = false) }
            }
        }
    }

    fun setOpen(open: Boolean) = admin { app.repo.updateNetwork(NetworkPatch(defaultAccess = if (open) Access.OPEN else Access.CLOSED)) }
    fun rename(name: String) = admin { app.repo.updateNetwork(NetworkPatch(name = name.trim())) }
    fun saveSpace(space: Space, isNew: Boolean, onDone: () -> Unit) =
        admin(onDone) { if (isNew) app.repo.createSpace(space) else app.repo.updateSpace(space) }
    fun deleteSpace(id: String, onDone: () -> Unit) = admin(onDone) { app.repo.deleteSpace(id) }
    fun createTeam(name: String, color: String) = admin {
        app.repo.createTeam(Team(id = Slug.from(name), name = name.trim(), color = color))
    }
    fun deleteTeam(id: String, onDone: () -> Unit) = admin(onDone) { app.repo.deleteTeam(id) }
    fun addMember(teamId: String, principal: String, role: String) =
        admin { app.repo.addTeamMember(teamId, TeamMember(principal.trim(), role)) }
    fun removeMember(teamId: String, principal: String) = admin { app.repo.removeTeamMember(teamId, principal) }
}

@Composable
fun accessLabel(access: String): String = stringResource(
    when (access) {
        Access.PRIVATE -> R.string.access_private
        Access.OPEN -> R.string.access_open
        else -> R.string.access_closed
    },
)

/** Ink-on-white switch colors. */
@Composable
fun inkSwitchColors() = SwitchDefaults.colors(
    checkedThumbColor = Color.White,
    checkedTrackColor = Mesh.Ink,
    uncheckedThumbColor = Mesh.Ink3,
    uncheckedTrackColor = Mesh.Paper,
)

val TeamColors = listOf("#4F46E5", "#10B981", "#F59E0B", "#E11D48", "#38BDF8", "#8B5CF6")

@Composable
fun NetworkScreen(onBack: () -> Unit, onSpace: (String) -> Unit, onTeam: (String) -> Unit) {
    val vm = meshViewModel(key = "network") { NetworkViewModel(it) }
    val state by vm.state.collectAsStateWithLifecycle()
    var newTeam by remember { mutableStateOf(false) }
    var renaming by remember { mutableStateOf(false) }
    val overview = state.overview
    val owner = state.isOwner

    Column(Modifier.fillMaxSize().background(Mesh.Paper)) {
        BackHeader(title = stringResource(R.string.network_title), onBack = onBack)
        LazyColumn(
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            state.error?.let { e -> item { ErrorNote(e, onRetry = vm::load) } }
            if (!owner) item {
                MeshCard(Modifier.fillMaxWidth(), color = Mesh.IndigoTint) {
                    Text(stringResource(R.string.read_only_note), color = Mesh.Ink, style = MaterialTheme.typography.bodyMedium)
                }
            }
            if (overview != null) {
                item {
                    MeshCard(Modifier.fillMaxWidth().clickable(enabled = owner) { renaming = true }) {
                        Text(stringResource(R.string.network_name), style = MaterialTheme.typography.labelMedium, color = Mesh.Ink3)
                        Text(overview.network.name.ifBlank { "—" }, style = MaterialTheme.typography.headlineSmall, color = Mesh.Ink)
                        Gap()
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            AccessIcon(overview.network.defaultAccess, 26.dp)
                            Spacer(Modifier.width(10.dp))
                            Column(Modifier.weight(1f)) {
                                Text(stringResource(R.string.open_to_visitors), style = MaterialTheme.typography.titleSmall, color = Mesh.Ink)
                                Text(stringResource(R.string.open_to_visitors_hint), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2)
                            }
                            Switch(
                                checked = overview.network.defaultAccess == Access.OPEN,
                                onCheckedChange = { vm.setOpen(it) },
                                enabled = owner && !state.busy,
                                colors = inkSwitchColors(),
                            )
                        }
                    }
                }

                item {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        SectionTitle(stringResource(R.string.spaces), Modifier.weight(1f))
                        if (owner) {
                            TextButton(onClick = { onSpace(io.mellowmesh.app.ui.Routes.NEW_SPACE) }) {
                                Icon(Icons.Filled.Add, contentDescription = null, tint = Mesh.Indigo)
                                Text(stringResource(R.string.new_space), color = Mesh.Indigo)
                            }
                        }
                    }
                }
                items(overview.spaces, key = { "s-" + it.id }) { space ->
                    MeshCard(Modifier.fillMaxWidth().clickable { onSpace(space.id) }, contentPadding = 14.dp) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            AccessIcon(space.access, 28.dp)
                            Spacer(Modifier.width(12.dp))
                            Column(Modifier.weight(1f)) {
                                Text(space.name.ifBlank { space.id }, style = MaterialTheme.typography.titleSmall, color = Mesh.Ink)
                                Text(accessLabel(space.access), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2)
                                Text(
                                    space.patterns.joinToString("  "),
                                    style = MaterialTheme.typography.labelSmall,
                                    color = Mesh.Ink3,
                                    maxLines = 1,
                                )
                            }
                            Text(
                                stringResource(R.string.members_count, space.members.size),
                                style = MaterialTheme.typography.labelMedium,
                                color = Mesh.Ink2,
                            )
                        }
                    }
                }

                item {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        SectionTitle(stringResource(R.string.teams), Modifier.weight(1f))
                        if (owner) {
                            TextButton(onClick = { newTeam = true }) {
                                Icon(Icons.Filled.Add, contentDescription = null, tint = Mesh.Indigo)
                                Text(stringResource(R.string.new_team), color = Mesh.Indigo)
                            }
                        }
                    }
                }
                items(overview.teams, key = { "t-" + it.id }) { team ->
                    MeshCard(Modifier.fillMaxWidth().clickable { onTeam(team.id) }, contentPadding = 14.dp) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Box(
                                Modifier.size(32.dp).clip(RoundedCornerShape(10.dp)).background(Mesh.parseHex(team.color)),
                            )
                            Spacer(Modifier.width(12.dp))
                            Column(Modifier.weight(1f)) {
                                Text(team.name.ifBlank { team.id }, style = MaterialTheme.typography.titleSmall, color = Mesh.Ink)
                                Text(stringResource(R.string.members_count, team.members.size), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2)
                            }
                        }
                    }
                }
            }
        }
    }

    if (newTeam) {
        var name by remember { mutableStateOf("") }
        var color by remember { mutableStateOf(TeamColors[1]) }
        AlertDialog(
            onDismissRequest = { newTeam = false },
            title = { Text(stringResource(R.string.new_team)) },
            text = {
                Column {
                    OutlinedTextField(value = name, onValueChange = { name = it }, label = { Text(stringResource(R.string.team_name)) }, singleLine = true)
                    Gap()
                    Text(stringResource(R.string.team_color), style = MaterialTheme.typography.labelLarge)
                    Gap(6.dp)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        TeamColors.forEach { c ->
                            Box(
                                Modifier
                                    .size(if (c == color) 34.dp else 28.dp)
                                    .clip(CircleShape)
                                    .background(Mesh.parseHex(c))
                                    .clickable { color = c },
                            )
                        }
                    }
                }
            },
            confirmButton = {
                TextButton(
                    enabled = name.isNotBlank(),
                    onClick = {
                        vm.createTeam(name, color)
                        newTeam = false
                    },
                ) { Text(stringResource(R.string.save)) }
            },
            dismissButton = { TextButton(onClick = { newTeam = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }

    if (renaming && overview != null) {
        var name by remember { mutableStateOf(overview.network.name) }
        AlertDialog(
            onDismissRequest = { renaming = false },
            title = { Text(stringResource(R.string.network_name)) },
            text = { OutlinedTextField(value = name, onValueChange = { name = it }, singleLine = true) },
            confirmButton = {
                TextButton(enabled = name.isNotBlank(), onClick = {
                    vm.rename(name)
                    renaming = false
                }) { Text(stringResource(R.string.save)) }
            },
            dismissButton = { TextButton(onClick = { renaming = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

/** Ink outlined button used in network forms. */
@Composable
fun SmallOutlined(text: String, onClick: () -> Unit, color: Color = Mesh.Ink, enabled: Boolean = true) {
    OutlinedButton(onClick = onClick, enabled = enabled) { Text(text, color = color) }
}
