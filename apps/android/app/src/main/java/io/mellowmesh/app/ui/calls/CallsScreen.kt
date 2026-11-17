package io.mellowmesh.app.ui.calls

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.OutlinedTextFieldDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.mellowmesh.app.R
import io.mellowmesh.app.data.friendly
import io.mellowmesh.app.meshApp
import io.mellowmesh.app.ui.components.HelperBlob
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.Conversations
import io.mellowmesh.core.Identity
import io.mellowmesh.core.Mood
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** The night-dial palette: calls happen "after hours", in a quiet sky. */
private object Night {
    val Sky = Color(0xFF0E0D1A)
    val Glow = Color(0xFF2A2560)
    val Sea = Color(0xFF1B3A4A)
    val Ink = Color(0xFFF3F2FA)
    val Ink2 = Color(0xFFB5B2D6)
    val Ink3 = Color(0xFF9C98C8)
    val Free = Color(0xFF8FF0C8)
    val Away = Color(0xFF6F6D86)
    val Hang = Color(0xFFE11D48)
    val Card = Color(0x14FFFFFF)
}

private fun nightBackground() = Brush.verticalGradient(listOf(Night.Glow, Night.Sky, Night.Sky, Night.Sea))

@Composable
private fun reduceMotion(): Boolean = io.mellowmesh.app.ui.theme.rememberReduceMotion()

/** How a Mellow shows on the dial. */
enum class OrbState { FREE, AWAY, RINGING, PICKED_UP }

/**
 * A Mellow as a call orb. Motion is the message:
 * FREE breathes, sends rings outward and carries an orbiting spark;
 * AWAY drains to grey, closes its eyes, sinks, stands in a dashed ring
 * with "z" drifting up; RINGING ripples fast; PICKED_UP glows and rises.
 */
@Composable
fun CallOrb(agentId: String, state: OrbState, size: Dp, modifier: Modifier = Modifier) {
    val still = reduceMotion()
    val color = Mesh.accent(Identity.accentOf(agentId))
    val t = rememberInfiniteTransition(label = "orb")
    val ringPeriod = if (state == OrbState.RINGING) 1100 else 2200
    val ring by t.animateFloat(0f, 1f, infiniteRepeatable(tween(ringPeriod, easing = LinearEasing)), label = "ring")
    val breathe by t.animateFloat(1f, 1.06f, infiniteRepeatable(tween(1200, easing = FastOutSlowInEasing), RepeatMode.Reverse), label = "breathe")
    val spin by t.animateFloat(0f, 360f, infiniteRepeatable(tween(5000, easing = LinearEasing)), label = "spin")
    val sink by t.animateFloat(0f, 1f, infiniteRepeatable(tween(1600, easing = FastOutSlowInEasing), RepeatMode.Reverse), label = "sink")
    val drift by t.animateFloat(0f, 1f, infiniteRepeatable(tween(2600, easing = LinearEasing)), label = "drift")
    val away = state == OrbState.AWAY
    val desc = stringResource(if (away) R.string.call_orb_away else R.string.call_orb_free)

    Box(modifier.size(size).semantics { contentDescription = desc }, contentAlignment = Alignment.Center) {
        Canvas(Modifier.fillMaxSize()) {
            val r = this.size.minDimension / 2f
            if (away) {
                drawCircle(
                    color = Night.Away.copy(alpha = 0.7f),
                    radius = r * 0.94f,
                    style = Stroke(width = 2.5f, pathEffect = PathEffect.dashPathEffect(floatArrayOf(10f, 14f))),
                )
            } else if (!still) {
                // Two rings walking outward, half a period apart.
                for (phase in listOf(0f, 0.5f)) {
                    val p = (ring + phase) % 1f
                    drawCircle(
                        color = color.copy(alpha = (1f - p) * 0.7f),
                        radius = r * (0.62f + 0.38f * p),
                        style = Stroke(width = 3f),
                    )
                }
            } else {
                drawCircle(color = color.copy(alpha = 0.5f), radius = r * 0.92f, style = Stroke(width = 3f))
            }
            if (state == OrbState.FREE || state == OrbState.PICKED_UP) {
                val angle = Math.toRadians((if (still) 300f else spin).toDouble())
                val sparkR = r * 0.9f
                drawCircle(
                    color = Color.White.copy(alpha = 0.9f),
                    radius = r * 0.05f,
                    center = center.copy(
                        x = center.x + (sparkR * kotlin.math.cos(angle)).toFloat(),
                        y = center.y + (sparkR * kotlin.math.sin(angle)).toFloat(),
                    ),
                )
            }
        }
        val scale = when {
            still -> 1f
            state == OrbState.PICKED_UP -> breathe * 1.04f
            away -> 0.92f
            else -> breathe
        }
        HelperBlob(
            color = if (away) Night.Away else color,
            mood = if (away) Mood.ASLEEP else Mood.IDLE,
            size = size * 0.62f,
            modifier = Modifier.graphicsLayer(
                scaleX = scale,
                scaleY = scale,
                alpha = if (away) 0.75f else 1f,
                translationY = if (away && !still) sink * 8f else 0f,
            ),
            description = null,
        )
        if (away && !still) {
            for (i in 0..1) {
                val p = (drift + i * 0.5f) % 1f
                Text(
                    "z",
                    color = Night.Ink3.copy(alpha = (1f - p)),
                    fontWeight = FontWeight.ExtraBold,
                    fontSize = (11 + i * 4).sp,
                    modifier = Modifier
                        .align(Alignment.TopEnd)
                        .offset(x = (p * 10).dp - 6.dp, y = (-p * 22).dp + 10.dp),
                )
            }
        }
    }
}

/** Who can pick up right now: the network answers and the Mellow is not asleep. */
@Composable
private fun rememberReachability(): (String) -> Boolean {
    val app = LocalContext.current.meshApp
    val reachable by app.repo.reachable.collectAsStateWithLifecycle()
    val home by app.repo.home.collectAsStateWithLifecycle()
    val moods = home?.helpers.orEmpty().associate { it.id.lowercase() to it.mood }
    return { id -> reachable == true && moods[id.lowercase()] != Mood.ASLEEP }
}

/** Calls: a night dial of every Mellow this phone knows, online or not. */
@Composable
fun CallsScreen(onBack: () -> Unit, onCall: (String) -> Unit) {
    val app = LocalContext.current.meshApp
    val agents by app.repo.agents.collectAsStateWithLifecycle()
    val reachable by app.repo.reachable.collectAsStateWithLifecycle()
    val outbox by app.repo.outbox.collectAsStateWithLifecycle()
    val canPickUp = rememberReachability()
    val dir = app.repo.directory
    val sorted = agents.sortedWith(compareByDescending<io.mellowmesh.core.AgentRegistration> { canPickUp(it.id) }.thenBy { dir.nameOf(it.id).lowercase() })

    Column(
        Modifier
            .fillMaxSize()
            .background(nightBackground())
            .statusBarsPadding()
            .navigationBarsPadding(),
    ) {
        Row(Modifier.fillMaxWidth().padding(horizontal = 8.dp, vertical = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            TextButton(onClick = onBack) { Text("←", color = Night.Ink, fontSize = 22.sp) }
            Text(stringResource(R.string.calls_caption), color = Night.Ink3, fontWeight = FontWeight.Bold, letterSpacing = 1.6.sp, fontSize = 12.sp)
        }
        Column(Modifier.padding(horizontal = 22.dp)) {
            Text(stringResource(R.string.calls_title), color = Night.Ink, style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.ExtraBold)
            Spacer(Modifier.height(4.dp))
            Text(stringResource(R.string.calls_intro), color = Night.Ink2, style = MaterialTheme.typography.bodyMedium)
            if (reachable == false) {
                Spacer(Modifier.height(12.dp))
                Text(
                    stringResource(R.string.calls_resting),
                    color = Night.Ink,
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier.clip(RoundedCornerShape(14.dp)).background(Night.Card).padding(12.dp),
                )
            }
            if (outbox.isNotEmpty()) {
                Spacer(Modifier.height(8.dp))
                Text(stringResource(R.string.pending_count, outbox.size), color = Night.Free, style = MaterialTheme.typography.labelLarge)
            }
        }
        if (sorted.isEmpty()) {
            Text(
                stringResource(R.string.calls_empty),
                color = Night.Ink2,
                textAlign = TextAlign.Center,
                modifier = Modifier.fillMaxWidth().padding(32.dp),
            )
        }
        LazyVerticalGrid(
            columns = GridCells.Fixed(2),
            contentPadding = PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(22.dp),
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            modifier = Modifier.fillMaxSize(),
        ) {
            items(sorted, key = { it.id }) { a ->
                val free = canPickUp(a.id)
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    modifier = Modifier.clip(RoundedCornerShape(24.dp)).clickable { onCall(a.id) }.padding(8.dp),
                ) {
                    CallOrb(a.id, if (free) OrbState.FREE else OrbState.AWAY, 116.dp)
                    Spacer(Modifier.height(6.dp))
                    Text(dir.nameOf(a.id), color = if (free) Night.Ink else Night.Ink2, fontWeight = FontWeight.Bold, fontSize = 15.sp)
                    Text(
                        stringResource(if (free) R.string.call_free else R.string.call_away),
                        color = if (free) Night.Free else Night.Ink3,
                        fontSize = 12.sp,
                        fontWeight = FontWeight.SemiBold,
                    )
                }
            }
        }
    }
}

private enum class CallPhase { CALLING, CONNECTED, AWAY }

/** One call: it rings, then a Mellow picks up — or is away and takes a message. */
@Composable
fun CallScreen(agentId: String, onHangUp: () -> Unit, onOpenChat: (String) -> Unit) {
    val app = LocalContext.current.meshApp
    val scope = rememberCoroutineScope()
    val canPickUp = rememberReachability()
    val name = app.repo.directory.nameOf(agentId)
    val conversationId = Conversations.agentConversationId(agentId)
    val reachable by app.repo.reachable.collectAsStateWithLifecycle()
    var phase by remember { mutableStateOf(CallPhase.CALLING) }
    var input by remember { mutableStateOf("") }
    var note by remember { mutableStateOf<String?>(null) }
    val lines = remember { mutableStateListOf<Pair<Boolean, String>>() }
    val burst = remember { Animatable(0f) }
    val still = reduceMotion()

    LaunchedEffect(agentId) {
        if (!still) delay(1600)
        phase = if (canPickUp(agentId) && conversationId != null) CallPhase.CONNECTED else CallPhase.AWAY
        if (phase == CallPhase.CONNECTED && !still) {
            burst.snapTo(0f)
            burst.animateTo(1f, tween(700, easing = FastOutSlowInEasing))
        }
    }
    // The Mellow's words arrive live while the call is open.
    LaunchedEffect(conversationId) {
        val ref = conversationId?.let { Conversations.parseId(it) } ?: return@LaunchedEffect
        app.repo.live.collect { msg ->
            if (Conversations.isChatTopic(ref, msg.topic) && !msg.from.equals(app.repo.me, ignoreCase = true)) {
                lines.add(false to msg.body)
                if (phase == CallPhase.AWAY) phase = CallPhase.CONNECTED
            }
        }
    }

    val orbState = when (phase) {
        CallPhase.CALLING -> OrbState.RINGING
        CallPhase.CONNECTED -> OrbState.PICKED_UP
        CallPhase.AWAY -> OrbState.AWAY
    }
    val title = when (phase) {
        CallPhase.CALLING -> stringResource(R.string.calling, name)
        CallPhase.CONNECTED -> stringResource(R.string.call_connected, name)
        CallPhase.AWAY -> stringResource(R.string.call_is_away, name)
    }
    val reason = when {
        phase != CallPhase.AWAY -> null
        reachable == false -> stringResource(R.string.call_away_resting)
        else -> stringResource(R.string.call_away_asleep)
    }

    Column(
        Modifier
            .fillMaxSize()
            .background(nightBackground())
            .statusBarsPadding()
            .navigationBarsPadding()
            .imePadding()
            .padding(horizontal = 22.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Spacer(Modifier.height(40.dp))
        Box(contentAlignment = Alignment.Center) {
            if (burst.value in 0.01f..0.99f) {
                Canvas(Modifier.size(260.dp)) {
                    drawCircle(
                        color = Night.Free.copy(alpha = (1f - burst.value) * 0.5f),
                        radius = size.minDimension / 2f * burst.value,
                    )
                }
            }
            CallOrb(agentId, orbState, 200.dp)
        }
        Spacer(Modifier.height(16.dp))
        Text(title, color = Night.Ink, style = MaterialTheme.typography.headlineSmall, fontWeight = FontWeight.ExtraBold, textAlign = TextAlign.Center)
        reason?.let {
            Spacer(Modifier.height(6.dp))
            Text(it, color = Night.Ink2, style = MaterialTheme.typography.bodyMedium, textAlign = TextAlign.Center)
        }
        Spacer(Modifier.height(16.dp))
        Column(Modifier.weight(1f).fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            for ((mine, text) in lines.takeLast(4)) {
                Text(
                    text,
                    color = Night.Ink,
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = 4,
                    modifier = Modifier
                        .align(if (mine) Alignment.End else Alignment.Start)
                        .clip(RoundedCornerShape(16.dp))
                        .background(if (mine) Color(0x334F46E5) else Night.Card)
                        .padding(horizontal = 12.dp, vertical = 8.dp),
                )
            }
            note?.let { Text(it, color = Night.Free, style = MaterialTheme.typography.bodySmall) }
        }
        if (phase != CallPhase.CALLING && conversationId != null) {
            OutlinedTextField(
                value = input,
                onValueChange = { input = it },
                placeholder = {
                    Text(
                        stringResource(if (phase == CallPhase.AWAY) R.string.leave_message else R.string.say_something),
                        color = Night.Ink3,
                    )
                },
                colors = OutlinedTextFieldDefaults.colors(
                    focusedTextColor = Night.Ink,
                    unfocusedTextColor = Night.Ink,
                    focusedBorderColor = Night.Free,
                    unfocusedBorderColor = Night.Ink3,
                    cursorColor = Night.Free,
                ),
                modifier = Modifier.fillMaxWidth(),
                maxLines = 3,
            )
            Spacer(Modifier.height(10.dp))
        }
        Row(horizontalArrangement = Arrangement.spacedBy(18.dp), verticalAlignment = Alignment.CenterVertically) {
            if (phase != CallPhase.CALLING && conversationId != null) {
                TextButton(onClick = { onOpenChat(conversationId) }) { Text(stringResource(R.string.open_conversation), color = Night.Ink2) }
                Box(
                    Modifier
                        .size(60.dp)
                        .clip(CircleShape)
                        .background(Night.Free)
                        .clickable(enabled = input.isNotBlank()) {
                            val text = input.trim()
                            input = ""
                            lines.add(true to text)
                            scope.launch {
                                note = try {
                                    if (app.repo.sendOrQueue(conversationId, text) != null) null
                                    else app.getString(R.string.message_left, name)
                                } catch (e: Exception) {
                                    e.friendly()
                                }
                            }
                        },
                    contentAlignment = Alignment.Center,
                ) { Text("➤", color = Night.Sky, fontSize = 22.sp, fontWeight = FontWeight.Bold) }
            }
            Box(
                Modifier
                    .size(60.dp)
                    .clip(CircleShape)
                    .background(Night.Hang)
                    .clickable(onClick = onHangUp)
                    .semantics { contentDescription = "Hang up" },
                contentAlignment = Alignment.Center,
            ) { Text("✕", color = Color.White, fontSize = 22.sp, fontWeight = FontWeight.ExtraBold) }
        }
        Spacer(Modifier.height(20.dp))
    }
}

/** The quick-access strip on the Plaza: a row of Mellows to call. */
@Composable
fun CallStrip(onOpenCalls: () -> Unit, onCall: (String) -> Unit, modifier: Modifier = Modifier) {
    val app = LocalContext.current.meshApp
    val agents by app.repo.agents.collectAsStateWithLifecycle()
    val canPickUp = rememberReachability()
    val dir = app.repo.directory
    Column(
        modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(24.dp))
            .background(nightBackground())
            .clickable(onClick = onOpenCalls)
            .padding(16.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(stringResource(R.string.calls_strip_title), color = Night.Ink, fontWeight = FontWeight.ExtraBold, fontSize = 17.sp, modifier = Modifier.weight(1f))
            Text(stringResource(R.string.calls_strip_all), color = Night.Free, fontWeight = FontWeight.Bold, fontSize = 13.sp)
        }
        Spacer(Modifier.height(10.dp))
        if (agents.isEmpty()) {
            Text(stringResource(R.string.calls_empty), color = Night.Ink2, style = MaterialTheme.typography.bodySmall)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            for (a in agents.sortedByDescending { canPickUp(it.id) }.take(4)) {
                val free = canPickUp(a.id)
                Column(
                    horizontalAlignment = Alignment.CenterHorizontally,
                    modifier = Modifier.width(68.dp).clip(RoundedCornerShape(16.dp)).clickable { onCall(a.id) },
                ) {
                    CallOrb(a.id, if (free) OrbState.FREE else OrbState.AWAY, 60.dp)
                    Text(dir.nameOf(a.id), color = Night.Ink, fontSize = 12.sp, maxLines = 1, fontWeight = FontWeight.SemiBold)
                }
            }
        }
    }
}
