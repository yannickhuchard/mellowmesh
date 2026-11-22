package io.mellowmesh.app.ui.settings

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CheckboxDefaults
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.RadioButtonDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
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
import io.mellowmesh.app.ui.components.MachineDot
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.network.ToggleRow
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.app.ui.theme.rememberReduceMotion
import io.mellowmesh.core.MailChange
import io.mellowmesh.core.MailMode
import io.mellowmesh.core.MailParty
import io.mellowmesh.core.MailPolicyView
import io.mellowmesh.core.MailRule
import io.mellowmesh.core.MeshException
import io.mellowmesh.core.Mood
import kotlinx.coroutines.launch

/** How a rule reads in one line ("Any helper's mailbox", "Coder only"…). */
@Composable
fun mailRuleLabel(rule: MailRule, view: MailPolicyView): String = when (rule.kind) {
    MailMode.ANY -> stringResource(R.string.mail_any)
    MailMode.NONE -> stringResource(R.string.mail_none)
    MailMode.ONLY ->
        if (rule.boxes.isEmpty()) stringResource(R.string.mail_only_empty)
        else stringResource(R.string.mail_only_list, rule.boxes.joinToString(", ") { view.nameOf(it) })
}

/**
 * Notes between AIs: who invited AIs (connected apps) can write to — any
 * helper's mailbox (default), only some, none — and a different rule for
 * one AI. Each change applies at once on the machine in use, and the
 * network rule can be applied on every machine in one go.
 */
@Composable
fun MailScreen(onBack: () -> Unit) {
    val app = LocalContext.current.meshApp
    val scope = rememberCoroutineScope()
    val machines by app.repo.machines.collectAsStateWithLifecycle()
    val here = machines.firstOrNull { it.active }
    var view by remember { mutableStateOf<MailPolicyView?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var notice by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    var editing by remember { mutableStateOf<MailParty?>(null) }
    var everywhere by rememberSaveable { mutableStateOf(true) }
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
        runCatching { app.repo.agentMail() }
            .onSuccess { view = it }
            .onFailure { error = explain(it) }
    }

    fun change(c: MailChange, spread: Boolean) {
        if (busy) return
        busy = true
        notice = null
        scope.launch {
            runCatching { app.repo.setAgentMail(c) }
                .onSuccess { v ->
                    view = v
                    if (spread && c.forAi == null && c.rule != null && c.rule!!.kind != MailMode.ONLY && machines.size > 1) {
                        val missed = app.repo.setNetworkMailEverywhere(c.rule!!)
                        notice = if (missed.isEmpty()) null else app.getString(R.string.mail_missed, missed.joinToString(", "))
                    }
                }
                .onFailure { error = explain(it) }
            busy = false
        }
    }

    Column(Modifier.fillMaxSize().background(Mesh.Paper)) {
        BackHeader(title = stringResource(R.string.mail_title), onBack = onBack)
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp),
        ) {
            here?.let {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Row(
                        Modifier
                            .clip(RoundedCornerShape(18.dp))
                            .background(Mesh.Card)
                            .padding(horizontal = 12.dp, vertical = 6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        MachineDot(it.reachable)
                        Spacer(Modifier.width(6.dp))
                        Text(stringResource(R.string.mail_on_machine, it.name), style = MaterialTheme.typography.labelLarge, color = Mesh.Ink)
                    }
                    Spacer(Modifier.width(8.dp))
                    Text(stringResource(R.string.mail_at_once), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2)
                }
                Gap(10.dp)
            }
            NoteFlight(open = view?.network?.kind != MailMode.NONE)
            Gap(12.dp)
            Text(stringResource(R.string.mail_question), style = MaterialTheme.typography.titleLarge, color = Mesh.Ink)
            Gap(8.dp)

            val v = view
            when {
                v == null && error == null -> Text(stringResource(R.string.loading), color = Mesh.Ink2)
                v == null -> Text(error.orEmpty(), color = Mesh.Rose, style = MaterialTheme.typography.bodyMedium)
                else -> {
                    RuleChoices(
                        rule = v.network,
                        mailboxes = v.mailboxes,
                        enabled = !busy,
                        includeFollow = false,
                        following = false,
                        onPick = { change(MailChange(rule = it), everywhere) },
                        onFollow = {},
                    )
                    Gap(8.dp)
                    Promise(stringResource(R.string.mail_promise_refuse))
                    Promise(stringResource(R.string.mail_promise_voice))
                    if (machines.size > 1) {
                        Gap(8.dp)
                        MeshCard(Modifier.fillMaxWidth(), contentPadding = 8.dp) {
                            ToggleRow(stringResource(R.string.mail_everywhere), everywhere, v.network.kind != MailMode.ONLY) { everywhere = it }
                            if (v.network.kind == MailMode.ONLY) {
                                Text(stringResource(R.string.mail_everywhere_only), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink3, modifier = Modifier.padding(horizontal = 8.dp))
                            }
                        }
                    }
                    error?.let { Text(it, color = Mesh.Rose, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(top = 6.dp)) }
                    notice?.let { Text(it, color = Mesh.Ink2, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(top = 6.dp)) }

                    Text(
                        stringResource(R.string.mail_each_ai).uppercase(),
                        style = MaterialTheme.typography.labelMedium,
                        color = Mesh.Ink3,
                        modifier = Modifier.padding(top = 22.dp, bottom = 8.dp),
                    )
                    MeshCard(Modifier.fillMaxWidth(), contentPadding = 4.dp) {
                        if (v.invited.isEmpty()) {
                            Text(stringResource(R.string.mail_no_invited), style = MaterialTheme.typography.bodyMedium, color = Mesh.Ink2, modifier = Modifier.padding(12.dp))
                        }
                        v.invited.forEachIndexed { i, ai ->
                            val (rule, own) = v.ruleFor(ai.id)
                            Row(
                                Modifier
                                    .fillMaxWidth()
                                    .clip(RoundedCornerShape(14.dp))
                                    .clickable(enabled = !busy) { editing = ai }
                                    .padding(horizontal = 12.dp, vertical = 11.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                Column(Modifier.weight(1f)) {
                                    Text(ai.name.ifBlank { ai.id }, style = MaterialTheme.typography.titleSmall, color = Mesh.Ink)
                                    Text(
                                        if (own) stringResource(R.string.mail_own_rule, mailRuleLabel(rule, v)) else stringResource(R.string.mail_network_rule),
                                        style = MaterialTheme.typography.bodySmall,
                                        color = if (own) Mesh.Indigo else Mesh.Ink2,
                                    )
                                }
                                Text("›", style = MaterialTheme.typography.titleLarge, color = Mesh.Ink3)
                            }
                            if (i < v.invited.lastIndex) HorizontalDivider(color = Mesh.Hairline)
                        }
                    }
                }
            }
            Gap(32.dp)
        }
    }

    val v = view
    editing?.let { ai ->
        if (v != null) {
            AiRuleDialog(
                ai = ai,
                view = v,
                onDismiss = { editing = null },
                onSave = { rule ->
                    editing = null
                    change(MailChange(forAi = ai.id, rule = rule), spread = false)
                },
            )
        }
    }
}

/** A line that never changes, whatever the rule. */
@Composable
private fun Promise(text: String) {
    Row(Modifier.padding(horizontal = 4.dp, vertical = 2.dp)) {
        Text("✓", color = Mesh.Green, style = MaterialTheme.typography.bodySmall)
        Spacer(Modifier.width(6.dp))
        Text(text, color = Mesh.Ink2, style = MaterialTheme.typography.bodySmall)
    }
}

/**
 * The three answers as large radio cards (plus "Follow the network rule"
 * for one AI). Picking "Only these" shows the helpers' mailboxes to tick.
 */
@Composable
private fun RuleChoices(
    rule: MailRule,
    mailboxes: List<MailParty>,
    enabled: Boolean,
    includeFollow: Boolean,
    following: Boolean,
    onPick: (MailRule) -> Unit,
    onFollow: () -> Unit,
) {
    if (includeFollow) {
        ChoiceCard(stringResource(R.string.mail_follow), null, following, enabled) { onFollow() }
        Gap(8.dp)
    }
    val kind = if (following) null else rule.kind
    ChoiceCard(stringResource(R.string.mail_any), stringResource(R.string.mail_any_body), kind == MailMode.ANY, enabled) {
        onPick(MailRule.of(MailMode.ANY))
    }
    Gap(8.dp)
    ChoiceCard(stringResource(R.string.mail_only), stringResource(R.string.mail_only_body), kind == MailMode.ONLY, enabled, onClick = {
        onPick(MailRule.of(MailMode.ONLY, if (rule.kind == MailMode.ONLY) rule.boxes else emptyList()))
    }, extra = {
        if (kind == MailMode.ONLY) {
            if (mailboxes.isEmpty()) {
                Text(stringResource(R.string.mail_no_helpers), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2, modifier = Modifier.padding(start = 40.dp, top = 6.dp))
            }
            for (box in mailboxes) {
                val ticked = box.id in rule.boxes
                Row(
                    Modifier
                        .fillMaxWidth()
                        .padding(start = 28.dp)
                        .clip(RoundedCornerShape(10.dp))
                        .clickable(enabled = enabled) {
                            val next = if (ticked) rule.boxes - box.id else rule.boxes + box.id
                            onPick(MailRule.of(MailMode.ONLY, next))
                        },
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Checkbox(checked = ticked, onCheckedChange = null, enabled = enabled, colors = CheckboxDefaults.colors(checkedColor = Mesh.Indigo), modifier = Modifier.padding(10.dp))
                    Text(box.name.ifBlank { box.id }, style = MaterialTheme.typography.bodyLarge, color = Mesh.Ink)
                }
            }
        }
    })
    Gap(8.dp)
    ChoiceCard(stringResource(R.string.mail_none), stringResource(R.string.mail_none_body), kind == MailMode.NONE, enabled) {
        onPick(MailRule.of(MailMode.NONE))
    }
}

@Composable
private fun ChoiceCard(
    title: String,
    body: String?,
    selected: Boolean,
    enabled: Boolean,
    extra: @Composable () -> Unit = {},
    onClick: () -> Unit,
) {
    Column(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(20.dp))
            .background(Mesh.Card)
            .border(BorderStroke(2.dp, if (selected) Mesh.Indigo else Mesh.Hairline), RoundedCornerShape(20.dp))
            .clickable(enabled = enabled && !selected, onClick = onClick)
            .padding(horizontal = 12.dp, vertical = 10.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            RadioButton(selected = selected, onClick = null, enabled = enabled, colors = RadioButtonDefaults.colors(selectedColor = Mesh.Indigo))
            Spacer(Modifier.width(8.dp))
            Column(Modifier.weight(1f)) {
                Text(title, style = MaterialTheme.typography.titleSmall, color = Mesh.Ink)
                body?.let { Text(it, style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2) }
            }
        }
        extra()
    }
}

/** One AI's own rule: follow the network, or any / only / none for it alone. */
@Composable
private fun AiRuleDialog(ai: MailParty, view: MailPolicyView, onDismiss: () -> Unit, onSave: (MailRule?) -> Unit) {
    val (start, own) = view.ruleFor(ai.id)
    var following by remember { mutableStateOf(!own) }
    var rule by remember { mutableStateOf(start) }
    val name = ai.name.ifBlank { ai.id }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.mail_ai_title, name)) },
        text = {
            Column(Modifier.verticalScroll(rememberScrollState())) {
                Text(
                    stringResource(R.string.mail_ai_body, mailRuleLabel(view.network, view)),
                    style = MaterialTheme.typography.bodySmall,
                    color = Mesh.Ink2,
                )
                Gap(10.dp)
                RuleChoices(
                    rule = rule,
                    mailboxes = view.mailboxes.filter { it.id != ai.id },
                    enabled = true,
                    includeFollow = true,
                    following = following,
                    onPick = {
                        following = false
                        rule = it
                    },
                    onFollow = { following = true },
                )
                Gap(8.dp)
                Text(stringResource(R.string.mail_ai_own_box, name), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink3)
            }
        },
        confirmButton = {
            TextButton(onClick = { onSave(if (following) null else rule) }) { Text(stringResource(R.string.save)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}

/**
 * A note flying from an AI to a helper's mailbox; when mailboxes are shut
 * it stays put and fades. Still when animations are off.
 */
@Composable
private fun NoteFlight(open: Boolean) {
    val reduceMotion = rememberReduceMotion()
    val t = rememberInfiniteTransition(label = "note")
    val p by t.animateFloat(
        initialValue = 0f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(tween(2600, easing = LinearEasing), RepeatMode.Restart),
        label = "flight",
    )
    val progress = if (reduceMotion || !open) 0.5f else p
    MeshCard(Modifier.fillMaxWidth(), contentPadding = 12.dp) {
        BoxWithConstraints(Modifier.fillMaxWidth().height(56.dp)) {
            val travel = maxWidth - 96.dp
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically) {
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    HelperBlob(Mesh.Indigo, Mood.THINKING, 36.dp)
                    Text("AI", style = MaterialTheme.typography.labelSmall, color = Mesh.Ink2)
                }
                Spacer(Modifier.weight(1f))
                Column(horizontalAlignment = Alignment.CenterHorizontally) {
                    Box(
                        Modifier
                            .clip(RoundedCornerShape(10.dp))
                            .background(Mesh.IndigoTint)
                            .padding(horizontal = 9.dp, vertical = 6.dp),
                    ) { Text("📮", style = MaterialTheme.typography.titleMedium) }
                    Text(stringResource(R.string.mail_helper), style = MaterialTheme.typography.labelSmall, color = Mesh.Ink2)
                }
            }
            val fade = when {
                !open -> 0.25f
                reduceMotion -> 1f
                progress < 0.12f -> progress / 0.12f
                progress > 0.85f -> (1f - progress) / 0.15f
                else -> 1f
            }
            Text(
                if (open) "✉" else "✕",
                style = MaterialTheme.typography.titleLarge,
                color = if (open) Mesh.Ink else Mesh.Rose,
                modifier = Modifier
                    .offset(x = 40.dp + travel * progress, y = 10.dp)
                    .alpha(fade.coerceIn(0f, 1f)),
            )
        }
    }
}
