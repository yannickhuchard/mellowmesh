package io.mellowmesh.app.ui.plaza

import androidx.compose.animation.core.animateFloat
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
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.mellowmesh.app.R
import io.mellowmesh.app.meshApp
import io.mellowmesh.app.ui.calls.CallStrip
import io.mellowmesh.app.ui.components.AccessIcon
import io.mellowmesh.app.ui.components.ErrorNote
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.HelperBlob
import io.mellowmesh.app.ui.components.InkButton
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.components.Mello
import io.mellowmesh.app.ui.components.NameWithTag
import io.mellowmesh.app.ui.components.SectionTitle
import io.mellowmesh.app.ui.components.TabHeader
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.Access
import io.mellowmesh.core.HelperCard
import io.mellowmesh.core.HomeModel
import io.mellowmesh.core.Mood
import io.mellowmesh.core.Place
import kotlinx.coroutines.launch

@Composable
fun placeLabel(place: Place): String = stringResource(
    when (place) {
        Place.HELPERS -> R.string.place_helpers
        Place.MEETING_ROOMS -> R.string.place_meeting_rooms
        Place.CAFE -> R.string.place_cafe
        Place.TEAMS -> R.string.place_teams
        Place.OFFICES -> R.string.place_offices
        Place.TODO_DESK -> R.string.place_todo_desk
        Place.SHOPS -> R.string.place_shops
        Place.HELP_DESK -> R.string.place_help_desk
    },
)

@Composable
fun PlazaScreen(
    onHelp: () -> Unit,
    onPlace: (Place) -> Unit,
    onHelper: (String) -> Unit,
    onBoard: () -> Unit,
    onRoadmap: () -> Unit,
    onNetwork: () -> Unit,
    onSettings: () -> Unit,
    onCalls: () -> Unit,
    onCall: (String) -> Unit,
) {
    val app = LocalContext.current.meshApp
    val reachable by app.repo.reachable.collectAsStateWithLifecycle()
    val home by app.repo.home.collectAsStateWithLifecycle()
    val error by app.repo.homeError.collectAsStateWithLifecycle()
    val scope = rememberCoroutineScope()
    val labels = Place.entries.associateWith { placeLabel(it) }
    val places = defaultPlaces { labels.getValue(it) }

    Column(Modifier.fillMaxSize().background(Mesh.Paper)) {
        TabHeader(
            title = home?.greeting ?: stringResource(R.string.tab_plaza),
            onHelp = onHelp,
            actions = {
                IconButton(onClick = { scope.launch { app.repo.refreshHome() } }) {
                    Icon(Icons.Filled.Refresh, contentDescription = stringResource(R.string.refresh), tint = Mesh.Ink2)
                }
                IconButton(onClick = onSettings) {
                    Icon(Icons.Filled.Settings, contentDescription = stringResource(R.string.settings), tint = Mesh.Ink2)
                }
            },
        )
        val model = home
        if (reachable == false && (model == null || error != null)) {
            OfflinePanel(onRetry = { scope.launch { app.repo.refreshHome() } }, onCalls = onCalls, onCall = onCall)
        } else if (model == null) {
            Column(Modifier.fillMaxSize().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Gap(40.dp)
                Mello(size = 120.dp, mood = if (error == null) Mood.THINKING else Mood.IDLE)
                Gap()
                val e = error
                if (e != null) ErrorNote(e, onRetry = { scope.launch { app.repo.refreshHome() } })
                else Text(stringResource(R.string.loading), color = Mesh.Ink2)
            }
        } else {
            PlazaContent(model, places, labels, error, onPlace, onHelper, onBoard, onRoadmap, onNetwork, onCalls, onCall)
        }
    }
}

@Composable
private fun PlazaContent(
    model: HomeModel,
    places: List<MapPlace>,
    labels: Map<Place, String>,
    error: String?,
    onPlace: (Place) -> Unit,
    onHelper: (String) -> Unit,
    onBoard: () -> Unit,
    onRoadmap: () -> Unit,
    onNetwork: () -> Unit,
    onCalls: () -> Unit,
    onCall: (String) -> Unit,
) {
    val mine = model.helpers.filter { it.isMine }
    val others = model.helpers.filter { !it.isMine }
    val leadHelper = mine.firstOrNull() ?: model.helpers.firstOrNull()
    LazyColumn(
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
        modifier = Modifier.fillMaxSize(),
    ) {
        if (error != null) item { ErrorNote(error) }
        item { CallStrip(onOpenCalls = onCalls, onCall = onCall) }

        model.network?.let { net ->
            item {
                MeshCard(Modifier.fillMaxWidth().clickable(onClick = onNetwork)) {
                    Text(stringResource(R.string.your_network), style = MaterialTheme.typography.labelMedium, color = Mesh.Ink3)
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(net.name, style = MaterialTheme.typography.headlineSmall, color = Mesh.Ink, modifier = Modifier.weight(1f))
                        AccessIcon(if (net.isOpen) Access.OPEN else Access.CLOSED, size = 26.dp)
                    }
                    Text(
                        stringResource(if (net.isOpen) R.string.network_open else R.string.network_closed),
                        style = MaterialTheme.typography.bodyMedium,
                        color = Mesh.Ink2,
                    )
                    Gap(8.dp)
                    Row(horizontalArrangement = Arrangement.spacedBy(16.dp)) {
                        BigNumber(net.spaces, stringResource(R.string.spaces_count, net.spaces).substringAfter(' '))
                        BigNumber(net.teams, stringResource(R.string.teams_count, net.teams).substringAfter(' '))
                    }
                }
            }
        }

        if (model.needsYou > 0) {
            item {
                MeshCard(Modifier.fillMaxWidth().clickable { onPlace(Place.TODO_DESK) }, color = Mesh.AmberTint) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        HelperBlob(Mesh.Amber, Mood.WAITING, 52.dp)
                        Spacer(Modifier.width(12.dp))
                        Column(Modifier.weight(1f)) {
                            Text(stringResource(R.string.needs_you), style = MaterialTheme.typography.titleMedium, color = Mesh.Ink)
                            Text(
                                if (model.needsYou == 1) stringResource(R.string.needs_you_one)
                                else stringResource(R.string.needs_you_count, model.needsYou),
                                style = MaterialTheme.typography.bodyMedium,
                                color = Mesh.Ink2,
                            )
                        }
                        Text(model.needsYou.toString(), style = MaterialTheme.typography.displaySmall, color = Mesh.Ink)
                    }
                }
            }
        }

        item {
            MeshCard(Modifier.fillMaxWidth(), contentPadding = 8.dp) {
                PlazaMap(
                    places = places,
                    nextStop = model.nextStop,
                    badges = model.badges,
                    youAreHere = stringResource(R.string.you_are_here),
                    helperColor = leadHelper?.let { Mesh.accent(it.accent) } ?: Mesh.Violet,
                    helperMood = leadHelper?.mood ?: Mood.IDLE,
                    onPlace = onPlace,
                )
                Row(
                    Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(Modifier.size(10.dp).clip(CircleShape).background(Mesh.Amber))
                    Spacer(Modifier.width(8.dp))
                    Text(
                        stringResource(R.string.next_stop, labels.getValue(model.nextStop)),
                        style = MaterialTheme.typography.bodyMedium,
                        color = Mesh.Ink,
                        modifier = Modifier.weight(1f),
                    )
                }
                InkButton(
                    text = stringResource(R.string.walk_me_there),
                    onClick = { onPlace(model.nextStop) },
                    modifier = Modifier.fillMaxWidth().padding(8.dp),
                )
            }
        }

        item {
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp), modifier = Modifier.fillMaxWidth()) {
                OutlinedButton(onClick = onBoard, modifier = Modifier.weight(1f)) {
                    Text(stringResource(R.string.master_board), color = Mesh.Ink)
                }
                OutlinedButton(onClick = onRoadmap, modifier = Modifier.weight(1f)) {
                    Text(stringResource(R.string.roadmap), color = Mesh.Ink)
                }
            }
        }

        item { SectionTitle(stringResource(R.string.your_helpers)) }
        if (mine.isEmpty()) {
            item {
                MeshCard(Modifier.fillMaxWidth().clickable { onPlace(Place.SHOPS) }) {
                    Text(stringResource(R.string.no_helpers_yet), color = Mesh.Ink2)
                }
            }
        }
        items(mine, key = { it.id }) { h -> HelperRow(h) { onHelper(h.id) } }
        if (others.isNotEmpty()) {
            item { SectionTitle(stringResource(R.string.other_helpers)) }
            items(others, key = { "o-" + it.id }) { h -> HelperRow(h) { onHelper(h.id) } }
        }
    }
}

@Composable
private fun BigNumber(value: Int, label: String) {
    Column {
        Text(value.toString(), style = MaterialTheme.typography.displaySmall, color = Mesh.Ink)
        Text(label, style = MaterialTheme.typography.labelMedium, color = Mesh.Ink2)
    }
}

@Composable
fun HelperRow(h: HelperCard, onClick: () -> Unit) {
    MeshCard(Modifier.fillMaxWidth().clickable(onClick = onClick), contentPadding = 12.dp) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            HelperBlob(
                color = Mesh.accent(h.accent),
                mood = h.mood,
                size = 56.dp,
                progress = h.progress,
                description = "AI helper ${h.name}, ${h.statusLine}",
            )
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                NameWithTag(h.name, isHelper = true)
                Text(
                    h.statusLine,
                    style = MaterialTheme.typography.bodyMedium,
                    color = if (h.mood == Mood.WAITING) Mesh.Amber else Mesh.Ink2,
                    maxLines = 1,
                )
            }
            h.progress?.let {
                Text("${(it * 100).toInt()}%", style = MaterialTheme.typography.titleMedium, color = Mesh.Indigo)
            }
        }
    }
}

/**
 * The hub does not answer. Say plainly what to check, keep trying on
 * its own, and let the person keep talking to their Mellows meanwhile:
 * what they write waits on the phone and leaves when the network is back.
 */
@Composable
private fun OfflinePanel(onRetry: () -> Unit, onCalls: () -> Unit, onCall: (String) -> Unit) {
    val app = LocalContext.current.meshApp
    val outbox by app.repo.outbox.collectAsStateWithLifecycle()
    val config by app.repo.config.collectAsStateWithLifecycle()
    val still = io.mellowmesh.app.ui.theme.rememberReduceMotion()
    val t = androidx.compose.animation.core.rememberInfiniteTransition(label = "resting")
    val bob by t.animateFloat(
        0f, 1f,
        androidx.compose.animation.core.infiniteRepeatable(
            androidx.compose.animation.core.tween(1800, easing = androidx.compose.animation.core.FastOutSlowInEasing),
            androidx.compose.animation.core.RepeatMode.Reverse,
        ),
        label = "bob",
    )
    // Quietly try again every 20 seconds while this panel is shown.
    androidx.compose.runtime.LaunchedEffect(Unit) {
        while (true) {
            kotlinx.coroutines.delay(20_000)
            app.repo.refreshHome()
        }
    }
    LazyColumn(
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 24.dp),
        verticalArrangement = Arrangement.spacedBy(12.dp),
        modifier = Modifier.fillMaxSize(),
    ) {
        item {
            Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
                Gap(12.dp)
                Mello(
                    size = 110.dp,
                    mood = Mood.ASLEEP,
                    modifier = Modifier.graphicsLayer(translationY = if (still) 0f else bob * 10f),
                )
                Gap(8.dp)
                Text(
                    stringResource(R.string.offline_title),
                    style = MaterialTheme.typography.headlineSmall,
                    color = Mesh.Ink,
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
                Text(
                    stringResource(R.string.offline_intro),
                    style = MaterialTheme.typography.bodyMedium,
                    color = Mesh.Ink2,
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
            }
        }
        item {
            MeshCard(Modifier.fillMaxWidth()) {
                Text(stringResource(R.string.offline_checks_title), style = MaterialTheme.typography.titleMedium, color = Mesh.Ink)
                Gap(8.dp)
                OfflineCheck(1, stringResource(R.string.offline_check_on), null)
                OfflineCheck(2, stringResource(R.string.offline_check_running), "mellowmesh daemon start")
                OfflineCheck(
                    3,
                    stringResource(R.string.offline_check_wifi),
                    config?.baseUrl?.let { stringResource(R.string.offline_check_address, it) },
                )
                Gap(8.dp)
                InkButton(stringResource(R.string.offline_retry), onClick = onRetry, modifier = Modifier.fillMaxWidth())
                Text(
                    stringResource(R.string.offline_auto),
                    style = MaterialTheme.typography.bodySmall,
                    color = Mesh.Ink3,
                    modifier = Modifier.padding(top = 6.dp),
                )
            }
        }
        if (outbox.isNotEmpty()) {
            item {
                MeshCard(Modifier.fillMaxWidth(), color = Mesh.IndigoTint) {
                    Text(stringResource(R.string.pending_count, outbox.size), style = MaterialTheme.typography.titleSmall, color = Mesh.Ink)
                }
            }
        }
        item {
            Text(stringResource(R.string.offline_meanwhile), style = MaterialTheme.typography.titleMedium, color = Mesh.Ink)
        }
        item { CallStrip(onOpenCalls = onCalls, onCall = onCall) }
    }
}

@Composable
private fun OfflineCheck(n: Int, text: String, detail: String?) {
    Row(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
        Box(
            Modifier.size(26.dp).clip(CircleShape).background(Mesh.IndigoTint),
            contentAlignment = Alignment.Center,
        ) { Text(n.toString(), color = Mesh.Indigo, style = MaterialTheme.typography.labelLarge) }
        Spacer(Modifier.width(10.dp))
        Column(Modifier.weight(1f)) {
            Text(text, style = MaterialTheme.typography.bodyMedium, color = Mesh.Ink)
            if (detail != null) {
                Text(
                    detail,
                    style = MaterialTheme.typography.bodySmall,
                    color = Mesh.Ink2,
                    fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                )
            }
        }
    }
}
