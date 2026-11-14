package io.mellowmesh.app.ui.todo

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import io.mellowmesh.app.MellowMeshApp
import io.mellowmesh.app.R
import io.mellowmesh.app.meshApp
import io.mellowmesh.app.data.friendly
import io.mellowmesh.app.ui.board.BoardView
import io.mellowmesh.app.ui.board.TaskGraphViewModel
import io.mellowmesh.app.ui.components.ErrorNote
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.HelperBlob
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.components.Mello
import io.mellowmesh.app.ui.components.NameWithTag
import io.mellowmesh.app.ui.components.SectionTitle
import io.mellowmesh.app.ui.components.TabHeader
import io.mellowmesh.app.ui.components.findFragmentActivity
import io.mellowmesh.app.ui.components.meshViewModel
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.Decision
import io.mellowmesh.core.DecisionOption
import io.mellowmesh.core.DecisionOutcome
import io.mellowmesh.core.HomeModelBuilder
import io.mellowmesh.core.Identity
import io.mellowmesh.core.Mood
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

data class TodoState(
    val decisions: List<Decision> = emptyList(),
    val sending: String? = null,
    val message: String? = null,
    val error: String? = null,
)

class TodoViewModel(private val app: MellowMeshApp) : ViewModel() {
    private val _state = MutableStateFlow(TodoState())
    val state: StateFlow<TodoState> = _state.asStateFlow()

    init {
        load()
        viewModelScope.launch {
            app.repo.live.collect { m -> if (m.topic.startsWith("_decision.", ignoreCase = true)) load() }
        }
    }

    fun load() {
        viewModelScope.launch {
            try {
                val me = app.repo.me
                val pending = app.repo.decisions().filter { it.isPending }
                    .sortedWith(compareByDescending<Decision> { Identity.samePrincipal(it.requiredDecider, me) }.thenBy { it.id })
                _state.update { it.copy(decisions = pending, error = null) }
            } catch (e: Exception) {
                _state.update { it.copy(error = e.friendly()) }
            }
        }
    }

    /** Called only after the biometric prompt succeeded. A live round trip: never queued. */
    fun answer(decision: Decision, option: DecisionOption, thanks: String) {
        _state.update { it.copy(sending = decision.id, error = null) }
        viewModelScope.launch {
            try {
                app.repo.respond(decision.id, option.id)
                _state.update { s -> s.copy(decisions = s.decisions.filter { it.id != decision.id }, message = thanks) }
                app.repo.refreshHome()
            } catch (e: Exception) {
                _state.update { it.copy(error = e.friendly()) }
            } finally {
                _state.update { it.copy(sending = null) }
            }
        }
    }

    fun clearMessage() = _state.update { it.copy(message = null) }
    fun showError(text: String) = _state.update { it.copy(error = text) }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun TodoScreen(onHelp: () -> Unit, onBoard: () -> Unit, onRoadmap: () -> Unit) {
    val app = LocalContext.current.meshApp
    val vm = meshViewModel { TodoViewModel(it) }
    val boardVm = meshViewModel(key = "board-mine") { TaskGraphViewModel(it, "mine") }
    val state by vm.state.collectAsStateWithLifecycle()
    val graphState by boardVm.state.collectAsStateWithLifecycle()
    var open by remember { mutableStateOf<Decision?>(null) }
    val names = app.repo.directory

    Column(Modifier.fillMaxSize().background(Mesh.Paper)) {
        TabHeader(
            title = stringResource(R.string.todo_title),
            onHelp = onHelp,
            actions = {
                TextButton(onClick = onBoard) { Text(stringResource(R.string.master_board), color = Mesh.Indigo) }
            },
        )
        state.error?.let { ErrorNote(it, onRetry = vm::load, modifier = Modifier.padding(horizontal = 16.dp)) }
        state.message?.let { msg ->
            MeshCard(Modifier.fillMaxWidth().padding(horizontal = 16.dp).clickable { vm.clearMessage() }, color = Mesh.GreenTint) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    HelperBlob(Mesh.Green, Mood.DONE, 36.dp)
                    Spacer(Modifier.width(10.dp))
                    Text(msg, color = Mesh.Ink)
                }
            }
        }

        SectionTitle(stringResource(R.string.needs_your_answer), Modifier.padding(horizontal = 16.dp))
        if (state.decisions.isEmpty()) {
            Row(Modifier.padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
                Mello(size = 40.dp, mood = Mood.DONE)
                Spacer(Modifier.width(10.dp))
                Text(stringResource(R.string.nothing_needs_you), color = Mesh.Ink2)
            }
        } else {
            LazyRow(
                contentPadding = PaddingValues(horizontal = 16.dp),
                horizontalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                items(state.decisions, key = { it.id }) { d ->
                    MeshCard(
                        Modifier.widthIn(max = 300.dp).clickable { open = d },
                        color = Mesh.AmberTint,
                    ) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            HelperBlob(Mesh.accent(Identity.accentOf(d.createdBy)), Mood.WAITING, 40.dp)
                            Spacer(Modifier.width(8.dp))
                            NameWithTag(names.nameOf(d.createdBy), isHelper = d.createdBy.startsWith("agent://"))
                        }
                        Gap(6.dp)
                        Text(d.title.ifBlank { d.question }, style = MaterialTheme.typography.titleMedium, color = Mesh.Ink, maxLines = 2)
                    }
                }
            }
        }

        SectionTitle(stringResource(R.string.my_tasks), Modifier.padding(horizontal = 16.dp))
        val board = graphState.board
        if (board != null) {
            BoardView(board = board, onMove = boardVm::move, modifier = Modifier.weight(1f))
        } else {
            graphState.error?.let { ErrorNote(it, onRetry = boardVm::load, modifier = Modifier.padding(horizontal = 16.dp)) }
        }
        TextButton(onClick = onRoadmap, modifier = Modifier.padding(horizontal = 8.dp)) {
            Text(stringResource(R.string.roadmap), color = Mesh.Indigo)
        }
    }

    val d = open
    if (d != null) {
        val context = LocalContext.current
        val helperName = names.nameOf(d.createdBy)
        val preview = HomeModelBuilder.preview(d, helperName)
        val confirmTitle = stringResource(R.string.confirm_title)
        val confirmSubtitle = stringResource(R.string.confirm_subtitle, d.title.ifBlank { d.question })
        val needLock = stringResource(R.string.need_screen_lock)
        val thanks = stringResource(R.string.answer_sent)
        ModalBottomSheet(onDismissRequest = { open = null }, containerColor = Mesh.Card) {
            Column(
                Modifier
                    .padding(horizontal = 20.dp)
                    .verticalScroll(rememberScrollState())
                    .navigationBarsPadding(),
            ) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    HelperBlob(Mesh.accent(Identity.accentOf(d.createdBy)), Mood.WAITING, 48.dp)
                    Spacer(Modifier.width(10.dp))
                    Column {
                        NameWithTag(helperName, isHelper = d.createdBy.startsWith("agent://"))
                        Text(stringResource(R.string.asked_by, helperName), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink3)
                    }
                }
                Gap()
                Text(d.title, style = MaterialTheme.typography.headlineSmall, color = Mesh.Ink)
                if (d.question.isNotBlank() && d.question != d.title) {
                    Gap(4.dp)
                    Text(d.question, style = MaterialTheme.typography.bodyLarge, color = Mesh.Ink2)
                }
                Gap()
                PreviewBlock(stringResource(R.string.if_yes), preview.ifYes, Mesh.GreenTint)
                Gap(8.dp)
                PreviewBlock(stringResource(R.string.if_not_now), preview.ifNotNow, Mesh.Paper)
                Gap(8.dp)
                PreviewBlock(stringResource(R.string.how_to_undo), preview.undo, Mesh.RoseTint)
                Gap()
                for (option in d.options) {
                    val busy = state.sending == d.id
                    val onClick: () -> Unit = {
                        val activity = context.findFragmentActivity()
                        if (activity == null || !Biometric.canConfirm(activity)) {
                            vm.showError(needLock)
                            open = null
                        } else {
                            Biometric.confirm(
                                activity = activity,
                                title = confirmTitle,
                                subtitle = confirmSubtitle,
                                onSuccess = {
                                    vm.answer(d, option, thanks)
                                    open = null
                                },
                                onFailure = { err -> if (err != null) vm.showError(err) },
                            )
                        }
                    }
                    when (option.outcome) {
                        DecisionOutcome.APPROVE -> Button(
                            onClick = onClick,
                            enabled = !busy,
                            modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
                            shape = RoundedCornerShape(16.dp),
                            colors = ButtonDefaults.buttonColors(containerColor = Mesh.Ink, contentColor = Color.White),
                        ) { Text(option.label) }
                        DecisionOutcome.REJECT -> OutlinedButton(
                            onClick = onClick,
                            enabled = !busy,
                            modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
                            shape = RoundedCornerShape(16.dp),
                        ) { Text(option.label, color = Mesh.Rose) }
                        else -> OutlinedButton(
                            onClick = onClick,
                            enabled = !busy,
                            modifier = Modifier.fillMaxWidth().padding(vertical = 4.dp),
                            shape = RoundedCornerShape(16.dp),
                        ) { Text(option.label, color = Mesh.Ink) }
                    }
                    if (option.pros.isNotEmpty() || option.cons.isNotEmpty()) {
                        Text(
                            (option.pros.map { "+ $it" } + option.cons.map { "– $it" }).joinToString("\n"),
                            style = MaterialTheme.typography.bodySmall,
                            color = Mesh.Ink2,
                            modifier = Modifier.padding(start = 8.dp, bottom = 6.dp),
                        )
                    }
                }
                TextButton(onClick = { open = null }, modifier = Modifier.fillMaxWidth()) {
                    Text(stringResource(R.string.not_now), color = Mesh.Ink2)
                }
                Gap(24.dp)
            }
        }
    }
}

@Composable
private fun PreviewBlock(title: String, body: String, tint: Color) {
    MeshCard(Modifier.fillMaxWidth(), color = tint, contentPadding = 12.dp) {
        Text(title, style = MaterialTheme.typography.labelLarge, color = Mesh.Ink)
        Text(body, style = MaterialTheme.typography.bodyMedium, color = Mesh.Ink2)
    }
}
