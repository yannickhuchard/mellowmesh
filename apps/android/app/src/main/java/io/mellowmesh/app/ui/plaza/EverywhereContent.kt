package io.mellowmesh.app.ui.plaza

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.mellowmesh.app.R
import io.mellowmesh.app.data.MachineStatus
import io.mellowmesh.app.meshApp
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.HelperBlob
import io.mellowmesh.app.ui.components.InkButton
import io.mellowmesh.app.ui.components.MachineDot
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.components.Mello
import io.mellowmesh.app.ui.components.NameWithTag
import io.mellowmesh.app.ui.components.SectionTitle
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.EverywhereModel
import io.mellowmesh.core.MachineHealth
import io.mellowmesh.core.Mood
import io.mellowmesh.core.PlacedHelper
import io.mellowmesh.core.Place
import kotlinx.coroutines.delay

/**
 * The Everywhere Plaza: one dot per machine, what needs the person over
 * every machine, then every Mellow in one list with its machine as a tag.
 * A machine in trouble becomes one card with its fix; it never empties the
 * view.
 */
@Composable
fun EverywhereContent(
    onAddMachine: () -> Unit,
    onPlace: (Place) -> Unit,
    onHelper: (String) -> Unit,
    onCall: (machineId: String, agentId: String) -> Unit,
) {
    val app = LocalContext.current.meshApp
    val model by app.repo.everywhere.collectAsStateWithLifecycle()
    val machines by app.repo.machines.collectAsStateWithLifecycle()
    var filter by rememberSaveable { mutableStateOf<String?>(null) }
    var removing by remember { mutableStateOf<MachineStatus?>(null) }

    // Ask every machine now, then quietly every 30 seconds while shown.
    LaunchedEffect(Unit) {
        while (true) {
            app.repo.refreshEverywhere()
            delay(30_000)
        }
    }

    val m = model
    if (m == null) {
        Column(Modifier.fillMaxSize().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            Gap(40.dp)
            Mello(size = 110.dp, mood = Mood.THINKING)
            Gap()
            Text(stringResource(R.string.everywhere_loading), color = Mesh.Ink2)
        }
        return
    }
    if (filter != null && m.machines.none { it.machineId == filter }) filter = null
    val shown = m.only(filter)
    val mine = shown.filter { it.card.isMine }
    val others = shown.filter { !it.card.isMine }

    val open: (PlacedHelper) -> Unit = { h ->
        if (h.health == MachineHealth.ANSWERS) {
            app.repo.switchMachine(h.machineId)
            onHelper(h.card.id)
        } else {
            // Out of reach: the call screen keeps what is written until it answers.
            onCall(h.machineId, h.card.id)
        }
    }

    LazyColumn(
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 8.dp, bottom = 24.dp),
        verticalArrangement = Arrangement.spacedBy(10.dp),
        modifier = Modifier.fillMaxSize(),
    ) {
        item { MachineStrip(m, filter) { filter = if (filter == it) null else it } }

        if (m.needsYou > 0) {
            item {
                val where = m.needsYouOn.joinToString(" · ") { it.label }
                MeshCard(
                    Modifier.fillMaxWidth().clickable {
                        m.needsYouOn.firstOrNull()?.let { app.repo.switchMachine(it.machineId) }
                        onPlace(Place.TODO_DESK)
                    },
                    color = Mesh.AmberTint,
                ) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        HelperBlob(Mesh.Amber, Mood.WAITING, 48.dp)
                        Spacer(Modifier.width(12.dp))
                        Column(Modifier.weight(1f)) {
                            Text(stringResource(R.string.needs_you), style = MaterialTheme.typography.titleMedium, color = Mesh.Ink)
                            Text(
                                stringResource(
                                    R.string.everywhere_needs_on,
                                    if (m.needsYou == 1) stringResource(R.string.needs_you_one) else stringResource(R.string.needs_you_count, m.needsYou),
                                    where,
                                ),
                                style = MaterialTheme.typography.bodyMedium,
                                color = Mesh.Ink2,
                            )
                        }
                        Text(m.needsYou.toString(), style = MaterialTheme.typography.displaySmall, color = Mesh.Ink)
                    }
                }
            }
        }

        if (m.machines.size > 1) {
            item {
                Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    FilterChip(stringResource(R.string.everywhere_all, m.helpers.size), filter == null) { filter = null }
                    for (s in m.machines) {
                        FilterChip("${s.label} ${s.mellows}", filter == s.machineId, dim = s.health != MachineHealth.ANSWERS) {
                            filter = s.machineId
                        }
                    }
                }
            }
        }

        item { SectionTitle(stringResource(R.string.everywhere_your_mellows)) }
        if (mine.isEmpty()) {
            item {
                MeshCard(Modifier.fillMaxWidth().clickable { onPlace(Place.SHOPS) }) {
                    Text(stringResource(R.string.everywhere_no_mellows), color = Mesh.Ink2)
                }
            }
        }
        items(mine, key = { "e-" + it.card.id }) { h -> PlacedRow(h, m.showNetworks) { open(h) } }
        if (others.isNotEmpty()) {
            item { SectionTitle(stringResource(R.string.everywhere_other_mellows)) }
            items(others, key = { "eo-" + it.card.id }) { h -> PlacedRow(h, m.showNetworks) { open(h) } }
        }

        // A machine to fix comes last: it must not push the working ones down.
        for (st in machines.filter { it.health.needsFix }) {
            item(key = "fix-" + st.machine.id) {
                TroubleCard(st, onPairAgain = onAddMachine, onRemove = { removing = st })
            }
        }
    }

    removing?.let { st ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text(stringResource(R.string.machine_remove_title, st.name)) },
            text = { Text(stringResource(R.string.machine_remove_confirm, st.name)) },
            confirmButton = {
                TextButton(onClick = {
                    removing = null
                    app.repo.removeMachine(st.machine.id)
                }) { Text(stringResource(R.string.machine_remove), color = Mesh.Rose) }
            },
            dismissButton = { TextButton(onClick = { removing = null }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

/** One dot per machine with its Mellow count; a tap narrows the list to it. */
@Composable
private fun MachineStrip(m: EverywhereModel, filter: String?, onPick: (String) -> Unit) {
    MeshCard(Modifier.fillMaxWidth(), contentPadding = 10.dp) {
        Row(
            Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()),
            horizontalArrangement = Arrangement.SpaceEvenly,
        ) {
            for (s in m.machines) {
                val selected = filter == s.machineId
                Column(
                    Modifier
                        .widthIn(min = 76.dp)
                        .clip(RoundedCornerShape(14.dp))
                        .background(if (selected) Mesh.IndigoTint else Mesh.Card)
                        .clickable { onPick(s.machineId) }
                        .padding(horizontal = 8.dp, vertical = 6.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    MachineDot(s.health, size = 14.dp)
                    Text(
                        s.label,
                        style = MaterialTheme.typography.labelLarge,
                        color = if (s.health == MachineHealth.ANSWERS) Mesh.Ink else Mesh.Ink2,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.widthIn(max = 110.dp),
                    )
                    Text(
                        when (s.health) {
                            MachineHealth.ADDRESS_LOST -> stringResource(R.string.machine_address_lost)
                            MachineHealth.KEY_REFUSED -> stringResource(R.string.machine_key_refused)
                            MachineHealth.RESTING -> stringResource(R.string.machine_resting_short, s.mellows)
                            else -> s.sameAs?.let { stringResource(R.string.machine_same_as, it) }
                                ?: stringResource(R.string.machine_mellows, s.mellows)
                        },
                        style = MaterialTheme.typography.bodySmall,
                        color = if (s.health.needsFix) Mesh.Amber else Mesh.Ink3,
                        maxLines = 1,
                    )
                }
            }
        }
    }
}

@Composable
private fun FilterChip(text: String, selected: Boolean, dim: Boolean = false, onClick: () -> Unit) {
    Text(
        text,
        style = MaterialTheme.typography.labelLarge,
        color = when {
            selected -> Mesh.Card
            dim -> Mesh.Ink3
            else -> Mesh.Ink
        },
        modifier = Modifier
            .clip(RoundedCornerShape(14.dp))
            .background(if (selected) Mesh.Ink else Mesh.Card)
            .clickable(onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 7.dp),
    )
}

/** A Mellow with the machine (and network, when several) it lives on. */
@Composable
private fun PlacedRow(h: PlacedHelper, withNetwork: Boolean, onClick: () -> Unit) {
    val c = h.card
    val away = h.health != MachineHealth.ANSWERS
    MeshCard(Modifier.fillMaxWidth().clickable(onClick = onClick), contentPadding = 12.dp) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            HelperBlob(
                color = if (away) Mesh.Ink3 else Mesh.accent(c.accent),
                mood = c.mood,
                size = 48.dp,
                progress = c.progress,
                description = "AI helper ${c.name} on ${h.machineLabel}, ${c.statusLine}",
            )
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                NameWithTag(c.name, isHelper = true)
                Text(
                    c.statusLine,
                    style = MaterialTheme.typography.bodyMedium,
                    color = if (c.mood == Mood.WAITING) Mesh.Amber else Mesh.Ink2,
                    maxLines = 1,
                )
            }
            c.progress?.let {
                Text("${(it * 100).toInt()}%", style = MaterialTheme.typography.titleSmall, color = Mesh.Indigo)
                Spacer(Modifier.width(6.dp))
            }
            Text(
                h.tag(withNetwork),
                style = MaterialTheme.typography.labelSmall,
                color = if (withNetwork) Mesh.Indigo else Mesh.Ink2,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier
                    .widthIn(max = 130.dp)
                    .clip(RoundedCornerShape(8.dp))
                    .background(if (withNetwork) Mesh.IndigoTint else Mesh.Paper)
                    .padding(horizontal = 7.dp, vertical = 3.dp),
            )
        }
    }
}

/** A machine whose address or key no longer works, with its fix. */
@Composable
private fun TroubleCard(st: MachineStatus, onPairAgain: () -> Unit, onRemove: () -> Unit) {
    val lost = st.health == MachineHealth.ADDRESS_LOST
    MeshCard(Modifier.fillMaxWidth(), color = if (lost) Mesh.AmberTint else Mesh.RoseTint) {
        Text(
            stringResource(if (lost) R.string.machine_lost_title else R.string.machine_key_title, st.name),
            style = MaterialTheme.typography.titleSmall,
            color = Mesh.Ink,
        )
        Text(
            stringResource(if (lost) R.string.machine_lost_body else R.string.machine_key_body),
            style = MaterialTheme.typography.bodyMedium,
            color = Mesh.Ink2,
        )
        Gap(8.dp)
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            InkButton(stringResource(R.string.machine_pair_again), onClick = onPairAgain)
            OutlinedButton(onClick = onRemove) { Text(stringResource(R.string.machine_remove), color = Mesh.Ink) }
        }
    }
}
