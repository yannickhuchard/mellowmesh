package io.mellowmesh.app.ui.board

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.mellowmesh.app.R
import io.mellowmesh.app.meshApp
import io.mellowmesh.app.ui.components.BackHeader
import io.mellowmesh.app.ui.components.ErrorNote
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.Mello
import io.mellowmesh.app.ui.components.meshViewModel
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.GraphScope
import io.mellowmesh.core.Identity
import io.mellowmesh.core.Mood

/** A chip used for lane / team filters. */
@Composable
fun FilterPill(text: String, selected: Boolean, onClick: () -> Unit) {
    Text(
        text,
        style = MaterialTheme.typography.labelLarge,
        color = if (selected) Color.White else Mesh.Ink,
        modifier = Modifier
            .clip(RoundedCornerShape(50))
            .background(if (selected) Mesh.Indigo else Mesh.Card)
            .clickable(onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 7.dp),
    )
}

/** Board for any scope: all tasks (master board), one helper, one team. */
@Composable
fun BoardScreen(scope: String, onBack: () -> Unit, onRoadmap: () -> Unit) {
    val app = LocalContext.current.meshApp
    val vm = meshViewModel(key = "board-$scope") { TaskGraphViewModel(it, scope) }
    val state by vm.state.collectAsStateWithLifecycle()
    val network by app.repo.network.collectAsStateWithLifecycle()
    val parsed = vm.scope
    val title = when (parsed) {
        is GraphScope.Agent -> stringResource(R.string.tasks) + " · " + Identity.shortName(parsed.query.removePrefix("agent:"))
        is GraphScope.TeamScope -> stringResource(R.string.team_board)
        GraphScope.Mine -> stringResource(R.string.my_tasks)
        else -> stringResource(R.string.master_board)
    }

    Column(Modifier.fillMaxSize().background(Mesh.Paper)) {
        BackHeader(
            title = title,
            onBack = onBack,
            actions = { TextButton(onClick = onRoadmap) { Text(stringResource(R.string.roadmap), color = Mesh.Indigo) } },
        )
        state.error?.let { ErrorNote(it, onRetry = vm::load, modifier = Modifier.padding(horizontal = 16.dp)) }
        val graph = state.graph
        val board = state.board
        if (graph == null || board == null) {
            Column(Modifier.fillMaxSize().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Gap(40.dp)
                Mello(size = 100.dp, mood = Mood.THINKING)
                Gap()
                Text(stringResource(R.string.loading), color = Mesh.Ink2)
            }
        } else {
            BoardView(
                board = board,
                onMove = vm::move,
                modifier = Modifier.fillMaxSize(),
                header = {
                    if (parsed == GraphScope.All || parsed == GraphScope.Mine) {
                        LazyRow(
                            contentPadding = PaddingValues(horizontal = 16.dp, vertical = 4.dp),
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            item {
                                FilterPill(
                                    stringResource(R.string.all_lanes),
                                    selected = state.laneFilter.isEmpty() && state.teamFilter == null,
                                    onClick = vm::clearFilters,
                                )
                            }
                            items(graph.lanes, key = { "lane-" + it.id }) { lane ->
                                FilterPill(lane.label.ifBlank { lane.id }, lane.id in state.laneFilter) { vm.toggleLane(lane.id) }
                            }
                            items(network?.teams.orEmpty(), key = { "team-" + it.id }) { team ->
                                FilterPill("# " + team.name.ifBlank { team.id }, state.teamFilter == team.id) { vm.setTeam(team.id) }
                            }
                        }
                        Gap(4.dp)
                    }
                },
            )
        }
    }
}

/** Small helper so other files can show the board count line. */
@Composable
fun BoardCountRow(text: String) {
    Row(Modifier.padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(text, style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2)
    }
}
