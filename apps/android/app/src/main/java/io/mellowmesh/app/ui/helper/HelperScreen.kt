package io.mellowmesh.app.ui.helper

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.mellowmesh.app.R
import io.mellowmesh.app.meshApp
import io.mellowmesh.app.ui.components.AiTag
import io.mellowmesh.app.ui.components.BackHeader
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.HelperBlob
import io.mellowmesh.app.ui.components.InkButton
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.components.Pill
import io.mellowmesh.app.ui.components.SectionTitle
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.Conversations
import io.mellowmesh.core.Identity
import io.mellowmesh.core.Mood
import io.mellowmesh.core.Sanitize
import io.mellowmesh.core.Shops

/** A helper's own page: who it is, its mood, what it is good at, chat and tasks. */
@Composable
fun HelperScreen(agentId: String, onBack: () -> Unit, onChat: (String) -> Unit, onBoard: () -> Unit) {
    val app = LocalContext.current.meshApp
    val home by app.repo.home.collectAsStateWithLifecycle()
    val agents by app.repo.agents.collectAsStateWithLifecycle()
    val card = home?.helpers?.firstOrNull { Identity.samePrincipal(it.id, agentId) }
    val reg = agents.firstOrNull { Identity.samePrincipal(it.id, agentId) }
    val name = card?.name ?: reg?.name?.takeIf { it.isNotBlank() }?.let { Sanitize.line(it) } ?: Identity.shortName(agentId)
    val mood = card?.mood ?: Mood.IDLE
    val capabilities = card?.capabilities ?: reg?.capabilities.orEmpty()
    val owner = card?.owner ?: reg?.owner
    val conversationId = Conversations.agentConversationId(agentId)

    Column(Modifier.fillMaxSize().background(Mesh.Paper)) {
        BackHeader(title = name, onBack = onBack, actions = { AiTag(Modifier.padding(end = 12.dp)) })
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(20.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            HelperBlob(
                color = Mesh.accent(Identity.accentOf(agentId)),
                mood = mood,
                size = 160.dp,
                progress = card?.progress,
                description = "AI helper $name, ${card?.statusLine ?: ""}",
            )
            Gap()
            Text(name, style = MaterialTheme.typography.headlineLarge, color = Mesh.Ink)
            Text(card?.statusLine ?: "", style = MaterialTheme.typography.bodyLarge, color = if (mood == Mood.WAITING) Mesh.Amber else Mesh.Ink2)
            if (owner != null) {
                Text(stringResource(R.string.owned_by, Identity.shortName(owner)), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink3)
            }
            Gap(20.dp)
            InkButton(
                text = stringResource(R.string.chat_with),
                enabled = conversationId != null,
                onClick = { conversationId?.let(onChat) },
                modifier = Modifier.fillMaxWidth(),
            )
            Gap(8.dp)
            OutlinedButton(onClick = onBoard, modifier = Modifier.fillMaxWidth()) {
                Text(stringResource(R.string.tasks), color = Mesh.Ink)
            }
            if (capabilities.isNotEmpty()) {
                SectionTitle(stringResource(R.string.good_at), Modifier.fillMaxWidth())
                MeshCard(Modifier.fillMaxWidth()) {
                    Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                        capabilities.take(4).forEach { Pill(it, Mesh.IndigoTint, Mesh.Indigo) }
                    }
                    if (capabilities.size > 4) {
                        Gap(6.dp)
                        Text(capabilities.drop(4).joinToString(" · "), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2)
                    }
                }
            }
            if (reg != null) {
                Gap()
                for (example in Shops.trySaying(reg)) {
                    Text(stringResource(R.string.try_saying, example), style = MaterialTheme.typography.bodyMedium, color = Mesh.Indigo)
                }
            }
        }
    }
}
