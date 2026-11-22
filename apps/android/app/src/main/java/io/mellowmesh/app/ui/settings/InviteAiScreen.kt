package io.mellowmesh.app.ui.settings

import android.content.Intent
import android.graphics.Bitmap
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import com.google.zxing.BarcodeFormat
import com.journeyapps.barcodescanner.BarcodeEncoder
import io.mellowmesh.app.R
import io.mellowmesh.app.meshApp
import io.mellowmesh.app.ui.components.BackHeader
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.InkButton
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.InviteCatalog
import io.mellowmesh.core.InviteLink
import io.mellowmesh.core.InviteRequest
import io.mellowmesh.core.MeshException
import kotlinx.coroutines.launch

/**
 * Invite an AI in one link: tap Grok, ChatGPT, Claude, Gemini, Mistral, or
 * name another AI (OpenBot, Buzz, your bot). The hub makes a short-lived
 * link that gives that AI its own limited key; copy it, copy a one-line
 * message around it, or share it straight into the AI's app.
 */
@Composable
fun InviteAiScreen(onBack: () -> Unit) {
    val context = LocalContext.current
    val app = context.meshApp
    val scope = rememberCoroutineScope()
    val clipboard = LocalClipboardManager.current
    var busy by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var link by remember { mutableStateOf<InviteLink?>(null) }
    var copied by remember { mutableStateOf<String?>(null) }
    var naming by remember { mutableStateOf(false) }
    val cannotReach = stringResource(R.string.mail_error_reach)
    val ownerOnly = stringResource(R.string.mail_error_owner)
    val oldHub = stringResource(R.string.mail_error_old_hub)
    val copiedText = stringResource(R.string.invite_copied)

    fun invite(request: InviteRequest) {
        if (busy) return
        busy = true
        error = null
        link = null
        copied = null
        scope.launch {
            runCatching { app.repo.inviteAi(request) }
                .onSuccess { link = it }
                .onFailure { e ->
                    error = when {
                        e is MeshException && e.kind == MeshException.Kind.NETWORK -> cannotReach
                        e is MeshException && (e.status == 401 || e.status == 403) -> ownerOnly
                        e is MeshException && e.status == 404 -> oldHub
                        else -> e.message ?: cannotReach
                    }
                }
            busy = false
        }
    }

    val hints = mapOf(
        "grok" to stringResource(R.string.invite_grok_hint),
        "claude-web" to stringResource(R.string.invite_claude_hint),
        "mistral" to stringResource(R.string.invite_mistral_hint),
    )
    val web = stringResource(R.string.invite_web)

    Column(Modifier.fillMaxSize().background(Mesh.Paper)) {
        BackHeader(title = stringResource(R.string.invite_screen_title), onBack = onBack)
        Column(
            Modifier
                .fillMaxSize()
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 16.dp),
        ) {
            Text(stringResource(R.string.invite_intro), style = MaterialTheme.typography.bodyMedium, color = Mesh.Ink2)
            Gap(12.dp)
            val tiles = InviteCatalog.web.map { Triple(it.platform, it.name, hints[it.platform] ?: web) }
            tiles.chunked(2).forEach { row ->
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    row.forEach { (platform, name, hint) ->
                        Tile(name, hint, selected = link?.principal?.endsWith(platform.replace('-', '_')) == true, enabled = !busy, modifier = Modifier.weight(1f)) {
                            invite(InviteRequest(platform = platform))
                        }
                    }
                    if (row.size == 1) {
                        Tile(stringResource(R.string.invite_other), stringResource(R.string.invite_other_hint), selected = false, enabled = !busy, accent = true, modifier = Modifier.weight(1f)) {
                            naming = true
                        }
                    }
                }
                Gap(8.dp)
            }
            if (tiles.size % 2 == 0) {
                Tile(stringResource(R.string.invite_other), stringResource(R.string.invite_other_hint), selected = false, enabled = !busy, accent = true, modifier = Modifier.fillMaxWidth()) {
                    naming = true
                }
            }

            if (busy) {
                Gap(12.dp)
                Text(stringResource(R.string.invite_making), color = Mesh.Ink2)
            }
            error?.let {
                Gap(8.dp)
                Text(it, color = Mesh.Rose, style = MaterialTheme.typography.bodySmall)
            }

            link?.let { l ->
                Gap(14.dp)
                MeshCard(Modifier.fillMaxWidth(), contentPadding = 16.dp) {
                    Text(stringResource(R.string.invite_ready, l.name), style = MaterialTheme.typography.titleMedium, color = Mesh.Ink)
                    Gap(8.dp)
                    Text(
                        l.joinUrl,
                        fontFamily = FontFamily.Monospace,
                        style = MaterialTheme.typography.bodySmall,
                        color = Mesh.Ink,
                        modifier = Modifier
                            .fillMaxWidth()
                            .clip(RoundedCornerShape(14.dp))
                            .background(Mesh.Paper)
                            .padding(12.dp),
                    )
                    Gap(10.dp)
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        InkButton(
                            text = stringResource(R.string.invite_copy_link),
                            modifier = Modifier.weight(1f),
                            onClick = {
                                clipboard.setText(AnnotatedString(l.joinUrl))
                                copied = copiedText
                            },
                        )
                        TextButton(onClick = {
                            clipboard.setText(AnnotatedString(l.message))
                            copied = copiedText
                        }) { Text(stringResource(R.string.invite_copy_message), color = Mesh.Indigo) }
                        TextButton(onClick = {
                            val send = Intent(Intent.ACTION_SEND).apply {
                                type = "text/plain"
                                putExtra(Intent.EXTRA_TEXT, l.message)
                            }
                            context.startActivity(Intent.createChooser(send, null))
                        }) { Text(stringResource(R.string.invite_share), color = Mesh.Indigo) }
                    }
                    copied?.let { Text(it, color = Mesh.Green, style = MaterialTheme.typography.bodySmall) }
                    Gap(8.dp)
                    val minutes = minutesLeft(l.expiresAt)
                    Text(
                        stringResource(R.string.invite_paste_in, l.name, minutes, l.opens, l.principal),
                        style = MaterialTheme.typography.bodySmall,
                        color = Mesh.Ink2,
                    )
                    if (!l.reachableFromWeb) {
                        Gap(8.dp)
                        Text(
                            stringResource(R.string.invite_unreachable),
                            style = MaterialTheme.typography.bodySmall,
                            color = Mesh.Ink,
                            modifier = Modifier
                                .fillMaxWidth()
                                .clip(RoundedCornerShape(14.dp))
                                .background(Mesh.AmberTint)
                                .padding(10.dp),
                        )
                    }
                    Gap(10.dp)
                    LinkQr(l.joinUrl)
                    if (l.code.isNotBlank()) {
                        Gap(6.dp)
                        Text(stringResource(R.string.invite_code, l.code), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink3)
                    }
                }
            }
            Gap(32.dp)
        }
    }

    if (naming) {
        var name by remember { mutableStateOf("") }
        AlertDialog(
            onDismissRequest = { naming = false },
            title = { Text(stringResource(R.string.invite_other_title)) },
            text = {
                Column {
                    Text(stringResource(R.string.invite_other_body), style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2)
                    Gap(8.dp)
                    OutlinedTextField(
                        value = name,
                        onValueChange = { name = it.take(40) },
                        label = { Text(stringResource(R.string.invite_name)) },
                        placeholder = { Text("Buzz") },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                }
            },
            confirmButton = {
                TextButton(enabled = InviteCatalog.validName(name), onClick = {
                    naming = false
                    invite(InviteRequest(name = name.trim()))
                }) { Text(stringResource(R.string.invite_make)) }
            },
            dismissButton = { TextButton(onClick = { naming = false }) { Text(stringResource(R.string.cancel)) } },
        )
    }
}

/** Minutes until an RFC 3339 time, at least 0 (30 when it cannot be read). */
private fun minutesLeft(expiresAt: String): Int = runCatching {
    val ms = java.time.Instant.parse(expiresAt).toEpochMilli() - System.currentTimeMillis()
    (ms / 60_000L).toInt().coerceAtLeast(0)
}.getOrDefault(30)

@Composable
private fun Tile(
    name: String,
    hint: String,
    selected: Boolean,
    enabled: Boolean,
    modifier: Modifier = Modifier,
    accent: Boolean = false,
    onClick: () -> Unit,
) {
    Column(
        modifier
            .clip(RoundedCornerShape(18.dp))
            .background(if (accent) Mesh.IndigoTint else Mesh.Card)
            .border(2.dp, if (selected) Mesh.Indigo else Mesh.Hairline, RoundedCornerShape(18.dp))
            .clickable(enabled = enabled, onClick = onClick)
            .padding(horizontal = 14.dp, vertical = 12.dp),
    ) {
        Text(name, style = MaterialTheme.typography.titleSmall, color = if (accent) Mesh.Indigo else Mesh.Ink)
        Text(hint, style = MaterialTheme.typography.bodySmall, color = if (accent) Mesh.Indigo else Mesh.Ink2)
    }
}

/** The link as a QR code, for an AI app on another screen with a camera. */
@Composable
private fun LinkQr(url: String) {
    val bitmap: Bitmap? = remember(url) {
        runCatching { BarcodeEncoder().encodeBitmap(url, BarcodeFormat.QR_CODE, 480, 480) }.getOrNull()
    }
    bitmap?.let {
        Row(Modifier.fillMaxWidth()) {
            Spacer(Modifier.weight(1f))
            Image(it.asImageBitmap(), contentDescription = url, modifier = Modifier.size(160.dp))
            Spacer(Modifier.weight(1f))
        }
    }
    Spacer(Modifier.width(0.dp))
}
