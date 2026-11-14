package io.mellowmesh.app.ui.settings

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.RadioButton
import androidx.compose.material3.RadioButtonDefaults
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.mellowmesh.app.R
import io.mellowmesh.app.meshApp
import io.mellowmesh.app.ui.components.BackHeader
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.HelperBlob
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.components.SectionTitle
import io.mellowmesh.app.ui.network.ToggleRow
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.LadderSettings
import io.mellowmesh.core.Level
import io.mellowmesh.core.Mood
import io.mellowmesh.core.Pairing
import io.mellowmesh.core.UrlSafety

private fun hhmm(minute: Int): String = "%02d:%02d".format(minute / 60, minute % 60)

/** Moves a time of day by whole hours, wrapping around midnight. */
private fun shift(minute: Int, hours: Int): Int = Math.floorMod(minute + hours * 60, 24 * 60)

private fun moodFor(level: Level): Mood = when (level) {
    Level.GLANCE -> Mood.ASLEEP
    Level.WHISPER -> Mood.IDLE
    Level.TAP -> Mood.THINKING
    Level.KNOCK -> Mood.WAITING
    Level.ALARM -> Mood.WAITING
}

@Composable
fun SettingsScreen(onBack: () -> Unit) {
    val app = LocalContext.current.meshApp
    val config by app.repo.config.collectAsStateWithLifecycle()
    val whoami by app.repo.whoami.collectAsStateWithLifecycle()
    var ladder by remember { mutableStateOf(app.store.ladder) }
    var askAlarm by remember { mutableStateOf(false) }
    var askDisconnect by remember { mutableStateOf(false) }

    fun save(s: LadderSettings) {
        ladder = s
        app.store.ladder = s
    }

    Column(Modifier.fillMaxSize().background(Mesh.Paper)) {
        BackHeader(title = stringResource(R.string.settings_title), onBack = onBack)
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp),
        ) {
            Text(stringResource(R.string.how_loud), style = MaterialTheme.typography.headlineSmall, color = Mesh.Ink)
            Gap(4.dp)
            Text(stringResource(R.string.how_loud_body), style = MaterialTheme.typography.bodyMedium, color = Mesh.Ink2)
            Gap()
            MeshCard(Modifier.fillMaxWidth(), contentPadding = 8.dp) {
                for (level in Level.entries) {
                    val selected = ladder.maxLevel == level
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(14.dp))
                            .background(if (selected) Mesh.IndigoTint else Mesh.Card)
                            .clickable {
                                if (level == Level.ALARM && ladder.maxLevel != Level.ALARM) askAlarm = true
                                else save(ladder.copy(maxLevel = level))
                            }
                            .padding(10.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        HelperBlob(if (level == Level.ALARM) Mesh.Rose else Mesh.Indigo, moodFor(level), 40.dp)
                        Spacer(Modifier.width(10.dp))
                        Column(Modifier.weight(1f)) {
                            Text("${level.rank} · ${level.title}", style = MaterialTheme.typography.titleSmall, color = Mesh.Ink)
                            Text(level.description, style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2)
                        }
                        RadioButton(
                            selected = selected,
                            onClick = null,
                            colors = RadioButtonDefaults.colors(selectedColor = Mesh.Ink),
                        )
                    }
                }
            }

            SectionTitle(stringResource(R.string.quiet_hours))
            MeshCard(Modifier.fillMaxWidth()) {
                ToggleRow(stringResource(R.string.quiet_hours), ladder.quietHoursEnabled, true) {
                    save(ladder.copy(quietHoursEnabled = it))
                }
                Text(
                    stringResource(R.string.quiet_hours_body, hhmm(ladder.quietStartMinute), hhmm(ladder.quietEndMinute)),
                    style = MaterialTheme.typography.bodySmall,
                    color = Mesh.Ink2,
                )
                Gap(8.dp)
                TimeStepper(stringResource(R.string.from), ladder.quietStartMinute, ladder.quietHoursEnabled) {
                    save(ladder.copy(quietStartMinute = it))
                }
                TimeStepper(stringResource(R.string.until), ladder.quietEndMinute, ladder.quietHoursEnabled) {
                    save(ladder.copy(quietEndMinute = it))
                }
            }

            SectionTitle(stringResource(R.string.daily_digest))
            MeshCard(Modifier.fillMaxWidth()) {
                ToggleRow(stringResource(R.string.daily_digest), ladder.digestEnabled, true) {
                    save(ladder.copy(digestEnabled = it))
                }
                Text(
                    stringResource(R.string.daily_digest_body, hhmm(ladder.digestMinute)),
                    style = MaterialTheme.typography.bodySmall,
                    color = Mesh.Ink2,
                )
                TimeStepper("", ladder.digestMinute, ladder.digestEnabled) { save(ladder.copy(digestMinute = it)) }
            }

            SectionTitle(stringResource(R.string.connection))
            MeshCard(Modifier.fillMaxWidth()) {
                val url = config?.baseUrl ?: ""
                InfoLine(stringResource(R.string.hub), url)
                when (Pairing.safety(url)) {
                    UrlSafety.PUBLIC_PLAINTEXT -> Text(stringResource(R.string.warning_public_http), color = Mesh.Rose, style = MaterialTheme.typography.bodySmall)
                    UrlSafety.PRIVATE_LAN -> Text(stringResource(R.string.note_private_http), color = Mesh.Ink2, style = MaterialTheme.typography.bodySmall)
                    else -> Unit
                }
                Gap(6.dp)
                InfoLine(stringResource(R.string.signed_in_as), whoami?.principal ?: config?.principal ?: "—")
                InfoLine(stringResource(R.string.device), config?.deviceName ?: "")
                if (whoami?.isOwner == true) {
                    Gap(4.dp)
                    Text(stringResource(R.string.owner_yes), color = Mesh.Green, style = MaterialTheme.typography.labelLarge)
                }
                Gap()
                OutlinedButton(onClick = { askDisconnect = true }, modifier = Modifier.fillMaxWidth()) {
                    Text(stringResource(R.string.disconnect), color = Mesh.Rose)
                }
            }
            Gap(32.dp)
        }
    }

    if (askAlarm) {
        AlertDialog(
            onDismissRequest = { askAlarm = false },
            title = { Text(Level.ALARM.title) },
            text = { Text(stringResource(R.string.alarm_confirm)) },
            confirmButton = {
                TextButton(onClick = {
                    askAlarm = false
                    save(ladder.copy(maxLevel = Level.ALARM))
                }) { Text(stringResource(R.string.yes)) }
            },
            dismissButton = { TextButton(onClick = { askAlarm = false }) { Text(stringResource(R.string.no)) } },
        )
    }

    if (askDisconnect) {
        AlertDialog(
            onDismissRequest = { askDisconnect = false },
            title = { Text(stringResource(R.string.disconnect)) },
            text = { Text(stringResource(R.string.disconnect_confirm)) },
            confirmButton = {
                TextButton(onClick = {
                    askDisconnect = false
                    app.repo.disconnect()
                }) { Text(stringResource(R.string.disconnect), color = Mesh.Rose) }
            },
            dismissButton = { TextButton(onClick = { askDisconnect = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

@Composable
private fun InfoLine(label: String, value: String) {
    Column(Modifier.padding(vertical = 4.dp)) {
        Text(label, style = MaterialTheme.typography.labelMedium, color = Mesh.Ink3)
        Text(value, style = MaterialTheme.typography.bodyMedium, color = Mesh.Ink)
    }
}

/** A time of day with − / + one hour buttons (no experimental time picker). */
@Composable
private fun TimeStepper(label: String, minute: Int, enabled: Boolean, onChange: (Int) -> Unit) {
    Row(Modifier.fillMaxWidth().padding(vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, style = MaterialTheme.typography.bodyLarge, color = Mesh.Ink, modifier = Modifier.weight(1f))
        TextButton(onClick = { onChange(shift(minute, -1)) }, enabled = enabled) { Text("−", style = MaterialTheme.typography.titleLarge) }
        Text(hhmm(minute), style = MaterialTheme.typography.headlineSmall, color = if (enabled) Mesh.Ink else Mesh.Ink3)
        TextButton(onClick = { onChange(shift(minute, 1)) }, enabled = enabled) { Text("+", style = MaterialTheme.typography.titleLarge) }
    }
}
