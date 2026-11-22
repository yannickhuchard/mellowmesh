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
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.mellowmesh.app.R
import io.mellowmesh.app.meshApp
import io.mellowmesh.app.ui.components.BackHeader
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.InkButton
import io.mellowmesh.app.ui.components.MachineDot
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.network.ToggleRow
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.EmailCheck
import io.mellowmesh.core.EmailPatch
import io.mellowmesh.core.EmailPersonChange
import io.mellowmesh.core.EmailServer
import io.mellowmesh.core.EmailView
import io.mellowmesh.core.MeshException
import io.mellowmesh.core.looksLikeEmail
import kotlinx.coroutines.launch

/** `tls` for the implicit-TLS ports, `starttls` for the plain ones. */
private fun securityFor(port: Int): String = if (port == 143 || port == 587 || port == 25) "starttls" else "tls"

/**
 * Email: the hub's own mailbox. The owner gives it an address and an app
 * password (servers are filled in from the address), checks it, and
 * chooses who may give commands by email and what the hub emails them.
 */
@Composable
fun EmailScreen(onBack: () -> Unit) {
    val app = LocalContext.current.meshApp
    val scope = rememberCoroutineScope()
    val machines by app.repo.machines.collectAsStateWithLifecycle()
    val whoami by app.repo.whoami.collectAsStateWithLifecycle()
    val here = machines.firstOrNull { it.active }
    var view by remember { mutableStateOf<EmailView?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var editing by remember { mutableStateOf(false) }
    var check by remember { mutableStateOf<EmailCheck?>(null) }
    var adding by remember { mutableStateOf(false) }
    val cannotReach = stringResource(R.string.mail_error_reach)
    val ownerOnly = stringResource(R.string.mail_error_owner)
    val oldHub = stringResource(R.string.mail_error_old_hub)

    fun explain(e: Throwable): String = when {
        e is MeshException && e.kind == MeshException.Kind.NETWORK -> cannotReach
        e is MeshException && (e.status == 401 || e.status == 403) -> ownerOnly
        e is MeshException && e.status == 404 -> oldHub
        else -> e.message ?: cannotReach
    }

    LaunchedEffect(here?.machine?.id) {
        view = null
        error = null
        check = null
        runCatching { app.repo.email() }
            .onSuccess { view = it }
            .onFailure { error = explain(it) }
    }

    fun act(block: suspend () -> EmailView) {
        if (busy) return
        busy = true
        error = null
        scope.launch {
            runCatching { block() }
                .onSuccess { view = it }
                .onFailure { error = explain(it) }
            busy = false
        }
    }

    Column(Modifier.fillMaxSize().background(Mesh.Paper)) {
        BackHeader(title = stringResource(R.string.email_title), onBack = onBack)
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp),
        ) {
            val v = view
            here?.let {
                Row(
                    Modifier
                        .clip(RoundedCornerShape(18.dp))
                        .background(Mesh.Card)
                        .padding(horizontal = 12.dp, vertical = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    MachineDot(it.reachable)
                    Spacer(Modifier.width(6.dp))
                    val on = v?.settings?.enabled == true
                    Text(
                        stringResource(R.string.mail_on_machine, it.name) + " · " +
                            if (on) stringResource(R.string.email_reading, v!!.settings.pollSeconds) else stringResource(R.string.email_off),
                        style = MaterialTheme.typography.labelLarge,
                        color = Mesh.Ink,
                    )
                }
                Gap(10.dp)
            }

            when {
                v == null && error == null -> Text(stringResource(R.string.loading), color = Mesh.Ink2)
                v == null -> Text(error.orEmpty(), color = Mesh.Rose, style = MaterialTheme.typography.bodyMedium)
                !v.configured || editing -> SetupCard(
                    view = v,
                    busy = busy,
                    onCancel = if (v.configured) ({ editing = false }) else null,
                    onSave = { patch ->
                        act {
                            val next = app.repo.setEmail(patch)
                            editing = false
                            check = null
                            next
                        }
                    },
                )
                else -> MailboxCard(
                    view = v,
                    busy = busy,
                    check = check,
                    onCheck = {
                        if (!busy) {
                            busy = true
                            error = null
                            check = null
                            scope.launch {
                                runCatching { app.repo.checkEmail() }
                                    .onSuccess { check = it }
                                    .onFailure { error = explain(it) }
                                runCatching { app.repo.email() }.onSuccess { view = it }
                                busy = false
                            }
                        }
                    },
                    onChange = { editing = true },
                )
            }
            error?.takeIf { v != null }?.let {
                Text(it, color = Mesh.Rose, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(top = 6.dp))
            }

            if (v != null) {
                Gap(10.dp)
                MeshCard(Modifier.fillMaxWidth(), contentPadding = 8.dp) {
                    ToggleRow(stringResource(R.string.email_on), v.settings.enabled, v.configured && !busy) { on ->
                        act { app.repo.setEmail(EmailPatch(enabled = on)) }
                    }
                    Text(
                        stringResource(if (v.configured) R.string.email_on_body else R.string.email_needs_setup),
                        style = MaterialTheme.typography.bodySmall,
                        color = Mesh.Ink2,
                        modifier = Modifier.padding(horizontal = 8.dp),
                    )
                }

                Label(stringResource(R.string.email_who))
                MeshCard(Modifier.fillMaxWidth(), contentPadding = 4.dp) {
                    if (v.people.isEmpty()) {
                        Text(stringResource(R.string.email_nobody), style = MaterialTheme.typography.bodyMedium, color = Mesh.Ink2, modifier = Modifier.padding(12.dp))
                    }
                    v.people.forEachIndexed { i, p ->
                        Row(Modifier.fillMaxWidth().padding(start = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                            Column(Modifier.weight(1f).padding(vertical = 10.dp)) {
                                Text(p.address, style = MaterialTheme.typography.titleSmall, color = Mesh.Ink)
                                Text(
                                    if (p.person == whoami?.principal) stringResource(R.string.email_you) else p.person,
                                    style = MaterialTheme.typography.bodySmall,
                                    color = Mesh.Ink2,
                                )
                            }
                            TextButton(enabled = !busy, onClick = {
                                act { app.repo.setEmailPerson(EmailPersonChange(address = p.address, remove = true)) }
                            }) { Text(stringResource(R.string.email_remove), color = Mesh.Ink2) }
                        }
                        if (i < v.people.lastIndex) HorizontalDivider(color = Mesh.Hairline)
                    }
                    HorizontalDivider(color = Mesh.Hairline)
                    Text(
                        stringResource(R.string.email_add),
                        style = MaterialTheme.typography.titleSmall,
                        color = Mesh.Indigo,
                        modifier = Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(14.dp))
                            .clickable(enabled = !busy) { adding = true }
                            .padding(12.dp),
                    )
                }
                Text(
                    stringResource(R.string.email_proof),
                    style = MaterialTheme.typography.bodySmall,
                    color = Mesh.Ink2,
                    modifier = Modifier.padding(horizontal = 4.dp, vertical = 6.dp),
                )

                Label(stringResource(R.string.email_notify))
                MeshCard(Modifier.fillMaxWidth(), contentPadding = 8.dp) {
                    ToggleRow(stringResource(R.string.email_decisions), v.settings.notifyDecisions, !busy) { on ->
                        act { app.repo.setEmail(EmailPatch(notifyDecisions = on)) }
                    }
                    Text(stringResource(R.string.email_decisions_body), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2, modifier = Modifier.padding(horizontal = 8.dp))
                    Gap(6.dp)
                    ToggleRow(stringResource(R.string.email_mentions), v.settings.notifyMentions, !busy) { on ->
                        act { app.repo.setEmail(EmailPatch(notifyMentions = on)) }
                    }
                    Text(stringResource(R.string.email_mentions_body), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2, modifier = Modifier.padding(horizontal = 8.dp))
                }

                Gap(12.dp)
                MeshCard(Modifier.fillMaxWidth(), color = Mesh.AmberTint, contentPadding = 12.dp) {
                    Text(stringResource(R.string.email_commands_title), style = MaterialTheme.typography.titleSmall, color = Mesh.Ink)
                    Text(stringResource(R.string.email_commands), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink)
                }

                Gap(10.dp)
                val st = v.status
                Text(
                    stringResource(R.string.email_status_line, st.received.toInt(), st.ignored.toInt(), st.sent.toInt()),
                    style = MaterialTheme.typography.bodySmall,
                    color = Mesh.Ink3,
                )
                st.lastError?.let {
                    Text(stringResource(R.string.email_last_error, it), style = MaterialTheme.typography.bodySmall, color = Mesh.Rose)
                }
            }
            Gap(32.dp)
        }
    }

    if (adding) {
        AddAddressDialog(
            hubAddress = view?.settings?.address.orEmpty(),
            onDismiss = { adding = false },
            onAdd = { address ->
                adding = false
                act { app.repo.setEmailPerson(EmailPersonChange(address = address)) }
            },
        )
    }
}

@Composable
private fun Label(text: String) {
    Text(
        text.uppercase(),
        style = MaterialTheme.typography.labelMedium,
        color = Mesh.Ink3,
        modifier = Modifier.padding(top = 20.dp, bottom = 8.dp),
    )
}

@Composable
private fun InfoRow(label: String, value: String) {
    Row(Modifier.fillMaxWidth().padding(vertical = 3.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2, modifier = Modifier.weight(1f))
        Text(value, style = MaterialTheme.typography.bodyMedium, color = Mesh.Ink)
    }
}

/** The mailbox once set up: what it is, Check and Change. */
@Composable
private fun MailboxCard(view: EmailView, busy: Boolean, check: EmailCheck?, onCheck: () -> Unit, onChange: () -> Unit) {
    val s = view.settings
    MeshCard(Modifier.fillMaxWidth(), contentPadding = 14.dp) {
        Text(stringResource(R.string.email_box).uppercase(), style = MaterialTheme.typography.labelMedium, color = Mesh.Ink3)
        Gap(6.dp)
        InfoRow(stringResource(R.string.email_address), s.address)
        InfoRow(stringResource(R.string.email_servers), "${s.imap.host} · ${s.smtp.host}")
        InfoRow(
            stringResource(R.string.email_password),
            stringResource(if (view.passwordFromEnv) R.string.email_password_env else R.string.email_password_saved),
        )
        Gap(10.dp)
        Row {
            InkButton(
                text = stringResource(if (busy && check == null) R.string.email_checking else R.string.email_check),
                onClick = onCheck,
                enabled = !busy,
                modifier = Modifier.weight(1f),
            )
            Spacer(Modifier.width(8.dp))
            TextButton(onClick = onChange, enabled = !busy, modifier = Modifier.weight(1f).padding(top = 4.dp)) {
                Text(stringResource(R.string.email_change), color = Mesh.Indigo)
            }
        }
        check?.let { c ->
            Gap(8.dp)
            Step(stringResource(R.string.email_step_inbox), c.inbox.ok, c.inbox.detail)
            Step(stringResource(R.string.email_step_send), c.send.ok, c.send.detail)
        }
    }
}

@Composable
private fun Step(label: String, ok: Boolean, detail: String) {
    Row(Modifier.padding(vertical = 2.dp)) {
        Text(if (ok) "✓" else "✗", color = if (ok) Mesh.Green else Mesh.Rose, style = MaterialTheme.typography.bodyMedium)
        Spacer(Modifier.width(8.dp))
        Column {
            Text(label, style = MaterialTheme.typography.bodyMedium, color = Mesh.Ink)
            if (detail.isNotBlank()) Text(detail, style = MaterialTheme.typography.bodySmall, color = if (ok) Mesh.Ink2 else Mesh.Rose)
        }
    }
}

/** First setup, or a change: address, app password, your address, and (advanced) servers. */
@Composable
private fun SetupCard(view: EmailView, busy: Boolean, onCancel: (() -> Unit)?, onSave: (EmailPatch) -> Unit) {
    val s = view.settings
    var address by remember { mutableStateOf(s.address) }
    var password by remember { mutableStateOf("") }
    val hasMe = view.people.isNotEmpty()
    var me by remember { mutableStateOf("") }
    var advanced by remember { mutableStateOf(false) }
    var login by remember { mutableStateOf(s.username) }
    var imapHost by remember { mutableStateOf(s.imap.host) }
    var imapPort by remember { mutableStateOf(s.imap.port.takeIf { it > 0 }?.toString() ?: "993") }
    var smtpHost by remember { mutableStateOf(s.smtp.host) }
    var smtpPort by remember { mutableStateOf(s.smtp.port.takeIf { it > 0 }?.toString() ?: "465") }
    val addressOk = looksLikeEmail(address)
    val meOk = hasMe || (looksLikeEmail(me) && !me.trim().equals(address.trim(), ignoreCase = true))
    val passwordOk = password.isNotEmpty() || view.passwordSet
    val portsOk = !advanced || (imapPort.toIntOrNull() in 1..65535 && smtpPort.toIntOrNull() in 1..65535)

    MeshCard(Modifier.fillMaxWidth(), contentPadding = 14.dp) {
        Text(stringResource(R.string.email_intro), style = MaterialTheme.typography.bodyMedium, color = Mesh.Ink2)
        Gap(10.dp)
        OutlinedTextField(
            value = address,
            onValueChange = { address = it.trim() },
            label = { Text(stringResource(R.string.email_address)) },
            singleLine = true,
            isError = address.isNotEmpty() && !addressOk,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
            modifier = Modifier.fillMaxWidth(),
        )
        Gap(8.dp)
        OutlinedTextField(
            value = password,
            onValueChange = { password = it },
            label = { Text(stringResource(R.string.email_password)) },
            placeholder = if (view.passwordSet) ({ Text(stringResource(R.string.email_password_saved)) }) else null,
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
            supportingText = { Text(stringResource(R.string.email_password_hint)) },
            modifier = Modifier.fillMaxWidth(),
        )
        if (!hasMe) {
            Gap(8.dp)
            OutlinedTextField(
                value = me,
                onValueChange = { me = it.trim() },
                label = { Text(stringResource(R.string.email_me)) },
                singleLine = true,
                isError = me.isNotEmpty() && !meOk,
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
                supportingText = {
                    Text(stringResource(if (me.isNotEmpty() && me.equals(address, ignoreCase = true)) R.string.email_same_address else R.string.email_me_hint))
                },
                modifier = Modifier.fillMaxWidth(),
            )
        }
        Gap(4.dp)
        TextButton(onClick = { advanced = !advanced }) {
            Text((if (advanced) "▾ " else "▸ ") + stringResource(R.string.email_advanced), color = Mesh.Indigo)
        }
        if (advanced) {
            OutlinedTextField(
                value = login,
                onValueChange = { login = it.trim() },
                label = { Text(stringResource(R.string.email_login)) },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            Gap(6.dp)
            Row {
                OutlinedTextField(
                    value = imapHost,
                    onValueChange = { imapHost = it.trim() },
                    label = { Text(stringResource(R.string.email_imap)) },
                    singleLine = true,
                    modifier = Modifier.weight(1f),
                )
                Spacer(Modifier.width(8.dp))
                OutlinedTextField(
                    value = imapPort,
                    onValueChange = { imapPort = it.filter(Char::isDigit).take(5) },
                    label = { Text(stringResource(R.string.email_port)) },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.width(96.dp),
                )
            }
            Gap(6.dp)
            Row {
                OutlinedTextField(
                    value = smtpHost,
                    onValueChange = { smtpHost = it.trim() },
                    label = { Text(stringResource(R.string.email_smtp)) },
                    singleLine = true,
                    modifier = Modifier.weight(1f),
                )
                Spacer(Modifier.width(8.dp))
                OutlinedTextField(
                    value = smtpPort,
                    onValueChange = { smtpPort = it.filter(Char::isDigit).take(5) },
                    label = { Text(stringResource(R.string.email_port)) },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                    modifier = Modifier.width(96.dp),
                )
            }
        } else if (s.imap.host.isNotBlank() && address.equals(s.address, ignoreCase = true)) {
            Text(
                stringResource(R.string.email_servers_auto, "${s.imap.host} · ${s.smtp.host}"),
                style = MaterialTheme.typography.bodySmall,
                color = Mesh.Ink3,
                modifier = Modifier.padding(horizontal = 4.dp),
            )
        }
        Gap(12.dp)
        Row(verticalAlignment = Alignment.CenterVertically) {
            InkButton(
                text = stringResource(R.string.email_save),
                enabled = !busy && addressOk && meOk && passwordOk && portsOk,
                modifier = Modifier.weight(1f),
                onClick = {
                    val changedAddress = !address.equals(s.address, ignoreCase = true)
                    // Servers typed by hand win; otherwise the hub fills them in
                    // from a new address.
                    val imap = if (advanced && imapHost.isNotBlank()) {
                        val p = imapPort.toInt()
                        EmailServer(imapHost, p, securityFor(p))
                    } else {
                        null
                    }
                    val smtp = if (advanced && smtpHost.isNotBlank()) {
                        val p = smtpPort.toInt()
                        EmailServer(smtpHost, p, securityFor(p))
                    } else {
                        null
                    }
                    onSave(
                        EmailPatch(
                            address = address.takeIf { changedAddress || s.address.isBlank() },
                            password = password.takeIf { it.isNotEmpty() },
                            ownerAddress = me.takeIf { !hasMe && it.isNotBlank() },
                            username = login.takeIf { advanced && it != s.username },
                            imap = imap,
                            smtp = smtp,
                        ),
                    )
                },
            )
            onCancel?.let {
                Spacer(Modifier.width(8.dp))
                TextButton(onClick = it) { Text(stringResource(R.string.cancel)) }
            }
        }
    }
}

@Composable
private fun AddAddressDialog(hubAddress: String, onDismiss: () -> Unit, onAdd: (String) -> Unit) {
    var address by remember { mutableStateOf("") }
    val ok = looksLikeEmail(address) && !address.equals(hubAddress, ignoreCase = true)
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.email_add_title)) },
        text = {
            Column {
                Text(stringResource(R.string.email_add_body), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2)
                Gap(8.dp)
                OutlinedTextField(
                    value = address,
                    onValueChange = { address = it.trim() },
                    singleLine = true,
                    isError = address.isNotEmpty() && !ok,
                    supportingText = { if (address.isNotEmpty() && !ok) Text(stringResource(R.string.email_bad_address)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Email),
                    modifier = Modifier.fillMaxWidth(),
                )
            }
        },
        confirmButton = { TextButton(enabled = ok, onClick = { onAdd(address) }) { Text(stringResource(R.string.save)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}
