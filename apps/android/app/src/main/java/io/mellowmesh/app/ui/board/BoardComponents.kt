package io.mellowmesh.app.ui.board

import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.pager.HorizontalPager
import androidx.compose.foundation.pager.rememberPagerState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.mellowmesh.app.MellowMeshApp
import io.mellowmesh.app.R
import io.mellowmesh.app.data.friendly
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.InkButton
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.components.Pill
import io.mellowmesh.app.ui.components.PrincipalAvatar
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.BoardCard
import io.mellowmesh.core.BoardColumn
import io.mellowmesh.core.BoardModel
import io.mellowmesh.core.GraphScope
import io.mellowmesh.core.GraphTask
import io.mellowmesh.core.Identity
import io.mellowmesh.core.TaskGraph
import io.mellowmesh.core.TaskGraphReducer
import io.mellowmesh.core.TaskPatch
import io.mellowmesh.core.Time
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class GraphState(
    val graph: TaskGraph? = null,
    val loading: Boolean = true,
    val error: String? = null,
    val laneFilter: Set<String> = emptySet(),
    val teamFilter: String? = null,
) {
    val board: BoardModel? get() = graph?.let { BoardModel.build(it, laneFilter, teamFilter) }
}

/**
 * Loads `/tasks/graph` for a scope and keeps it live by applying
 * `_task.<id>.<event>` messages with the core reducer. Shared by boards and
 * the roadmap.
 */
class TaskGraphViewModel(private val app: MellowMeshApp, scopeText: String) : ViewModel() {
    val scope: GraphScope = GraphScope.parse(scopeText)
    private val _state = MutableStateFlow(GraphState())
    val state: StateFlow<GraphState> = _state.asStateFlow()

    init {
        load()
        viewModelScope.launch {
            app.repo.live.collect { msg ->
                if (TaskGraphReducer.parseTopic(msg.topic) == null) return@collect
                val g = _state.value.graph ?: return@collect
                val next = TaskGraphReducer.apply(g, msg, System.currentTimeMillis())
                if (next !== g) _state.update { it.copy(graph = next) }
            }
        }
    }

    fun load() {
        viewModelScope.launch {
            _state.update { it.copy(loading = true) }
            try {
                val g = app.repo.taskGraph(scope)
                _state.update { it.copy(graph = g, loading = false, error = null) }
            } catch (e: Exception) {
                _state.update { it.copy(loading = false, error = e.friendly()) }
            }
        }
    }

    fun toggleLane(id: String) = _state.update {
        it.copy(laneFilter = if (id in it.laneFilter) it.laneFilter - id else it.laneFilter + id)
    }

    fun clearFilters() = _state.update { it.copy(laneFilter = emptySet(), teamFilter = null) }

    fun setTeam(id: String?) = _state.update { it.copy(teamFilter = if (it.teamFilter == id) null else id) }

    /** Optimistic move, then `PATCH /tasks/:id {status}`; reload on failure. */
    fun move(task: GraphTask, column: BoardColumn) {
        val status = column.targetStatus
        _state.update { s ->
            s.copy(graph = s.graph?.let { g -> g.copy(tasks = g.tasks.map { if (it.id == task.id) it.copy(status = status) else it }) })
        }
        viewModelScope.launch {
            try {
                app.repo.updateTask(task.id, TaskPatch(status = status))
            } catch (e: Exception) {
                _state.update { it.copy(error = e.friendly()) }
                load()
            }
        }
    }
}

/** Column colors in the v3 language: indigo progress, amber waiting, green done. */
fun columnColor(c: BoardColumn): Color = when (c) {
    BoardColumn.TODO -> Mesh.Ink3
    BoardColumn.CLAIMED -> Mesh.Sky
    BoardColumn.DOING -> Mesh.Indigo
    BoardColumn.WAITING -> Mesh.Amber
    BoardColumn.DONE -> Mesh.Green
}

/**
 * Kanban on a phone: a horizontal pager of columns with a chip row on top.
 * Tap a card to see it; long-press it to "Move to…".
 */
@OptIn(ExperimentalFoundationApi::class, ExperimentalMaterial3Api::class)
@Composable
fun BoardView(
    board: BoardModel,
    onMove: (GraphTask, BoardColumn) -> Unit,
    modifier: Modifier = Modifier,
    header: (@Composable () -> Unit)? = null,
) {
    val columns = BoardColumn.entries
    val pager = rememberPagerState(pageCount = { columns.size })
    val scope = rememberCoroutineScope()
    var moving by remember { mutableStateOf<BoardCard?>(null) }
    var showing by remember { mutableStateOf<BoardCard?>(null) }

    Column(modifier) {
        header?.invoke()
        LazyRow(
            contentPadding = PaddingValues(horizontal = 16.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            items(columns) { c ->
                val selected = pager.currentPage == c.ordinal
                Row(
                    Modifier
                        .clip(RoundedCornerShape(50))
                        .background(if (selected) Mesh.Ink else Mesh.Card)
                        .clickable { scope.launch { pager.animateScrollToPage(c.ordinal) } }
                        .padding(horizontal = 14.dp, vertical = 8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(Modifier.size(8.dp).clip(CircleShape).background(columnColor(c)))
                    Spacer(Modifier.width(6.dp))
                    Text(
                        "${c.title}  ${board.cards(c).size}",
                        style = MaterialTheme.typography.labelLarge,
                        color = if (selected) Color.White else Mesh.Ink,
                    )
                }
            }
        }
        Gap(8.dp)
        HorizontalPager(state = pager, modifier = Modifier.fillMaxSize()) { page ->
            val column = columns[page]
            val cards = board.cards(column)
            if (cards.isEmpty()) {
                Box(Modifier.fillMaxSize().padding(24.dp)) {
                    Text(stringResource(R.string.no_tasks), color = Mesh.Ink3)
                }
            } else {
                LazyColumn(
                    contentPadding = PaddingValues(horizontal = 16.dp, vertical = 4.dp),
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                    modifier = Modifier.fillMaxSize(),
                ) {
                    items(cards, key = { it.task.id }) { card ->
                        TaskCardView(card, onClick = { showing = card }, onLongClick = { moving = card })
                    }
                }
            }
        }
    }

    val m = moving
    if (m != null) {
        ModalBottomSheet(onDismissRequest = { moving = null }, containerColor = Mesh.Card) {
            Column(Modifier.padding(horizontal = 20.dp).navigationBarsPadding()) {
                Text(stringResource(R.string.move_to), style = MaterialTheme.typography.titleLarge, color = Mesh.Ink)
                Text(m.task.title, style = MaterialTheme.typography.bodyMedium, color = Mesh.Ink2)
                Gap()
                for (c in columns) {
                    if (c == m.column) continue
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(14.dp))
                            .clickable {
                                onMove(m.task, c)
                                moving = null
                            }
                            .padding(vertical = 14.dp, horizontal = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Box(Modifier.size(12.dp).clip(CircleShape).background(columnColor(c)))
                        Spacer(Modifier.width(12.dp))
                        Text(c.title, style = MaterialTheme.typography.titleMedium, color = Mesh.Ink)
                    }
                }
                Gap(24.dp)
            }
        }
    }

    val s = showing
    if (s != null) {
        ModalBottomSheet(onDismissRequest = { showing = null }, containerColor = Mesh.Card) {
            TaskSheetContent(s.task, s.laneLabel, s.blockedByTitles, s.task.critical)
            Column(Modifier.padding(20.dp).navigationBarsPadding()) {
                InkButton(
                    text = stringResource(R.string.move_to),
                    onClick = {
                        showing = null
                        moving = s
                    },
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        }
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun TaskCardView(card: BoardCard, onClick: () -> Unit, onLongClick: () -> Unit) {
    val t = card.task
    MeshCard(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(22.dp))
            .combinedClickable(onClick = onClick, onLongClick = onLongClick),
        contentPadding = 14.dp,
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(t.title, style = MaterialTheme.typography.titleSmall, color = Mesh.Ink, modifier = Modifier.weight(1f))
            if (t.critical) Pill("!", Mesh.AmberTint, Mesh.Amber)
        }
        if (card.blockedByTitles.isNotEmpty()) {
            Text(
                stringResource(R.string.waiting_for, card.blockedByTitles.joinToString(", ")),
                style = MaterialTheme.typography.bodySmall,
                color = Mesh.Amber,
            )
        }
        Gap(8.dp)
        Row(verticalAlignment = Alignment.CenterVertically) {
            val owner = card.ownerId
            if (owner != null) {
                PrincipalAvatar(owner, Identity.shortName(owner), 22.dp)
                Spacer(Modifier.width(6.dp))
            }
            Text(card.laneLabel, style = MaterialTheme.typography.labelMedium, color = Mesh.Ink2, modifier = Modifier.weight(1f))
            val secs = if (t.runningSeconds > 0) t.runningSeconds else t.waitingSeconds
            if (secs > 0) Text(Time.shortDuration(secs), style = MaterialTheme.typography.labelMedium, color = Mesh.Ink3)
        }
    }
}

/** Task details: who, waiting / working time, blockers, critical path. */
@Composable
fun TaskSheetContent(task: GraphTask, laneLabel: String, blockedBy: List<String>, critical: Boolean) {
    Column(Modifier.padding(horizontal = 20.dp)) {
        Text(task.title, style = MaterialTheme.typography.headlineSmall, color = Mesh.Ink)
        task.description?.takeIf { it.isNotBlank() }?.let {
            Gap(4.dp)
            Text(it, style = MaterialTheme.typography.bodyMedium, color = Mesh.Ink2)
        }
        Gap()
        Text(stringResource(R.string.task_owner, laneLabel), style = MaterialTheme.typography.bodyMedium, color = Mesh.Ink)
        Gap(8.dp)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            Pill(stringResource(R.string.waited, Time.shortDuration(task.waitingSeconds)), Mesh.AmberTint, Mesh.Ink)
            Pill(stringResource(R.string.worked, Time.shortDuration(task.runningSeconds)), Mesh.IndigoTint, Mesh.Indigo)
        }
        if (blockedBy.isNotEmpty()) {
            Gap(8.dp)
            Text(stringResource(R.string.waiting_for, blockedBy.joinToString(", ")), color = Mesh.Amber, style = MaterialTheme.typography.bodyMedium)
        }
        if (critical) {
            Gap(8.dp)
            Text(stringResource(R.string.critical_path), color = Mesh.Ink2, style = MaterialTheme.typography.bodySmall)
        }
        Spacer(Modifier.height(8.dp))
    }
}
