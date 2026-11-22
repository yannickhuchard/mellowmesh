package io.mellowmesh.app.ui.network

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.mellowmesh.app.R
import io.mellowmesh.app.meshApp
import io.mellowmesh.app.ui.chats.SectionLabel
import io.mellowmesh.app.ui.chats.segmentColor
import io.mellowmesh.app.ui.components.BackHeader
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.HopIn
import io.mellowmesh.app.ui.components.MachineDot
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.components.Mello
import io.mellowmesh.app.ui.components.Pill
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.ChatSpace
import io.mellowmesh.core.MachineHealth
import io.mellowmesh.core.MachineSummary
import io.mellowmesh.core.Mood
import io.mellowmesh.core.NetworkSegment

/**
 * Your networks: one virtual Mellow network over all of them, and for each
 * network its own door and its own administration. Every action names the
 * network it changes; administration is offered only where this phone
 * holds the owner's key.
 */
@Composable
fun NetworksScreen(
    onBack: () -> Unit,
    onManage: (machineId: String) -> Unit,
    onInviteAi: (machineId: String) -> Unit,
    onAddMachine: () -> Unit,
    onShops: () -> Unit,
) {
    val app = LocalContext.current.meshApp
    val everywhere by app.repo.everywhere.collectAsStateWithLifecycle()
    val ownerOn by app.repo.ownerOn.collectAsStateWithLifecycle()
    LaunchedEffect(Unit) { app.repo.refreshEverywhere() }

    Column(Modifier.fillMaxSize().background(Mesh.Paper)) {
        BackHeader(title = stringResource(R.string.networks_title), onBack = onBack, subtitle = stringResource(R.string.networks_intro))
        val ew = everywhere
        if (ew == null) {
            Column(Modifier.fillMaxSize().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Gap(40.dp)
                Mello(size = 100.dp, mood = Mood.THINKING)
                Gap()
                Text(stringResource(R.string.everywhere_loading), color = Mesh.Ink2)
            }
            return@Column
        }
        val segments = ChatSpace.build(ew, emptyList(), emptyList()).segments
        val machinesOf = ew.machines.filter { it.sameAs == null }.groupBy { ChatSpace.segmentKey(it.networkName, it.label) }
        LazyColumn(
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            itemsIndexed(segments, key = { _, s -> s.key }) { i, s ->
                HopIn(order = i) {
                    SegmentCard(
                        s,
                        machinesOf[s.key].orEmpty(),
                        ownerOn,
                        onManage = onManage,
                        onInviteAi = onInviteAi,
                    )
                }
            }
            item(key = "doors") {
                Gap(4.dp)
                SectionLabel(stringResource(R.string.networks_doors))
            }
            item(key = "door-pair") {
                Door(stringResource(R.string.networks_door_pair), stringResource(R.string.networks_door_pair_body), onAddMachine)
            }
            item(key = "door-join") {
                Door(stringResource(R.string.networks_door_join), stringResource(R.string.networks_door_join_body), onAddMachine)
            }
            item(key = "door-mellow") {
                Door(stringResource(R.string.networks_door_mellow), stringResource(R.string.networks_door_mellow_body), onShops)
            }
        }
    }
}

@Composable
private fun SegmentCard(
    s: NetworkSegment,
    machines: List<MachineSummary>,
    ownerOn: Map<String, Boolean>,
    onManage: (String) -> Unit,
    onInviteAi: (String) -> Unit,
) {
    val color = segmentColor(s.slot)
    // Administer through a machine of this network where this phone is the owner, answering first.
    val adminVia = machines
        .filter { ownerOn[it.machineId] == true }
        .sortedBy { if (it.health == MachineHealth.ANSWERS) 0 else 1 }
        .firstOrNull()
    MeshCard(Modifier.fillMaxWidth(), contentPadding = 0.dp) {
        Row(Modifier.height(androidx.compose.foundation.layout.IntrinsicSize.Min)) {
            Box(Modifier.width(6.dp).fillMaxHeight().background(if (s.live) color else Mesh.Ink3))
            Column(Modifier.padding(14.dp).weight(1f)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(s.name, style = MaterialTheme.typography.titleMedium, color = Mesh.Ink, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                    Pill(
                        stringResource(if (adminVia != null) R.string.networks_you_own else R.string.networks_you_member),
                        if (adminVia != null) Mesh.IndigoTint else Mesh.Paper,
                        Mesh.Ink,
                    )
                }
                Text(
                    stringResource(R.string.networks_counts, s.mellows, machines.size),
                    style = MaterialTheme.typography.bodySmall,
                    color = Mesh.Ink2,
                )
                Gap(6.dp)
                Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    for (m in machines) {
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            MachineDot(m.health)
                            Spacer(Modifier.width(4.dp))
                            Text(m.label, style = MaterialTheme.typography.labelMedium, color = Mesh.Ink2, maxLines = 1)
                        }
                    }
                }
                Gap(4.dp)
                if (adminVia != null) {
                    Row {
                        TextButton(onClick = { onInviteAi(adminVia.machineId) }) {
                            Text(stringResource(R.string.networks_invite_ai_to, s.name), color = color, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    }
                    TextButton(onClick = { onManage(adminVia.machineId) }) {
                        Text(stringResource(R.string.networks_manage, s.name), color = Mesh.Ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                } else {
                    Text(stringResource(R.string.networks_member_note), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink3)
                }
            }
        }
    }
}

@Composable
private fun Door(title: String, body: String, onClick: () -> Unit) {
    MeshCard(Modifier.fillMaxWidth().clickable(onClick = onClick), contentPadding = 14.dp) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(36.dp).clip(RoundedCornerShape(12.dp)).background(Mesh.Paper), contentAlignment = Alignment.Center) {
                Text("+", style = MaterialTheme.typography.titleLarge, color = Mesh.Ink2)
            }
            Spacer(Modifier.width(12.dp))
            Column(Modifier.weight(1f)) {
                Text(title, style = MaterialTheme.typography.titleSmall, color = Mesh.Ink)
                Text(body, style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2)
            }
        }
    }
}
