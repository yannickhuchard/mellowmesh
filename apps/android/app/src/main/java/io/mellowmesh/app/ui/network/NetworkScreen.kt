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
import androidx.compose.animation.core.animateFloat
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.graphics.graphicsLayer
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
    /** AI platforms spotted on the hub's computer; null while unknown. */
    val platforms: io.mellowmesh.core.PlatformsSnapshot? = null,
    /** Name of a platform that just appeared — fuels the arrival banner. */
    val justArrived: String? = null,
    /** A Mellow is being made right now. */
    val making: Boolean = false,
    val makeError: String? = null,
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
        refreshPlatforms()
    }

    /** Ask the hub what runs on its computer. Quiet on failure: an older
     * hub has no /platforms, and that must not break the screen. */
    fun refreshPlatforms() {
        viewModelScope.launch {
            try {
                val snap = app.repo.platforms()
                val before = _state.value.platforms?.platforms?.filter { it.running }?.map { it.id }?.toSet()
                val fresh = snap.platforms.filter { it.running }
                // The first look sets the baseline; later looks celebrate.
                val arrived = if (before != null) fresh.firstOrNull { it.id !in before }?.name else null
                _state.update { it.copy(platforms = snap, justArrived = arrived ?: it.justArrived) }
            } catch (e: Exception) {
                // Not an error worth words: the section just stays away.
            }
        }
    }

    fun dismissArrival() = _state.update { it.copy(justArrived = null) }

    fun clearMakeError() = _state.update { it.copy(makeError = null) }

    /** Make a Mellow from a platform; `onMade` gets its conversation id. */
    fun makeMellow(platformId: String, name: String, purpose: String, onMade: (String) -> Unit) {
        _state.update { it.copy(making = true, makeError = null) }
        viewModelScope.launch {
            try {
                val made = app.repo.createMellow(platformId, name, purpose)
                _state.update { it.copy(making = false, justArrived = made.name) }
                onMade(made.conversationId)
            } catch (e: Exception) {
                _state.update { it.copy(making = false, makeError = e.friendly()) }
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
fun NetworkScreen(
    onBack: () -> Unit,
    onSpace: (String) -> Unit,
    onTeam: (String) -> Unit,
    onChat: (String) -> Unit = {},
) {
    val vm = meshViewModel(key = "network") { NetworkViewModel(it) }
    val state by vm.state.collectAsStateWithLifecycle()
    var makingFrom by remember { mutableStateOf<io.mellowmesh.core.PlatformInfo?>(null) }
    makingFrom?.let { platform ->
        MakeMellowDialog(
            platform = platform,
            busy = state.making,
            error = state.makeError,
            onDismiss = {
                makingFrom = null
                vm.clearMakeError()
            },
            onCreate = { name, purpose ->
                vm.makeMellow(platform.id, name, purpose) { conversationId ->
                    makingFrom = null
                    if (conversationId.isNotBlank()) onChat(conversationId)
                }
            },
        )
    }
    var newTeam by remember { mutableStateOf(false) }
    var renaming by remember { mutableStateOf(false) }
    val overview = state.overview
    val owner = state.isOwner

    // Keep looking while the screen is open: a platform that starts on the
    // computer shows up here within seconds.
    LaunchedEffect(Unit) {
        while (true) {
            kotlinx.coroutines.delay(20_000)
            vm.refreshPlatforms()
        }
    }
    state.justArrived?.let { name ->
        LaunchedEffect(name) {
            kotlinx.coroutines.delay(6_000)
            vm.dismissArrival()
        }
    }

    Column(Modifier.fillMaxSize().background(Mesh.Paper)) {
        BackHeader(title = stringResource(R.string.network_title), onBack = onBack)
        LazyColumn(
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            state.error?.let { e -> item { ErrorNote(e, onRetry = vm::load) } }
            state.justArrived?.let { name ->
                item { ArrivalBanner(name) }
            }
            state.platforms?.takeIf { it.platforms.isNotEmpty() }?.let { snap ->
                item { SectionTitle(stringResource(R.string.spotted_title)) }
                item { SpottedCard(snap, onMake = { makingFrom = it }) }
            }
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

/** A loud, happy arrival: Mello bounces, the card breathes mint. */
@Composable
fun ArrivalBanner(name: String) {
    val pulse = androidx.compose.animation.core.rememberInfiniteTransition(label = "arrive")
    val scale by pulse.animateFloat(
        initialValue = 1f,
        targetValue = 1.12f,
        animationSpec = androidx.compose.animation.core.infiniteRepeatable(
            animation = androidx.compose.animation.core.tween(650),
            repeatMode = androidx.compose.animation.core.RepeatMode.Reverse,
        ),
        label = "arrive-scale",
    )
    MeshCard(Modifier.fillMaxWidth(), color = Mesh.GreenTint) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            io.mellowmesh.app.ui.components.Mello(
                size = 54.dp,
                modifier = Modifier.graphicsLayer(scaleX = scale, scaleY = scale),
                mood = io.mellowmesh.core.Mood.DONE,
            )
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    stringResource(R.string.platform_arrived, name),
                    style = MaterialTheme.typography.titleMedium,
                    color = Mesh.Ink,
                )
                Text(
                    stringResource(R.string.platform_arrived_hint),
                    style = MaterialTheme.typography.bodySmall,
                    color = Mesh.Ink2,
                )
            }
        }
    }
}

/**
 * What the hub's computer has, in three plain states — working right now,
 * installed and asleep, not there — and, for the ones the hub can pilot,
 * the one action that matters: make a Mellow from it.
 */
@Composable
fun SpottedCard(snap: io.mellowmesh.core.PlatformsSnapshot, onMake: (io.mellowmesh.core.PlatformInfo) -> Unit) {
    val present = snap.platforms.filter { it.installed || it.running }
        .sortedWith(compareByDescending<io.mellowmesh.core.PlatformInfo> { it.canChat }.thenByDescending { it.running })
    val absent = snap.platforms.filter { !it.installed && !it.running }
    MeshCard(Modifier.fillMaxWidth()) {
        Text(stringResource(R.string.spotted_hint), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2)
        Gap(6.dp)
        if (present.isEmpty()) {
            Text(stringResource(R.string.nothing_installed), style = MaterialTheme.typography.bodyMedium, color = Mesh.Ink2)
        }
        for (p in present) {
            Column(Modifier.padding(vertical = 8.dp)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Box(
                        Modifier.size(40.dp).clip(RoundedCornerShape(11.dp)).background(Mesh.IndigoTint),
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(p.name.take(1).uppercase(), style = MaterialTheme.typography.titleMedium, color = Mesh.Indigo)
                    }
                    Spacer(Modifier.width(12.dp))
                    Column(Modifier.weight(1f)) {
                        Text(p.name, style = MaterialTheme.typography.titleSmall, color = Mesh.Ink)
                        Text(
                            stringResource(if (p.running) R.string.state_working else R.string.state_asleep),
                            style = MaterialTheme.typography.bodySmall,
                            color = if (p.running) Mesh.Green else Mesh.Ink2,
                        )
                    }
                }
                Gap(8.dp)
                if (p.canChat) {
                    io.mellowmesh.app.ui.components.InkButton(
                        text = stringResource(R.string.make_mellow),
                        onClick = { onMake(p) },
                        modifier = Modifier.fillMaxWidth(),
                    )
                } else {
                    Text(
                        stringResource(R.string.no_remote_door),
                        style = MaterialTheme.typography.bodySmall,
                        color = Mesh.Ink2,
                    )
                }
            }
        }
        if (absent.isNotEmpty()) {
            Gap(4.dp)
            Text(
                stringResource(R.string.not_on_computer, absent.joinToString(" · ") { it.name }),
                style = MaterialTheme.typography.labelSmall,
                color = Mesh.Ink3,
            )
        }
    }
}

/** Name it, give it a job: the two things that turn an app into a Mellow. */
@Composable
fun MakeMellowDialog(
    platform: io.mellowmesh.core.PlatformInfo,
    busy: Boolean,
    error: String?,
    onDismiss: () -> Unit,
    onCreate: (name: String, purpose: String) -> Unit,
) {
    var name by remember { mutableStateOf("") }
    var purpose by remember { mutableStateOf("") }
    androidx.compose.material3.AlertDialog(
        onDismissRequest = { if (!busy) onDismiss() },
        containerColor = Color.White,
        title = { Text(stringResource(R.string.make_mellow_title, platform.name), color = Mesh.Ink) },
        text = {
            Column {
                Text(stringResource(R.string.make_mellow_body), style = MaterialTheme.typography.bodyMedium, color = Mesh.Ink2)
                Gap(12.dp)
                androidx.compose.material3.OutlinedTextField(
                    value = name,
                    onValueChange = { name = it.take(48) },
                    label = { Text(stringResource(R.string.mellow_name)) },
                    placeholder = { Text(stringResource(R.string.mellow_name_hint)) },
                    singleLine = true,
                    enabled = !busy,
                    modifier = Modifier.fillMaxWidth(),
                )
                Gap(8.dp)
                androidx.compose.material3.OutlinedTextField(
                    value = purpose,
                    onValueChange = { purpose = it.take(500) },
                    label = { Text(stringResource(R.string.mellow_purpose)) },
                    placeholder = { Text(stringResource(R.string.mellow_purpose_hint)) },
                    minLines = 2,
                    enabled = !busy,
                    modifier = Modifier.fillMaxWidth(),
                )
                Gap(8.dp)
                Text(stringResource(R.string.mellow_safety), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2)
                error?.let {
                    Gap(8.dp)
                    Text(it, style = MaterialTheme.typography.bodySmall, color = Mesh.Rose)
                }
            }
        },
        confirmButton = {
            TextButton(
                onClick = { onCreate(name, purpose) },
                enabled = !busy && name.isNotBlank() && purpose.isNotBlank(),
            ) {
                Text(
                    stringResource(if (busy) R.string.making_mellow else R.string.create_say_hello),
                    color = Mesh.Indigo,
                )
            }
        },
        dismissButton = {
            TextButton(onClick = onDismiss, enabled = !busy) { Text(stringResource(R.string.cancel), color = Mesh.Ink2) }
        },
    )
}
