package io.mellowmesh.app.ui.journeys

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.mellowmesh.app.R
import io.mellowmesh.app.data.friendly
import io.mellowmesh.app.meshApp
import io.mellowmesh.app.ui.chats.SectionLabel
import io.mellowmesh.app.ui.chats.SegmentStrip
import io.mellowmesh.app.ui.chats.segmentColor
import io.mellowmesh.app.ui.components.BackHeader
import io.mellowmesh.app.ui.components.ErrorNote
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.HelperBlob
import io.mellowmesh.app.ui.components.HopIn
import io.mellowmesh.app.ui.components.InkButton
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.components.Mello
import io.mellowmesh.app.ui.components.Pill
import io.mellowmesh.app.ui.components.SoundFx
import io.mellowmesh.app.ui.components.TabHeader
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.app.ui.theme.rememberReduceMotion
import io.mellowmesh.core.ChatSpace
import io.mellowmesh.core.Conversations
import io.mellowmesh.core.EverywhereModel
import io.mellowmesh.core.Identity
import io.mellowmesh.core.Journey
import io.mellowmesh.core.Journeys
import io.mellowmesh.core.Mood
import io.mellowmesh.core.PlacedHelper
import io.mellowmesh.core.Station
import io.mellowmesh.core.StationState
import io.mellowmesh.core.Time
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** Every journey, drawn from the tasks of every machine (refreshed with the rest). */
@Composable
private fun rememberJourneys(segment: String? = null): Pair<List<Journey>, EverywhereModel?> {
    val app = LocalContext.current.meshApp
    val tasks by app.repo.allTasks.collectAsStateWithLifecycle()
    val pending by app.repo.pendingDecisions.collectAsStateWithLifecycle()
    val everywhere by app.repo.everywhere.collectAsStateWithLifecycle()
    val names = everywhere?.helpers.orEmpty().associate { it.card.id.lowercase() to it.card.name }
    return Journeys.build(tasks, pending, names, segment) to everywhere
}

/**
 * Journeys: the paths Mellows help along. Each one is a line of stations
 * that draws itself as work happens; what needs the person comes first.
 */
@Composable
fun JourneysScreen(onHelp: () -> Unit, onOpen: (String) -> Unit) {
    val app = LocalContext.current.meshApp
    var segment by rememberSaveable { mutableStateOf<String?>(null) }
    var starting by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        while (true) {
            app.repo.refreshEverywhere()
            delay(30_000)
        }
    }
    val (journeys, ew) = rememberJourneys(segment)
    val groups by app.repo.groups.collectAsStateWithLifecycle()
    val conversations by app.repo.allConversations.collectAsStateWithLifecycle()
    val space = ew?.let { ChatSpace.build(it, conversations, groups) }
    val slotOf = space?.segments.orEmpty().associate { it.key to it.slot }

    Column(Modifier.fillMaxSize().background(Mesh.Paper)) {
        TabHeader(
            title = stringResource(R.string.tab_journeys),
            onHelp = onHelp,
            subtitle = stringResource(R.string.journeys_intro),
            actions = {
                Text(
                    stringResource(R.string.journeys_new),
                    style = MaterialTheme.typography.labelLarge,
                    color = Mesh.Card,
                    modifier = Modifier
                        .clip(CircleShape)
                        .background(Mesh.Ink)
                        .clickable { starting = true }
                        .padding(horizontal = 14.dp, vertical = 9.dp),
                )
            },
        )
        if (ew == null) {
            Column(Modifier.fillMaxSize().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Gap(40.dp)
                Mello(size = 110.dp, mood = Mood.THINKING)
                Gap()
                Text(stringResource(R.string.everywhere_loading), color = Mesh.Ink2)
            }
            return@Column
        }
        LazyColumn(
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
            modifier = Modifier.fillMaxSize(),
        ) {
            if ((space?.segments?.size ?: 0) > 1) {
                item(key = "segments") { SegmentStrip(space!!.segments, space.total, segment) { segment = it } }
            }
            if (journeys.isEmpty()) {
                item(key = "empty") {
                    Column(Modifier.fillMaxWidth().padding(top = 32.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                        Mello(size = 110.dp)
                        Gap()
                        Text(stringResource(R.string.journeys_empty), color = Mesh.Ink2, style = MaterialTheme.typography.bodyLarge)
                        Gap(20.dp)
                        InkButton(stringResource(R.string.journeys_start), onClick = { starting = true }, modifier = Modifier.fillMaxWidth())
                    }
                }
            }
            val moving = journeys.filter { !it.finished }
            val arrived = journeys.filter { it.finished }
            itemsIndexed(moving, key = { _, j -> "j-" + j.slug }) { i, j ->
                HopIn(order = i, modifier = Modifier.animateItem()) {
                    JourneyCard(j, ew, j.segmentKeys.mapNotNull { slotOf[it] }.takeIf { slotOf.size > 1 }.orEmpty()) { onOpen(j.slug) }
                }
            }
            if (arrived.isNotEmpty()) {
                item(key = "arrived") { SectionLabel(stringResource(R.string.journeys_arrived)) }
                items(arrived, key = { "a-" + it.slug }) { j ->
                    JourneyCard(j, ew, emptyList(), modifier = Modifier.animateItem()) { onOpen(j.slug) }
                }
            }
        }
    }
    if (starting && ew != null) {
        StepDialog(
            title = stringResource(R.string.journeys_start),
            askName = true,
            everyone = ew.helpers,
            onDismiss = { starting = false },
            onDone = { slug -> starting = false; onOpen(slug) },
            journey = null,
        )
    }
}

@Composable
private fun JourneyCard(j: Journey, ew: EverywhereModel, slots: List<Int>, modifier: Modifier = Modifier, onClick: () -> Unit) {
    val now = System.currentTimeMillis()
    MeshCard(
        modifier.fillMaxWidth().clickable(onClick = onClick),
        color = if (j.needsYou) Mesh.AmberTint else Mesh.Card,
        contentPadding = 14.dp,
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(j.name, style = MaterialTheme.typography.titleMedium, color = Mesh.Ink, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
            for (s in slots) {
                Spacer(Modifier.width(4.dp))
                Box(Modifier.size(8.dp).clip(CircleShape).background(segmentColor(s)))
            }
            Spacer(Modifier.width(8.dp))
            Text(Time.ago(now, j.lastMs), style = MaterialTheme.typography.labelSmall, color = Mesh.Ink3)
        }
        Gap(8.dp)
        MiniPath(j)
        Gap(8.dp)
        Row(verticalAlignment = Alignment.CenterVertically) {
            Row {
                helpersOf(j, ew).take(4).forEachIndexed { i, h ->
                    Box(Modifier.offset(x = (-8 * i).dp)) { HelperBlob(Mesh.accent(h.card.accent), h.card.mood, 24.dp, description = h.card.name) }
                }
            }
            Spacer(Modifier.width(4.dp))
            Text(
                j.line,
                style = MaterialTheme.typography.bodyMedium,
                color = if (j.needsYou) Mesh.Ink else Mesh.Ink2,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            j.repeat?.let {
                Spacer(Modifier.width(6.dp))
                Pill(stringResource(R.string.journeys_repeats_short, it.times), Mesh.GreenTint, Mesh.Ink)
            }
        }
    }
}

private fun helpersOf(j: Journey, ew: EverywhereModel): List<PlacedHelper> {
    val ids = j.stations.mapNotNull { it.who?.lowercase() }.distinct()
    return ids.mapNotNull { id -> ew.helpers.firstOrNull { it.card.id.lowercase() == id } }
}

/** The journey in one line of dots: filled = done, ring = under way, amber = needs you, dotted = next. */
@Composable
private fun MiniPath(j: Journey) {
    val shown = j.stations.takeLast(12)
    Canvas(Modifier.fillMaxWidth().height(16.dp)) {
        if (shown.isEmpty()) return@Canvas
        val r = 5.dp.toPx()
        val step = if (shown.size > 1) (size.width - 2 * r) / (shown.size - 1).coerceAtLeast(1) else 0f
        val y = size.height / 2
        val stepX = step.coerceAtMost(28.dp.toPx())
        shown.forEachIndexed { i, s ->
            val x = r + i * stepX
            if (i > 0) {
                val prevX = r + (i - 1) * stepX
                val dotted = s.state == StationState.NEXT
                drawLine(
                    color = if (dotted) Mesh.Ink3 else Mesh.Ink,
                    start = Offset(prevX + r, y),
                    end = Offset(x - r, y),
                    strokeWidth = 2.dp.toPx(),
                    pathEffect = if (dotted) PathEffect.dashPathEffect(floatArrayOf(4f, 6f)) else null,
                )
            }
            stationDot(s, Offset(x, y), r)
        }
    }
}

private fun androidx.compose.ui.graphics.drawscope.DrawScope.stationDot(s: Station, c: Offset, r: Float) {
    when (s.state) {
        StationState.DONE -> drawCircle(if (s.repeats) Mesh.Green else Mesh.Ink, r, c)
        StationState.NOW -> {
            drawCircle(Mesh.Card, r, c)
            drawCircle(Mesh.Indigo, r, c, style = Stroke(2.5f * density))
        }
        StationState.NEEDS_YOU -> drawCircle(Mesh.Amber, r, c)
        StationState.NEXT -> drawCircle(
            Mesh.Ink3,
            r,
            c,
            style = Stroke(1.5f * density, pathEffect = PathEffect.dashPathEffect(floatArrayOf(3f, 3f))),
        )
    }
}

/**
 * One journey: its stations top to bottom on a line that fills up to where
 * the work is, the dotted next steps, and what repeats.
 */
@Composable
fun JourneyScreen(
    slug: String,
    onBack: () -> Unit,
    onTalk: (machineId: String, conversationId: String) -> Unit,
    onTodo: () -> Unit,
) {
    val app = LocalContext.current.meshApp
    val scope = rememberCoroutineScope()
    val (journeys, ew) = rememberJourneys()
    val routines by app.repo.routines.collectAsStateWithLifecycle()
    var adding by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { app.repo.refreshEverywhere() }
    val j = journeys.firstOrNull { it.slug == slug }

    Column(Modifier.fillMaxSize().background(Mesh.Paper)) {
        BackHeader(
            title = j?.name ?: Journeys.nameOf(slug),
            onBack = onBack,
            subtitle = j?.let { stringResource(R.string.journeys_done_of, it.done, it.stations.size) },
        )
        if (j == null || ew == null) {
            Column(Modifier.fillMaxSize().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Gap(40.dp)
                Mello(size = 100.dp, mood = Mood.THINKING)
                Gap()
                Text(stringResource(R.string.everywhere_loading), color = Mesh.Ink2)
            }
            return@Column
        }
        val routine = routines.firstOrNull { it.journey == slug }
        val machineOf = ew.helpers.associate { it.card.id.lowercase() to it.machineId }
        LazyColumn(
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 24.dp),
            modifier = Modifier.fillMaxSize(),
        ) {
            error?.let { e -> item(key = "error") { ErrorNote(e) } }
            val r = j.repeat
            if (r != null || routine != null) {
                item(key = "repeat") {
                    RepeatCard(
                        steps = routine?.steps?.map { it.title } ?: r!!.steps,
                        times = r?.times,
                        kept = routine != null,
                        busy = busy,
                        onKeep = {
                            Journeys.routineOf(j, "r_" + slug)?.let { app.repo.keepRoutine(it) }
                            SoundFx.play(app, SoundFx.Cue.JOINED)
                        },
                        onRun = {
                            val rt = routine ?: return@RepeatCard
                            val home = rt.steps.firstNotNullOfOrNull { s -> s.who?.let { machineOf[it.lowercase()] } } ?: j.home ?: return@RepeatCard
                            busy = true
                            scope.launch {
                                error = runCatching { app.repo.addSteps(home, Journeys.nextRound(rt) { app.repo.newTaskId() }) }
                                    .exceptionOrNull()?.friendly()
                                busy = false
                            }
                        },
                        onDrop = { app.repo.dropRoutine(slug) },
                    )
                    Gap(12.dp)
                }
            }
            val reached = j.stations.indexOfLast { it.state != StationState.NEXT }
            itemsIndexed(j.stations, key = { _, s -> s.taskId }) { i, s ->
                StationRow(
                    s = s,
                    first = i == 0,
                    last = i == j.stations.lastIndex,
                    lineFilled = i < reached,
                    helper = s.who?.let { w -> ew.helpers.firstOrNull { it.card.id.equals(w, true) } },
                    order = i,
                    modifier = Modifier.animateItem(),
                    onClick = {
                        when {
                            s.state == StationState.NEEDS_YOU -> onTodo()
                            s.who != null -> Conversations.agentConversationId(s.who!!)?.let { conv ->
                                onTalk(machineOf[s.who!!.lowercase()] ?: s.machineId, conv)
                            }
                        }
                    },
                )
            }
            item(key = "add") {
                Gap(8.dp)
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clip(RoundedCornerShape(16.dp))
                        .clickable { adding = true }
                        .padding(vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(Modifier.width(32.dp), contentAlignment = Alignment.Center) {
                        Text("+", style = MaterialTheme.typography.titleLarge, color = Mesh.Indigo)
                    }
                    Spacer(Modifier.width(12.dp))
                    Text(stringResource(R.string.journeys_add_step), color = Mesh.Indigo, style = MaterialTheme.typography.titleSmall)
                }
            }
        }
        if (adding) {
            StepDialog(
                title = stringResource(R.string.journeys_add_step),
                askName = false,
                everyone = ew.helpers,
                onDismiss = { adding = false },
                onDone = { adding = false },
                journey = j,
            )
        }
    }
}

@Composable
private fun RepeatCard(
    steps: List<String>,
    times: Int?,
    kept: Boolean,
    busy: Boolean,
    onKeep: () -> Unit,
    onRun: () -> Unit,
    onDrop: () -> Unit,
) {
    MeshCard(Modifier.fillMaxWidth(), color = Mesh.GreenTint) {
        Text(
            if (kept) stringResource(R.string.journeys_routine) else stringResource(R.string.journeys_repeats, times ?: 3),
            style = MaterialTheme.typography.titleSmall,
            color = Mesh.Ink,
        )
        Gap(4.dp)
        Text(steps.joinToString("  →  "), style = MaterialTheme.typography.bodyMedium, color = Mesh.Ink2)
        Gap(8.dp)
        Row(verticalAlignment = Alignment.CenterVertically) {
            if (kept) {
                InkButton(stringResource(if (busy) R.string.journeys_running else R.string.journeys_run_again), onClick = onRun, enabled = !busy)
                Spacer(Modifier.width(8.dp))
                TextButton(onClick = onDrop) { Text(stringResource(R.string.journeys_forget_routine), color = Mesh.Ink2) }
            } else {
                InkButton(stringResource(R.string.journeys_make_routine), onClick = onKeep)
            }
        }
    }
}

/** A station: the line on the left (filled up to where the work is), the step and who helps. */
@Composable
private fun StationRow(
    s: Station,
    first: Boolean,
    last: Boolean,
    lineFilled: Boolean,
    helper: PlacedHelper?,
    order: Int,
    modifier: Modifier = Modifier,
    onClick: () -> Unit,
) {
    val still = rememberReduceMotion()
    // The line fills downwards, one station after the other, when the journey opens.
    val fill = remember { Animatable(0f) }
    LaunchedEffect(lineFilled) {
        if (lineFilled) {
            delay(order.coerceAtMost(12) * 70L)
            fill.animateTo(1f, if (still) tween(160) else spring(dampingRatio = 0.9f, stiffness = Spring.StiffnessLow))
        } else {
            fill.snapTo(0f)
        }
    }
    val now = System.currentTimeMillis()
    Row(modifier.fillMaxWidth().heightIn(min = 72.dp).clip(RoundedCornerShape(16.dp)).clickable(onClick = onClick)) {
        Canvas(Modifier.width(32.dp).height(72.dp)) {
            val cx = size.width / 2
            val cy = 26.dp.toPx()
            val r = 9.dp.toPx()
            val stroke = 3.dp.toPx()
            if (!first) drawLine(Mesh.Ink, Offset(cx, 0f), Offset(cx, cy - r), stroke)
            if (!last) {
                val dotted = !lineFilled
                drawLine(
                    if (dotted) Mesh.Ink3 else Mesh.Hairline.copy(alpha = 0.25f),
                    Offset(cx, cy + r),
                    Offset(cx, size.height),
                    stroke,
                    cap = StrokeCap.Round,
                    pathEffect = if (dotted) PathEffect.dashPathEffect(floatArrayOf(6f, 8f)) else null,
                )
                if (lineFilled) {
                    val end = cy + r + (size.height - cy - r) * fill.value
                    drawLine(Mesh.Ink, Offset(cx, cy + r), Offset(cx, end), stroke)
                }
            }
            stationDot(s, Offset(cx, cy), r)
        }
        Spacer(Modifier.width(12.dp))
        Column(Modifier.weight(1f).padding(top = 14.dp, bottom = 10.dp)) {
            Text(
                s.title,
                style = MaterialTheme.typography.titleSmall,
                color = if (s.state == StationState.NEXT) Mesh.Ink2 else Mesh.Ink,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
            val tail = when (s.state) {
                StationState.DONE -> stringResource(R.string.journeys_done_ago, Time.ago(now, s.atMs))
                StationState.NOW -> stringResource(R.string.journeys_now)
                StationState.NEEDS_YOU -> stringResource(R.string.journeys_needs_you)
                StationState.NEXT -> stringResource(R.string.journeys_next)
            }
            Text(
                listOfNotNull(s.whoName, tail).joinToString(" · "),
                style = MaterialTheme.typography.bodySmall,
                color = if (s.state == StationState.NEEDS_YOU) Mesh.Amber else Mesh.Ink3,
            )
            if (s.repeats) {
                Gap(4.dp)
                Pill(stringResource(R.string.journeys_repeat_tag), Mesh.GreenTint, Mesh.Ink)
            }
        }
        if (helper != null) {
            Box(Modifier.padding(top = 12.dp, end = 4.dp)) {
                HelperBlob(Mesh.accent(helper.card.accent), helper.card.mood, 34.dp, progress = helper.card.progress, description = helper.card.name)
            }
        } else if (s.who != null) {
            Box(Modifier.padding(top = 12.dp, end = 4.dp)) {
                io.mellowmesh.app.ui.components.PrincipalAvatar(s.who!!, s.whoName ?: Identity.shortName(s.who!!), 34.dp)
            }
        }
    }
}

/**
 * Start a journey (a name and its first step) or add a step to one: what
 * is to be done, and which Mellow helps (any network). The step is made on
 * the helping Mellow's machine, so that Mellow sees it.
 */
@Composable
private fun StepDialog(
    title: String,
    askName: Boolean,
    everyone: List<PlacedHelper>,
    journey: Journey?,
    onDismiss: () -> Unit,
    onDone: (slug: String) -> Unit,
) {
    val app = LocalContext.current.meshApp
    val scope = rememberCoroutineScope()
    var name by remember { mutableStateOf("") }
    var step by remember { mutableStateOf("") }
    var who by remember { mutableStateOf<PlacedHelper?>(null) }
    var error by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val slug = journey?.slug ?: Journeys.cleanName(name)?.let { Journeys.slug(it) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            Column {
                if (askName) {
                    OutlinedTextField(
                        value = name,
                        onValueChange = { name = it.take(Journeys.MAX_NAME) },
                        label = { Text(stringResource(R.string.journeys_name)) },
                        placeholder = { Text(stringResource(R.string.journeys_name_hint)) },
                        singleLine = true,
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Gap(8.dp)
                }
                OutlinedTextField(
                    value = step,
                    onValueChange = { step = it.take(200) },
                    label = { Text(stringResource(if (askName) R.string.journeys_first_step else R.string.journeys_step)) },
                    placeholder = { Text(stringResource(R.string.journeys_step_hint)) },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                Gap(8.dp)
                Text(stringResource(R.string.journeys_who), style = MaterialTheme.typography.labelLarge, color = Mesh.Ink2)
                Row(Modifier.horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    for (h in everyone.filter { it.card.isMine }.ifEmpty { everyone }) {
                        val on = who?.card?.id == h.card.id
                        Column(
                            Modifier
                                .width(64.dp)
                                .clip(RoundedCornerShape(14.dp))
                                .background(if (on) Mesh.IndigoTint else Color.Transparent)
                                .clickable { who = if (on) null else h }
                                .padding(vertical = 6.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                        ) {
                            HelperBlob(Mesh.accent(h.card.accent), h.card.mood, 36.dp, description = h.card.name)
                            Text(h.card.name, style = MaterialTheme.typography.labelSmall, color = Mesh.Ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        }
                    }
                }
                error?.let {
                    Gap(6.dp)
                    Text(it, color = Mesh.Rose, style = MaterialTheme.typography.bodySmall)
                }
            }
        },
        confirmButton = {
            TextButton(
                enabled = !busy && slug != null && step.isNotBlank(),
                onClick = {
                    val s = slug ?: return@TextButton
                    val machine = who?.machineId ?: journey?.home ?: app.repo.activeMachine ?: return@TextButton
                    busy = true
                    scope.launch {
                        val r = runCatching { app.repo.addSteps(machine, listOf(Journeys.step(s, step, who?.card?.id))) }
                        busy = false
                        r.onSuccess { onDone(s) }.onFailure { error = it.friendly() }
                    }
                },
            ) { Text(stringResource(if (askName) R.string.journeys_start_short else R.string.journeys_add)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.cancel)) } },
    )
}
