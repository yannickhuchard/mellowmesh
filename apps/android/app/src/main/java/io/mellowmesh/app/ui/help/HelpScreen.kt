package io.mellowmesh.app.ui.help

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import io.mellowmesh.app.R
import io.mellowmesh.app.ui.components.BackHeader
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.HelperBlob
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.components.Mello
import io.mellowmesh.app.ui.components.PersonAvatar
import io.mellowmesh.app.ui.components.SectionTitle
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.app.ui.welcome.TourPager
import io.mellowmesh.core.Mood

private data class Learn(val title: Int, val body: Int)

private val LearnItems = listOf(
    Learn(R.string.learn_helper_t, R.string.learn_helper_b),
    Learn(R.string.learn_request_t, R.string.learn_request_b),
    Learn(R.string.learn_room_t, R.string.learn_room_b),
    Learn(R.string.learn_space_t, R.string.learn_space_b),
    Learn(R.string.learn_team_t, R.string.learn_team_b),
    Learn(R.string.learn_task_t, R.string.learn_task_b),
)

/** The Help desk: Mello, the tour again, and "Learn" explanations in plain words. */
@Composable
fun HelpScreen(onBack: () -> Unit) {
    var touring by remember { mutableStateOf(false) }
    var open by remember { mutableStateOf<Int?>(null) }

    if (touring) {
        Column(Modifier.fillMaxSize().background(Mesh.Paper).navigationBarsPadding()) {
            BackHeader(title = stringResource(R.string.help_title), onBack = { touring = false })
            TourPager(onDone = { touring = false }, modifier = Modifier.weight(1f))
        }
        return
    }

    Column(Modifier.fillMaxSize().background(Mesh.Paper)) {
        BackHeader(title = stringResource(R.string.help_title), onBack = onBack)
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Mello(size = 88.dp, mood = Mood.IDLE)
                Spacer(Modifier.width(12.dp))
                Text(stringResource(R.string.help_intro), style = MaterialTheme.typography.titleMedium, color = Mesh.Ink)
            }
            Gap()
            OutlinedButton(onClick = { touring = true }, modifier = Modifier.fillMaxWidth()) {
                Text(stringResource(R.string.replay_tour), color = Mesh.Ink)
            }

            SectionTitle(stringResource(R.string.learn))
            LearnItems.forEachIndexed { i, item ->
                MeshCard(
                    Modifier
                        .fillMaxWidth()
                        .padding(bottom = 8.dp)
                        .clickable { open = if (open == i) null else i },
                    contentPadding = 14.dp,
                ) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        if (i == 0) HelperBlob(Mesh.Indigo, Mood.IDLE, 32.dp) else Spacer(Modifier.width(0.dp))
                        if (i == 0) Spacer(Modifier.width(8.dp))
                        Text(stringResource(item.title), style = MaterialTheme.typography.titleSmall, color = Mesh.Ink)
                    }
                    if (open == i) {
                        Gap(6.dp)
                        Text(stringResource(item.body), style = MaterialTheme.typography.bodyMedium, color = Mesh.Ink2)
                        if (i == 0) {
                            Gap(8.dp)
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                HelperBlob(Mesh.Sky, Mood.DONE, 36.dp)
                                Spacer(Modifier.width(6.dp))
                                Text("= AI", color = Mesh.Ink2)
                                Spacer(Modifier.width(16.dp))
                                PersonAvatar("Alice Martin", 32.dp)
                                Spacer(Modifier.width(6.dp))
                                Text("= person", color = Mesh.Ink2)
                            }
                        }
                    }
                }
            }
            Gap(24.dp)
        }
    }
}
