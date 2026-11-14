package io.mellowmesh.app.ui.roadmap

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.ExperimentalTextApi
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.mellowmesh.app.R
import io.mellowmesh.app.ui.board.TaskGraphViewModel
import io.mellowmesh.app.ui.board.TaskSheetContent
import io.mellowmesh.app.ui.components.BackHeader
import io.mellowmesh.app.ui.components.ErrorNote
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.HelperBlob
import io.mellowmesh.app.ui.components.Mello
import io.mellowmesh.app.ui.components.PersonAvatar
import io.mellowmesh.app.ui.components.meshViewModel
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.Identity
import io.mellowmesh.core.Mood
import io.mellowmesh.core.RoadmapBar
import io.mellowmesh.core.RoadmapLane
import io.mellowmesh.core.RoadmapModel
import io.mellowmesh.core.SegmentKind
import io.mellowmesh.core.TimeScale
import kotlinx.coroutines.delay
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import kotlin.math.abs

private val HOUR_FMT: DateTimeFormatter = DateTimeFormatter.ofPattern("HH:mm")
private val DAY_FMT: DateTimeFormatter = DateTimeFormatter.ofPattern("EEE d")

/** Vertical layout of lanes in pixels. */
private class LaneLayout(model: RoadmapModel, val rowPx: Float, val padPx: Float, val axisPx: Float) {
    val tops: List<Float>
    val heights: List<Float>
    val total: Float

    init {
        var y = axisPx
        val t = ArrayList<Float>()
        val h = ArrayList<Float>()
        for (lane in model.lanes) {
            val height = maxOf(1, lane.rows) * rowPx + 2 * padPx
            t += y
            h += height
            y += height
        }
        tops = t
        heights = h
        total = y
    }

    fun barTop(laneIndex: Int, row: Int): Float = tops[laneIndex] + padPx + row * rowPx + rowPx * 0.15f
    fun barHeight(): Float = rowPx * 0.7f
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalTextApi::class)
@Composable
fun RoadmapScreen(scope: String, onBack: () -> Unit) {
    val vm = meshViewModel(key = "board-$scope") { TaskGraphViewModel(it, scope) }
    val state by vm.state.collectAsStateWithLifecycle()
    var now by remember { mutableLongStateOf(System.currentTimeMillis()) }
    LaunchedEffect(Unit) {
        while (true) {
            delay(30_000)
            now = System.currentTimeMillis()
        }
    }

    Column(Modifier.fillMaxSize().background(Mesh.Paper)) {
        BackHeader(title = stringResource(R.string.roadmap), onBack = onBack)
        state.error?.let { ErrorNote(it, onRetry = vm::load, modifier = Modifier.padding(horizontal = 16.dp)) }
        val graph = state.graph
        if (graph == null) {
            Column(Modifier.fillMaxSize().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Gap(40.dp)
                Mello(size = 100.dp, mood = Mood.THINKING)
                Gap()
                Text(stringResource(R.string.loading), color = Mesh.Ink2)
            }
        } else {
            val model = remember(graph, now) { RoadmapModel.build(graph, maxOf(now, graph.now?.let { io.mellowmesh.core.Time.parseMs(it) } ?: 0L)) }
            if (model.lanes.isEmpty()) {
                Text(stringResource(R.string.no_tasks), color = Mesh.Ink3, modifier = Modifier.padding(24.dp))
            } else {
                RoadmapChart(model)
            }
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class, ExperimentalTextApi::class)
@Composable
private fun RoadmapChart(model: RoadmapModel) {
    val density = LocalDensity.current
    val rowPx = with(density) { 34.dp.toPx() }
    val padPx = with(density) { 8.dp.toPx() }
    val axisPx = with(density) { 28.dp.toPx() }
    val tickGapPx = with(density) { 64.dp.toPx() }
    val headWidth = 92.dp
    val layout = remember(model, rowPx) { LaneLayout(model, rowPx, padPx, axisPx) }
    var widthPx by remember { mutableFloatStateOf(0f) }
    var heightPx by remember { mutableFloatStateOf(0f) }
    var scale by remember { mutableStateOf<TimeScale?>(null) }
    var scrollY by remember { mutableFloatStateOf(0f) }
    var selected by remember { mutableStateOf<RoadmapBar?>(null) }
    val measurer = rememberTextMeasurer()
    val tickStyle = TextStyle(fontSize = 10.sp, color = Mesh.Ink3)
    val nowStyle = TextStyle(fontSize = 10.sp, color = Mesh.Rose)
    val maxScroll = (layout.total - heightPx).coerceAtLeast(0f)

    Row(Modifier.fillMaxSize()) {
        // Lane heads: avatar + name, scrolled with the chart.
        Box(
            Modifier
                .width(headWidth)
                .fillMaxHeight()
                .clipToBounds()
                .background(Mesh.Card),
        ) {
            model.lanes.forEachIndexed { i, lane ->
                val top = layout.tops[i] - scrollY
                Column(
                    Modifier
                        .offset { IntOffset(0, top.toInt()) }
                        .width(headWidth)
                        .height(with(density) { layout.heights[i].toDp() })
                        .padding(6.dp),
                    verticalArrangement = Arrangement.Center,
                    horizontalAlignment = Alignment.CenterHorizontally,
                ) {
                    LaneAvatar(lane)
                    Text(
                        lane.lane.label.ifBlank { Identity.shortName(lane.lane.id) },
                        style = MaterialTheme.typography.labelSmall,
                        color = Mesh.Ink,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
            }
        }

        Canvas(
            Modifier
                .weight(1f)
                .fillMaxHeight()
                .clipToBounds()
                .onSizeChanged {
                    widthPx = it.width.toFloat()
                    heightPx = it.height.toFloat()
                }
                .semantics { contentDescription = "Roadmap with ${model.lanes.size} lanes" }
                .pointerInput(model) {
                    detectTransformGestures { centroid, pan, zoom, _ ->
                        val base = scale ?: TimeScale.fit(model.startMs, model.endMs, size.width.toFloat())
                        scale = base.zoom(zoom.toDouble(), centroid.x).pan(pan.x)
                        scrollY = (scrollY - pan.y).coerceIn(0f, (layout.total - size.height).coerceAtLeast(0f))
                    }
                }
                .pointerInput(model) {
                    detectTapGestures { tap ->
                        val s = scale ?: TimeScale.fit(model.startMs, model.endMs, size.width.toFloat())
                        selected = hitTest(model, layout, s, tap.x, tap.y + scrollY)
                    }
                },
        ) {
            if (widthPx <= 0f) return@Canvas
            val s = scale ?: TimeScale.fit(model.startMs, model.endMs, size.width)
            val tickStep = s.tickStepMs(tickGapPx)
            // Axis ticks and grid.
            for (t in s.ticks(size.width, tickGapPx)) {
                val x = s.x(t)
                drawLine(Mesh.Hairline, Offset(x, axisPx), Offset(x, size.height), strokeWidth = 1f)
                val label = Instant.ofEpochMilli(t).atZone(ZoneId.systemDefault())
                    .format(if (tickStep >= 24L * 3_600_000) DAY_FMT else HOUR_FMT)
                drawText(measurer, label, topLeft = Offset(x + 4f, 6f), style = tickStyle)
            }
            // Lane stripes.
            model.lanes.forEachIndexed { i, _ ->
                val top = layout.tops[i] - scrollY
                if (i % 2 == 1) {
                    drawRect(Color.Black.copy(alpha = 0.025f), topLeft = Offset(0f, top), size = Size(size.width, layout.heights[i]))
                }
                drawLine(Mesh.Hairline, Offset(0f, top), Offset(size.width, top), strokeWidth = 1f)
            }
            // Dependency arrows (under the bars).
            for (a in model.arrows) {
                val from = barRect(model, layout, s, a.from) ?: continue
                val to = barRect(model, layout, s, a.to) ?: continue
                drawArrow(
                    start = Offset(from.second.x, from.first.y + layout.barHeight() / 2 - scrollY),
                    end = Offset(to.first.x, to.first.y + layout.barHeight() / 2 - scrollY),
                    color = if (a.critical) Mesh.Amber else Mesh.Ink3,
                    width = if (a.critical) 3f else 2f,
                )
            }
            // Bars.
            model.lanes.forEachIndexed { li, lane ->
                for (bar in lane.bars) {
                    drawBar(bar, s, layout.barTop(li, bar.row) - scrollY, layout.barHeight())
                }
            }
            // Now line.
            val nx = s.x(model.nowMs)
            drawLine(Mesh.Rose, Offset(nx, axisPx - 4f), Offset(nx, size.height), strokeWidth = 2.5f)
            drawCircle(Mesh.Rose, radius = 5f, center = Offset(nx, axisPx - 4f))
            drawText(measurer, "now", topLeft = Offset(nx + 6f, axisPx - 16f), style = nowStyle)
        }
    }

    val sel = selected
    if (sel != null) {
        ModalBottomSheet(onDismissRequest = { selected = null }, containerColor = Mesh.Card) {
            val laneLabel = model.lanes.firstOrNull { l -> l.bars.any { it.task.id == sel.task.id } }?.lane?.label ?: ""
            val titles = model.lanes.flatMap { it.bars }.associate { it.task.id to it.task.title }
            TaskSheetContent(
                task = sel.task.copy(
                    waitingSeconds = if (sel.task.waitingSeconds > 0) sel.task.waitingSeconds else sel.waitingMs / 1000,
                    runningSeconds = if (sel.task.runningSeconds > 0) sel.task.runningSeconds else sel.runningMs / 1000,
                ),
                laneLabel = laneLabel,
                blockedBy = sel.task.blockedBy.map { titles[it] ?: it },
                critical = sel.critical,
            )
            Spacer(Modifier.height(24.dp).navigationBarsPadding())
        }
    }
}

@Composable
private fun LaneAvatar(lane: RoadmapLane) {
    when (lane.lane.kind) {
        "agent" -> HelperBlob(Mesh.accent(Identity.accentOf(lane.lane.id.removePrefix("agent:"))), Mood.IDLE, 34.dp)
        "human" -> PersonAvatar(lane.lane.label.ifBlank { Identity.shortName(lane.lane.id) }, 30.dp)
        "team" -> Box(
            Modifier.size(30.dp).clip(RoundedCornerShape(10.dp)).background(Mesh.GreenTint),
            contentAlignment = Alignment.Center,
        ) { Text(Identity.initials(lane.lane.label.ifBlank { "T" }), style = MaterialTheme.typography.labelMedium, color = Mesh.Ink) }
        else -> Box(Modifier.size(30.dp).clip(RoundedCornerShape(15.dp)).background(Color(0xFFE6E4DE)))
    }
}

/** (top-left of the bar, top-right of the bar) in unscrolled coordinates. */
private fun barRect(model: RoadmapModel, layout: LaneLayout, s: TimeScale, taskId: String): Pair<Offset, Offset>? {
    model.lanes.forEachIndexed { li, lane ->
        val bar = lane.bars.firstOrNull { it.task.id == taskId } ?: return@forEachIndexed
        val y = layout.barTop(li, bar.row)
        return Offset(s.x(bar.startMs), y) to Offset(s.x(bar.endMs), y)
    }
    return null
}

private fun hitTest(model: RoadmapModel, layout: LaneLayout, s: TimeScale, x: Float, y: Float): RoadmapBar? {
    model.lanes.forEachIndexed { li, lane ->
        for (bar in lane.bars) {
            val top = layout.barTop(li, bar.row)
            val x0 = s.x(bar.startMs)
            val x1 = maxOf(s.x(bar.endMs), x0 + 12f)
            if (y in top..(top + layout.barHeight()) && x in (x0 - 8f)..(x1 + 8f)) return bar
        }
    }
    return null
}

/** Waiting = hatched, running = solid indigo (green when done); critical = amber outline. */
private fun DrawScope.drawBar(bar: RoadmapBar, s: TimeScale, top: Float, height: Float) {
    val radius = CornerRadius(height / 2.5f, height / 2.5f)
    for (seg in bar.segments) {
        val x0 = s.x(seg.startMs)
        val x1 = maxOf(s.x(seg.endMs), x0 + 3f)
        val w = x1 - x0
        if (x1 < 0f || x0 > size.width) continue
        when (seg.kind) {
            SegmentKind.WAITING -> {
                drawRoundRect(Mesh.AmberTint, topLeft = Offset(x0, top), size = Size(w, height), cornerRadius = radius)
                clipRect(left = x0, top = top, right = x1, bottom = top + height) {
                    var hx = x0 - height
                    while (hx < x1) {
                        drawLine(Mesh.Amber.copy(alpha = 0.55f), Offset(hx, top + height), Offset(hx + height, top), strokeWidth = 2f)
                        hx += 9f
                    }
                }
            }
            SegmentKind.RUNNING -> {
                drawRoundRect(
                    if (bar.done) Mesh.Green else Mesh.Indigo,
                    topLeft = Offset(x0, top),
                    size = Size(w, height),
                    cornerRadius = radius,
                )
            }
        }
    }
    if (bar.critical) {
        val x0 = s.x(bar.startMs)
        val x1 = maxOf(s.x(bar.endMs), x0 + 3f)
        drawRoundRect(
            Mesh.Amber,
            topLeft = Offset(x0 - 2f, top - 2f),
            size = Size(x1 - x0 + 4f, height + 4f),
            cornerRadius = radius,
            style = Stroke(width = 2.5f),
        )
    }
}

private fun DrawScope.drawArrow(start: Offset, end: Offset, color: Color, width: Float) {
    val dx = maxOf(abs(end.x - start.x) / 2f, 24f)
    val path = Path().apply {
        moveTo(start.x, start.y)
        cubicTo(start.x + dx, start.y, end.x - dx, end.y, end.x, end.y)
    }
    drawPath(path, color, style = Stroke(width = width, cap = StrokeCap.Round, pathEffect = PathEffect.cornerPathEffect(4f)))
    val head = Path().apply {
        moveTo(end.x, end.y)
        lineTo(end.x - 9f, end.y - 5f)
        lineTo(end.x - 9f, end.y + 5f)
        close()
    }
    drawPath(head, color)
}
