package io.mellowmesh.app.ui.welcome

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.compose.animation.AnimatedContent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.ViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewModelScope
import com.journeyapps.barcodescanner.ScanContract
import com.journeyapps.barcodescanner.ScanOptions
import io.mellowmesh.app.MellowMeshApp
import io.mellowmesh.app.R
import io.mellowmesh.app.data.friendly
import io.mellowmesh.app.ui.components.ErrorNote
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.InkButton
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.components.Mello
import io.mellowmesh.app.ui.components.meshViewModel
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.Mood
import io.mellowmesh.core.Pairing
import io.mellowmesh.core.UrlSafety
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

enum class ConnectMode { CHOOSE, CODE, PASTE, MANUAL }

data class PairingState(
    val tourDone: Boolean = false,
    val mode: ConnectMode = ConnectMode.CHOOSE,
    val link: String = "",
    val url: String = "",
    val token: String = "",
    val principal: String = "",
    /** "Enter a code" fields: the short code and the hub address beside it. */
    val code: String = "",
    val codeUrl: String = "",
    val busy: Boolean = false,
    val error: String? = null,
    /** Set when the address is plain http over the internet and needs an explicit OK. */
    val confirmPublicHttp: Boolean = false,
    val safety: UrlSafety? = null,
)

class PairingViewModel(private val app: MellowMeshApp) : ViewModel() {
    private val _state = MutableStateFlow(PairingState(tourDone = app.store.tourDone))
    val state: StateFlow<PairingState> = _state.asStateFlow()

    private var pending: Triple<String, String?, String?>? = null
    private var pendingName: String? = null

    fun finishTour() {
        app.store.tourDone = true
        _state.update { it.copy(tourDone = true) }
    }

    fun setMode(m: ConnectMode) = _state.update { it.copy(mode = m, error = null, confirmPublicHttp = false) }
    fun setLink(v: String) = _state.update { it.copy(link = v, error = null) }
    fun setUrl(v: String) = _state.update {
        val safety = runCatching { Pairing.safety(Pairing.validateBaseUrl(v)) }.getOrNull()
        it.copy(url = v, error = null, safety = safety, confirmPublicHttp = false)
    }
    fun setToken(v: String) = _state.update { it.copy(token = v.trim(), error = null) }
    fun setPrincipal(v: String) = _state.update { it.copy(principal = v.trim(), error = null) }
    fun setCode(v: String) = _state.update { it.copy(code = v, error = null) }
    fun setCodeUrl(v: String) = _state.update { it.copy(codeUrl = v, error = null) }

    /** "Enter a code": trade the short code for the pairing link, then connect. */
    fun submitCode() {
        val s = _state.value
        _state.update { it.copy(busy = true, error = null) }
        viewModelScope.launch {
            try {
                val info = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
                    Pairing.claim(s.codeUrl, s.code)
                }
                _state.update { it.copy(busy = false, safety = Pairing.safety(info.baseUrl)) }
                connectChecked(info.baseUrl, info.token, info.principal, info.networkName)
            } catch (e: Exception) {
                _state.update { it.copy(busy = false, error = e.friendly()) }
            }
        }
    }

    /** A scanned or pasted pairing link. */
    fun submitLink(text: String) {
        try {
            val info = Pairing.parse(text)
            _state.update { it.copy(link = "", safety = Pairing.safety(info.baseUrl)) }
            connectChecked(info.baseUrl, info.token, info.principal, info.networkName)
        } catch (e: Exception) {
            _state.update { it.copy(error = e.friendly(), mode = ConnectMode.PASTE) }
        }
    }

    fun submitManual() {
        val s = _state.value
        try {
            val url = Pairing.validateBaseUrl(s.url)
            val token = s.token.ifBlank { null }?.also { Pairing.validateToken(it) }
            val principal = s.principal.ifBlank { null }
            connectChecked(url, token, principal, null)
        } catch (e: Exception) {
            _state.update { it.copy(error = e.friendly()) }
        }
    }

    private fun connectChecked(url: String, token: String?, principal: String?, name: String?) {
        if (Pairing.safety(url) == UrlSafety.PUBLIC_PLAINTEXT) {
            pending = Triple(url, token, principal)
            pendingName = name
            _state.update { it.copy(confirmPublicHttp = true, safety = UrlSafety.PUBLIC_PLAINTEXT) }
            return
        }
        connect(url, token, principal, name)
    }

    fun confirmPublicHttp() {
        val p = pending ?: return
        pending = null
        _state.update { it.copy(confirmPublicHttp = false) }
        connect(p.first, p.second, p.third, pendingName)
    }

    private fun connect(url: String, token: String?, principal: String?, name: String?) {
        _state.update { it.copy(busy = true, error = null) }
        viewModelScope.launch {
            try {
                app.repo.pair(url, token, principal, name)
                app.store.tourDone = true
                // The root switches to the Plaza as soon as the config is stored.
            } catch (e: Exception) {
                _state.update { it.copy(error = e.friendly()) }
            } finally {
                _state.update { it.copy(busy = false) }
            }
        }
    }
}

@Composable
fun WelcomeScreen(pendingLink: String?, onLinkConsumed: () -> Unit) {
    val vm = meshViewModel { PairingViewModel(it) }
    val state by vm.state.collectAsStateWithLifecycle()

    LaunchedEffect(pendingLink) {
        if (pendingLink != null) {
            vm.finishTour()
            vm.submitLink(pendingLink)
            onLinkConsumed()
        }
    }

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(Mesh.Paper)
            .statusBarsPadding()
            .navigationBarsPadding()
            .imePadding(),
    ) {
        if (!state.tourDone) {
            TourPager(onDone = vm::finishTour, modifier = Modifier.fillMaxSize())
        } else {
            ConnectPanel(state, vm)
        }
    }
}

/** Mello's three-step tour: a mall of helpers · talk like you text · you stay in charge. */
@Composable
fun TourPager(onDone: () -> Unit, modifier: Modifier = Modifier) {
    var step by remember { mutableIntStateOf(0) }
    val titles = listOf(R.string.tour_1_title, R.string.tour_2_title, R.string.tour_3_title)
    val bodies = listOf(R.string.tour_1_body, R.string.tour_2_body, R.string.tour_3_body)
    val moods = listOf(Mood.IDLE, Mood.THINKING, Mood.DONE)
    Column(
        modifier = modifier.padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
            TextButton(onClick = onDone) { Text(stringResource(R.string.tour_skip), color = Mesh.Ink2) }
        }
        Spacer(Modifier.weight(1f))
        AnimatedContent(targetState = step, label = "tour") { s ->
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Mello(size = 168.dp, mood = moods[s])
                Gap(24.dp)
                if (s == 0) {
                    Text(stringResource(R.string.welcome_hello), style = MaterialTheme.typography.titleMedium, color = Mesh.Ink2)
                    Gap(8.dp)
                }
                Text(
                    stringResource(titles[s]),
                    style = MaterialTheme.typography.displaySmall,
                    color = Mesh.Ink,
                    textAlign = TextAlign.Center,
                )
                Gap(12.dp)
                Text(
                    stringResource(bodies[s]),
                    style = MaterialTheme.typography.bodyLarge,
                    color = Mesh.Ink2,
                    textAlign = TextAlign.Center,
                )
            }
        }
        Spacer(Modifier.weight(1f))
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            for (i in 0 until 3) {
                Box(
                    Modifier
                        .size(if (i == step) 10.dp else 8.dp)
                        .clip(CircleShape)
                        .background(if (i == step) Mesh.Ink else Mesh.Ink3.copy(alpha = 0.4f)),
                )
            }
        }
        Gap(20.dp)
        InkButton(
            text = stringResource(if (step < 2) R.string.tour_next else R.string.connect_title),
            onClick = { if (step < 2) step++ else onDone() },
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

@Composable
private fun ConnectPanel(state: PairingState, vm: PairingViewModel) {
    val scanPrompt = stringResource(R.string.scan_prompt)
    val scanLauncher = rememberLauncherForActivityResult(ScanContract()) { result ->
        val text = result.contents
        if (text != null) vm.submitLink(text)
    }
    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(24.dp),
    ) {
        Mello(size = 96.dp, mood = if (state.busy) Mood.WORKING else Mood.IDLE)
        Gap(16.dp)
        Text(stringResource(R.string.connect_title), style = MaterialTheme.typography.headlineLarge, color = Mesh.Ink)
        Gap(8.dp)
        Text(stringResource(R.string.connect_body), style = MaterialTheme.typography.bodyLarge, color = Mesh.Ink2)
        Gap(24.dp)

        InkButton(
            text = stringResource(R.string.scan_qr),
            enabled = !state.busy,
            onClick = {
                val options = ScanOptions().apply {
                    setDesiredBarcodeFormats(ScanOptions.QR_CODE)
                    setPrompt(scanPrompt)
                    setBeepEnabled(false)
                    setOrientationLocked(true)
                }
                scanLauncher.launch(options)
            },
            modifier = Modifier.fillMaxWidth(),
        )
        Gap(10.dp)
        Row(horizontalArrangement = Arrangement.spacedBy(10.dp), modifier = Modifier.fillMaxWidth()) {
            OutlinedButton(onClick = { vm.setMode(ConnectMode.CODE) }, modifier = Modifier.weight(1f)) {
                Text(stringResource(R.string.enter_code), color = Mesh.Ink)
            }
            OutlinedButton(onClick = { vm.setMode(ConnectMode.PASTE) }, modifier = Modifier.weight(1f)) {
                Text(stringResource(R.string.paste_link), color = Mesh.Ink)
            }
            OutlinedButton(onClick = { vm.setMode(ConnectMode.MANUAL) }, modifier = Modifier.weight(1f)) {
                Text(stringResource(R.string.enter_manually), color = Mesh.Ink)
            }
        }
        Gap(16.dp)

        when (state.mode) {
            ConnectMode.CODE -> MeshCard(Modifier.fillMaxWidth()) {
                OutlinedTextField(
                    value = state.code,
                    onValueChange = vm::setCode,
                    label = { Text(stringResource(R.string.pairing_code_label)) },
                    placeholder = { Text(stringResource(R.string.pairing_code_hint)) },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Ascii),
                    modifier = Modifier.fillMaxWidth(),
                )
                Gap(8.dp)
                OutlinedTextField(
                    value = state.codeUrl,
                    onValueChange = vm::setCodeUrl,
                    label = { Text(stringResource(R.string.hub_address)) },
                    placeholder = { Text(stringResource(R.string.hub_address_hint)) },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                    modifier = Modifier.fillMaxWidth(),
                )
                Gap(8.dp)
                Text(
                    stringResource(R.string.pairing_code_where),
                    style = MaterialTheme.typography.bodySmall,
                    color = Mesh.Ink2,
                )
                Gap()
                InkButton(
                    text = stringResource(if (state.busy) R.string.connecting else R.string.connect),
                    enabled = !state.busy && state.code.isNotBlank() && state.codeUrl.isNotBlank(),
                    onClick = vm::submitCode,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            ConnectMode.PASTE -> MeshCard(Modifier.fillMaxWidth()) {
                OutlinedTextField(
                    value = state.link,
                    onValueChange = vm::setLink,
                    label = { Text(stringResource(R.string.pairing_link_label)) },
                    modifier = Modifier.fillMaxWidth(),
                    minLines = 2,
                )
                Gap()
                InkButton(
                    text = stringResource(if (state.busy) R.string.connecting else R.string.connect),
                    enabled = !state.busy && state.link.isNotBlank(),
                    onClick = { vm.submitLink(state.link) },
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            ConnectMode.MANUAL -> MeshCard(Modifier.fillMaxWidth()) {
                OutlinedTextField(
                    value = state.url,
                    onValueChange = vm::setUrl,
                    label = { Text(stringResource(R.string.hub_address)) },
                    placeholder = { Text(stringResource(R.string.hub_address_hint)) },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                    modifier = Modifier.fillMaxWidth(),
                )
                Gap(8.dp)
                OutlinedTextField(
                    value = state.token,
                    onValueChange = vm::setToken,
                    label = { Text(stringResource(R.string.hub_key)) },
                    singleLine = true,
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                    modifier = Modifier.fillMaxWidth(),
                )
                Gap(8.dp)
                OutlinedTextField(
                    value = state.principal,
                    onValueChange = vm::setPrincipal,
                    label = { Text(stringResource(R.string.your_identity)) },
                    placeholder = { Text(stringResource(R.string.your_identity_hint)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                SafetyNote(state.safety)
                Gap()
                InkButton(
                    text = stringResource(if (state.busy) R.string.connecting else R.string.connect),
                    enabled = !state.busy && state.url.isNotBlank(),
                    onClick = vm::submitManual,
                    modifier = Modifier.fillMaxWidth(),
                )
            }
            ConnectMode.CHOOSE -> Unit
        }

        if (state.confirmPublicHttp) {
            Gap()
            MeshCard(Modifier.fillMaxWidth(), color = Mesh.RoseTint) {
                Text(stringResource(R.string.warning_public_http), color = Mesh.Ink, style = MaterialTheme.typography.bodyMedium)
                Gap(8.dp)
                OutlinedButton(onClick = vm::confirmPublicHttp) {
                    Text(stringResource(R.string.connect_anyway), color = Mesh.Rose)
                }
            }
        }
        state.error?.let {
            Gap()
            ErrorNote(it)
        }
        Spacer(Modifier.height(32.dp))
    }
}

@Composable
private fun SafetyNote(safety: UrlSafety?) {
    when (safety) {
        UrlSafety.PUBLIC_PLAINTEXT -> {
            Gap(8.dp)
            Text(stringResource(R.string.warning_public_http), color = Mesh.Rose, style = MaterialTheme.typography.bodySmall)
        }
        UrlSafety.PRIVATE_LAN -> {
            Gap(8.dp)
            Text(stringResource(R.string.note_private_http), color = Mesh.Ink2, style = MaterialTheme.typography.bodySmall)
        }
        else -> Unit
    }
}
