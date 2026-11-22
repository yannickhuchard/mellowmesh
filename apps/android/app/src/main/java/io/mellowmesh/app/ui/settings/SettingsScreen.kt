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
fun SettingsScreen(onBack: () -> Unit, onAddMachine: () -> Unit, onMail: () -> Unit, onEmail: () -> Unit, onInviteAi: () -> Unit) {
    val app = LocalContext.current.meshApp
    val config by app.repo.config.collectAsStateWithLifecycle()
    val whoami by app.repo.whoami.collectAsStateWithLifecycle()
    var ladder by remember { mutableStateOf(app.store.ladder) }
    var askAlarm by remember { mutableStateOf(false) }
    var askRemove by remember { mutableStateOf<io.mellowmesh.app.data.MachineStatus?>(null) }
    val machines by app.repo.machines.collectAsStateWithLifecycle()

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

            SectionTitle(stringResource(R.string.machines_title))
            Text(
                stringResource(
                    R.string.machines_summary,
                    config?.let { app.store.networkName() } ?: stringResource(R.string.your_network),
                    machines.size,
                ),
                style = MaterialTheme.typography.bodyMedium,
                color = Mesh.Ink2,
            )
            Gap(8.dp)
            MeshCard(Modifier.fillMaxWidth()) {
                machines.forEachIndexed { i, m ->
                    MachineRow(
                        status = m,
                        onUse = { app.repo.switchMachine(m.machine.id) },
                        onRemove = { askRemove = m },
                    )
                    if (i < machines.lastIndex) {
                        androidx.compose.material3.HorizontalDivider(color = Mesh.Hairline)
                    }
                }
            }
            Gap(10.dp)
            io.mellowmesh.app.ui.components.InkButton(
                stringResource(R.string.add_machine),
                onClick = onAddMachine,
                modifier = Modifier.fillMaxWidth(),
            )
            Text(
                stringResource(R.string.add_machine_hint),
                style = MaterialTheme.typography.bodySmall,
                color = Mesh.Ink3,
                modifier = Modifier.padding(top = 6.dp),
            )

            SectionTitle(stringResource(R.string.invite_title))
            MeshCard(Modifier.fillMaxWidth(), contentPadding = 4.dp) {
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(18.dp))
                        .clickable(onClick = onInviteAi)
                        .padding(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text(stringResource(R.string.invite_row_title), style = MaterialTheme.typography.titleSmall, color = Mesh.Ink)
                        Text(
                            stringResource(if (whoami?.isOwner == false) R.string.mail_settings_owner_only else R.string.invite_row),
                            style = MaterialTheme.typography.bodySmall,
                            color = Mesh.Ink2,
                        )
                    }
                    Text("›", style = MaterialTheme.typography.titleLarge, color = Mesh.Ink3)
                }
            }

            SectionTitle(stringResource(R.string.mail_title))
            MeshCard(Modifier.fillMaxWidth(), contentPadding = 4.dp) {
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(18.dp))
                        .clickable(onClick = onMail)
                        .padding(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text(stringResource(R.string.mail_question), style = MaterialTheme.typography.titleSmall, color = Mesh.Ink)
                        Text(
                            stringResource(if (whoami?.isOwner == false) R.string.mail_settings_owner_only else R.string.mail_settings_row),
                            style = MaterialTheme.typography.bodySmall,
                            color = Mesh.Ink2,
                        )
                    }
                    Text("›", style = MaterialTheme.typography.titleLarge, color = Mesh.Ink3)
                }
            }

            SectionTitle(stringResource(R.string.email_title))
            MeshCard(Modifier.fillMaxWidth(), contentPadding = 4.dp) {
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(18.dp))
                        .clickable(onClick = onEmail)
                        .padding(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text(stringResource(R.string.email_settings_title), style = MaterialTheme.typography.titleSmall, color = Mesh.Ink)
                        Text(
                            stringResource(if (whoami?.isOwner == false) R.string.mail_settings_owner_only else R.string.email_settings_row),
                            style = MaterialTheme.typography.bodySmall,
                            color = Mesh.Ink2,
                        )
                    }
                    Text("›", style = MaterialTheme.typography.titleLarge, color = Mesh.Ink3)
                }
            }

            SectionTitle(stringResource(R.string.connection))
            MeshCard(Modifier.fillMaxWidth()) {
                val url = config?.baseUrl ?: ""
                InfoLine(stringResource(R.string.hub_in_use), url)
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

    askRemove?.let { m ->
        val last = machines.size <= 1
        AlertDialog(
            onDismissRequest = { askRemove = null },
            title = { Text(stringResource(R.string.machine_remove_title, m.machine.label)) },
            text = {
                Text(
                    if (last) stringResource(R.string.disconnect_confirm)
                    else stringResource(R.string.machine_remove_confirm, m.machine.label),
                )
            },
            confirmButton = {
                TextButton(onClick = {
                    askRemove = null
                    app.repo.removeMachine(m.machine.id)
                }) { Text(stringResource(R.string.machine_remove), color = Mesh.Rose) }
            },
            dismissButton = { TextButton(onClick = { askRemove = null }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

/** One paired machine: state, name, where it was reached, and its actions. */
@Composable
private fun MachineRow(status: io.mellowmesh.app.data.MachineStatus, onUse: () -> Unit, onRemove: () -> Unit) {
    val m = status.machine
    val stateText = when (status.reachable) {
        true -> stringResource(R.string.machine_answers)
        false -> status.lastAnswerMs?.let {
            stringResource(
                R.string.machine_resting_since,
                android.text.format.DateUtils.getRelativeTimeSpanString(it).toString(),
            )
        } ?: stringResource(R.string.machine_resting)
        null -> stringResource(R.string.machine_unknown)
    }
    androidx.compose.foundation.layout.Row(
        Modifier.fillMaxWidth().padding(vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        io.mellowmesh.app.ui.components.MachineDot(status.reachable)
        androidx.compose.foundation.layout.Spacer(Modifier.padding(start = 8.dp))
        Column(Modifier.weight(1f)) {
            androidx.compose.foundation.layout.Row(verticalAlignment = Alignment.CenterVertically) {
                Text(m.label, style = MaterialTheme.typography.titleSmall, color = if (status.reachable == false) Mesh.Ink2 else Mesh.Ink)
                if (status.active) {
                    Text(
                        stringResource(R.string.machine_here),
                        style = MaterialTheme.typography.labelSmall,
                        color = Mesh.Indigo,
                        modifier = Modifier
                            .padding(start = 6.dp)
                            .background(Mesh.IndigoTint, androidx.compose.foundation.shape.RoundedCornerShape(8.dp))
                            .padding(horizontal = 6.dp, vertical = 2.dp),
                    )
                }
            }
            Text(
                stateText + " · " + stringResource(R.string.machine_mellows, status.agents.size),
                style = MaterialTheme.typography.bodySmall,
                color = Mesh.Ink2,
            )
            Text(m.baseUrl, style = MaterialTheme.typography.bodySmall, color = Mesh.Ink3, maxLines = 1)
        }
        if (!status.active) {
            TextButton(onClick = onUse) { Text(stringResource(R.string.machine_use_short), color = Mesh.Ink) }
        }
        TextButton(onClick = onRemove) { Text(stringResource(R.string.machine_remove), color = Mesh.Rose) }
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
