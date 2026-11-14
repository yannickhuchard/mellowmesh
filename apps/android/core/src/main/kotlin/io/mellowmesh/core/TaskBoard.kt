package io.mellowmesh.core

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive

// ============================================================================
// Board (kanban)
// ============================================================================

enum class BoardColumn(val title: String, val targetStatus: String) {
    TODO("To do", TaskStatus.OPEN),
    CLAIMED("Claimed", TaskStatus.CLAIMED),
    DOING("Doing", TaskStatus.IN_PROGRESS),
    WAITING("Waiting", TaskStatus.BLOCKED),
    DONE("Done", TaskStatus.COMPLETED),
}

data class BoardCard(
    val task: GraphTask,
    val column: BoardColumn,
    val laneLabel: String,
    val ownerId: String?,
    /** Titles of the tasks this one waits for. */
    val blockedByTitles: List<String>,
)

data class BoardModel(val columns: Map<BoardColumn, List<BoardCard>>) {
    fun cards(column: BoardColumn): List<BoardCard> = columns[column].orEmpty()
    val total: Int get() = columns.values.sumOf { it.size }

    companion object {
        /**
         * Column of a task: To do = open, Claimed = claimed, Doing = in_progress,
         * Waiting = blocked or open with unfinished blockers, Done = completed.
         * Cancelled (and unknown) tasks are hidden (null).
         */
        fun columnOf(t: GraphTask): BoardColumn? = when (t.status.lowercase()) {
            TaskStatus.OPEN -> if (t.blockedBy.isNotEmpty()) BoardColumn.WAITING else BoardColumn.TODO
            TaskStatus.CLAIMED -> BoardColumn.CLAIMED
            TaskStatus.IN_PROGRESS -> BoardColumn.DOING
            TaskStatus.BLOCKED -> BoardColumn.WAITING
            TaskStatus.COMPLETED, "done" -> BoardColumn.DONE
            else -> null
        }

        private fun priorityRank(p: String): Int = when (p.lowercase()) {
            "critical", "urgent" -> 0
            "high" -> 1
            "medium", "normal" -> 2
            "low" -> 3
            else -> 4
        }

        /**
         * Build the board. `laneFilter` keeps only tasks of those lanes and
         * `teamFilter` only tasks of that team (both optional, for filter chips).
         */
        fun build(graph: TaskGraph, laneFilter: Set<String> = emptySet(), teamFilter: String? = null): BoardModel {
            val lanes = graph.lanes.associateBy { it.id }
            val titles = graph.tasks.associate { it.id to it.title }
            val cards = graph.tasks
                .filter { laneFilter.isEmpty() || it.lane in laneFilter }
                .filter { teamFilter == null || it.team.equals(teamFilter, ignoreCase = true) }
                .mapNotNull { t ->
                    val col = columnOf(t) ?: return@mapNotNull null
                    BoardCard(
                        task = t,
                        column = col,
                        laneLabel = lanes[t.lane]?.label?.let { Sanitize.line(it) }?.ifBlank { null } ?: Sanitize.line(t.lane),
                        ownerId = t.claimedBy ?: t.assignedTo,
                        blockedByTitles = t.blockedBy.map { id -> titles[id]?.let { Sanitize.line(it) } ?: id },
                    )
                }
            val byColumn = BoardColumn.entries.associateWith { c ->
                cards.filter { it.column == c }.sortedWith(
                    if (c == BoardColumn.DONE) {
                        compareByDescending<BoardCard> { Time.parseMs(it.task.completedAt) ?: 0L }.thenBy { it.task.id }
                    } else {
                        compareByDescending<BoardCard> { it.task.critical }
                            .thenBy { priorityRank(it.task.priority) }
                            .thenBy { Time.parseMs(it.task.createdAt) ?: Long.MAX_VALUE }
                            .thenBy { it.task.id }
                    },
                )
            }
            return BoardModel(byColumn)
        }
    }
}

// ============================================================================
// Roadmap (multi-lane timeline)
// ============================================================================

enum class SegmentKind { WAITING, RUNNING }

data class Segment(val kind: SegmentKind, val startMs: Long, val endMs: Long) {
    val durationMs: Long get() = (endMs - startMs).coerceAtLeast(0)
}

data class RoadmapBar(
    val task: GraphTask,
    val segments: List<Segment>,
    val critical: Boolean,
    val done: Boolean,
    /** Row inside the lane so overlapping bars do not collide. */
    val row: Int,
) {
    val startMs: Long get() = segments.minOfOrNull { it.startMs } ?: 0L
    val endMs: Long get() = segments.maxOfOrNull { it.endMs } ?: 0L
    val waitingMs: Long get() = segments.filter { it.kind == SegmentKind.WAITING }.sumOf { it.durationMs }
    val runningMs: Long get() = segments.filter { it.kind == SegmentKind.RUNNING }.sumOf { it.durationMs }
}

data class RoadmapLane(val lane: Lane, val bars: List<RoadmapBar>) {
    val rows: Int get() = (bars.maxOfOrNull { it.row } ?: -1) + 1
}

data class RoadmapArrow(val from: String, val to: String, val critical: Boolean)

data class RoadmapModel(
    val nowMs: Long,
    val startMs: Long,
    val endMs: Long,
    val lanes: List<RoadmapLane>,
    val arrows: List<RoadmapArrow>,
    val criticalPath: List<String>,
) {
    fun bar(taskId: String): RoadmapBar? = lanes.asSequence().flatMap { it.bars }.firstOrNull { it.task.id == taskId }

    companion object {
        /**
         * Segments of a task: waiting from creation until it is started (or
         * claimed, or finished, or now), then running until it completes (or now).
         */
        fun segmentsOf(t: GraphTask, nowMs: Long): List<Segment> {
            if (t.status.lowercase() == TaskStatus.CANCELLED) return emptyList()
            val created = Time.parseMs(t.createdAt) ?: return emptyList()
            val start = Time.parseMs(t.startedAt) ?: Time.parseMs(t.claimedAt)
            val done = Time.parseMs(t.completedAt)
            val end = done ?: nowMs
            val waitEnd = (start ?: done ?: nowMs).coerceAtLeast(created)
            val out = ArrayList<Segment>()
            out += Segment(SegmentKind.WAITING, created, waitEnd)
            if (start != null) out += Segment(SegmentKind.RUNNING, waitEnd, end.coerceAtLeast(waitEnd))
            return out
        }

        fun build(graph: TaskGraph, nowMs: Long = Time.parseMs(graph.now) ?: System.currentTimeMillis()): RoadmapModel {
            val visible = graph.tasks.filter { it.status.lowercase() != TaskStatus.CANCELLED && Time.parseMs(it.createdAt) != null }
            val critical = TaskGraphMath.criticalPath(visible, graph.edges, nowMs)
            val criticalSet = critical.toSet() + visible.filter { it.critical }.map { it.id }
            val laneOrder = graph.lanes.map { it.id }
            val knownLanes = graph.lanes.associateBy { it.id }
            val byLane = visible.groupBy { it.lane }
            val laneIds = laneOrder.filter { it in byLane } + byLane.keys.filter { it !in knownLanes }.sorted()
            val lanes = laneIds.map { id ->
                val lane = knownLanes[id] ?: Lane(id, "unassigned", id)
                val tasks = byLane[id].orEmpty().sortedWith(compareBy<GraphTask> { Time.parseMs(it.createdAt) ?: 0L }.thenBy { it.id })
                // Greedy row packing: first row whose last bar ended before this one starts.
                val rowEnds = ArrayList<Long>()
                val bars = tasks.map { t ->
                    val segs = segmentsOf(t, nowMs)
                    val s = segs.minOf { it.startMs }
                    val e = segs.maxOf { it.endMs }
                    var row = rowEnds.indexOfFirst { it <= s }
                    if (row < 0) {
                        rowEnds += e; row = rowEnds.size - 1
                    } else {
                        rowEnds[row] = e
                    }
                    RoadmapBar(t, segs, t.id in criticalSet, t.status.lowercase() == TaskStatus.COMPLETED, row)
                }
                RoadmapLane(lane, bars)
            }
            val ids = visible.map { it.id }.toSet()
            val edges = TaskGraphMath.edges(visible, graph.edges).filter { it.from in ids && it.to in ids }
            val critPairs = critical.zipWithNext().toSet()
            val arrows = edges.map { RoadmapArrow(it.from, it.to, (it.from to it.to) in critPairs) }
            val start = visible.mapNotNull { Time.parseMs(it.createdAt) }.minOrNull() ?: (nowMs - 3_600_000)
            val end = maxOf(nowMs, visible.mapNotNull { Time.parseMs(it.completedAt) }.maxOrNull() ?: nowMs)
            return RoadmapModel(nowMs, start, end, lanes, arrows, critical)
        }
    }
}

object TaskGraphMath {
    /** Edges from the graph plus those implied by `depends_on` (dependency → dependent), de-duplicated. */
    fun edges(tasks: List<GraphTask>, edges: List<GraphEdge>): List<GraphEdge> {
        val out = LinkedHashSet<GraphEdge>()
        out.addAll(edges)
        for (t in tasks) for (d in t.dependsOn) out.add(GraphEdge(d, t.id))
        return out.toList()
    }

    /**
     * Longest chain of dependent tasks by elapsed time (creation → completion
     * or now): the chain that decides when everything is done. Cycles are
     * ignored. Returns task ids in dependency order.
     */
    fun criticalPath(tasks: List<GraphTask>, edges: List<GraphEdge>, nowMs: Long): List<String> {
        val byId = tasks.associateBy { it.id }
        val all = edges(tasks, edges).filter { it.from in byId && it.to in byId && it.from != it.to }
        if (all.isEmpty()) return emptyList()
        fun weight(t: GraphTask): Long {
            val c = Time.parseMs(t.createdAt) ?: return 0
            val e = Time.parseMs(t.completedAt) ?: nowMs
            return (e - c).coerceAtLeast(0)
        }
        val preds = all.groupBy({ it.to }, { it.from })
        val best = HashMap<String, Long>()
        val prev = HashMap<String, String?>()
        val visiting = HashSet<String>()
        fun longest(id: String): Long {
            best[id]?.let { return it }
            if (!visiting.add(id)) return 0 // cycle guard
            var bestPred: String? = null
            var bestVal = 0L
            for (p in preds[id].orEmpty()) {
                val v = longest(p)
                if (bestPred == null || v > bestVal) {
                    bestVal = v; bestPred = p
                }
            }
            visiting.remove(id)
            val total = bestVal + weight(byId.getValue(id))
            best[id] = total
            prev[id] = bestPred
            return total
        }
        val involved = all.flatMap { listOf(it.from, it.to) }.toSet()
        involved.forEach { longest(it) }
        var cur: String? = involved.maxWithOrNull(compareBy<String> { best[it] ?: 0L }.thenBy { it }) ?: return emptyList()
        val path = ArrayList<String>()
        val seen = HashSet<String>()
        while (cur != null && seen.add(cur)) {
            path += cur
            cur = prev[cur]
        }
        return path.reversed().takeIf { it.size >= 2 } ?: emptyList()
    }

    /** Ids of unfinished dependencies of `t`. */
    fun blockedBy(t: GraphTask, byId: Map<String, GraphTask>): List<String> =
        t.dependsOn.filter { d -> byId[d]?.let { it.status.lowercase() != TaskStatus.COMPLETED } ?: false }
}

/**
 * Maps time to pixels for the roadmap, with zoom and pan. `pxPerMs` is the
 * scale; `originMs` is the time at x = 0.
 */
data class TimeScale(val originMs: Long, val pxPerMs: Double) {
    fun x(ms: Long): Float = ((ms - originMs) * pxPerMs).toFloat()
    fun timeAt(x: Float): Long = originMs + (x / pxPerMs).toLong()

    /** Zoom by `factor` keeping the time under `focusX` in place. */
    fun zoom(factor: Double, focusX: Float): TimeScale {
        val f = factor.coerceIn(0.05, 20.0)
        val newPx = (pxPerMs * f).coerceIn(MIN_PX_PER_MS, MAX_PX_PER_MS)
        val focusT = timeAt(focusX)
        return TimeScale(focusT - (focusX / newPx).toLong(), newPx)
    }

    fun pan(dxPx: Float): TimeScale = copy(originMs = originMs - (dxPx / pxPerMs).toLong())

    /** A tick step that leaves at least `minGapPx` between ticks. */
    fun tickStepMs(minGapPx: Float = 72f): Long =
        STEPS.firstOrNull { it * pxPerMs >= minGapPx } ?: STEPS.last()

    /** Tick times covering [x0, x1] pixels, aligned to the step. */
    fun ticks(widthPx: Float, minGapPx: Float = 72f): List<Long> {
        val step = tickStepMs(minGapPx)
        val t0 = timeAt(0f)
        val t1 = timeAt(widthPx)
        var t = Math.floorDiv(t0, step) * step
        val out = ArrayList<Long>()
        while (t <= t1 && out.size < 500) {
            if (t >= t0) out += t
            t += step
        }
        return out
    }

    companion object {
        private const val MIN = 60_000L
        private const val HOUR = 60 * MIN
        val STEPS = listOf(MIN, 5 * MIN, 15 * MIN, 30 * MIN, HOUR, 3 * HOUR, 6 * HOUR, 12 * HOUR, 24 * HOUR, 7 * 24 * HOUR)
        const val MIN_PX_PER_MS = 72.0 / (30L * 24 * HOUR)
        const val MAX_PX_PER_MS = 72.0 / (MIN / 4)

        /** Fit [startMs, endMs] into `widthPx` with a little margin on both sides. */
        fun fit(startMs: Long, endMs: Long, widthPx: Float): TimeScale {
            val span = (endMs - startMs).coerceAtLeast(10 * MIN)
            val margin = span / 12
            val px = (widthPx / (span + 2 * margin).toDouble()).coerceIn(MIN_PX_PER_MS, MAX_PX_PER_MS)
            return TimeScale(startMs - margin, px)
        }
    }
}

// ============================================================================
// Live reducer: patch the in-memory graph from `_task.<id>.<event>` messages.
// ============================================================================

object TaskGraphReducer {
    val EVENTS = setOf("created", "claimed", "started", "progress", "completed", "updated", "cancelled", "reclaimed")

    /** `_task.<id>.<event>` → (id, event), or null. */
    fun parseTopic(topic: String): Pair<String, String>? {
        val segs = topic.split('.')
        if (segs.size != 3 || !segs[0].equals("_task", ignoreCase = true)) return null
        val ev = segs[2].lowercase()
        return if (ev in EVENTS && segs[1].isNotEmpty()) segs[1] to ev else null
    }

    /**
     * Apply one live message. Returns the same graph instance when the message
     * changes nothing (so callers can skip a redraw).
     */
    fun apply(graph: TaskGraph, msg: Message, nowMs: Long): TaskGraph {
        val (rawId, event) = parseTopic(msg.topic) ?: return graph
        val payload = msg.payload as? JsonObject
        val existing = graph.tasks.firstOrNull { it.id.equals(rawId, ignoreCase = true) }
        val id = existing?.id ?: rawId
        val fromPayload: Task? = payload?.takeIf { it["id"] != null && it["title"] != null }?.let {
            try {
                MeshJson.decodeFromJsonElement(Task.serializer(), it)
            } catch (_: Exception) {
                null
            }
        }?.takeIf { it.id.equals(id, ignoreCase = true) }

        val nowIso = Time.formatMs(nowMs)
        val updated: GraphTask = when {
            fromPayload != null -> merge(existing, fromPayload)
            existing == null -> return graph // progress/reclaim of a task we do not show
            event == "reclaimed" -> existing.copy(status = TaskStatus.OPEN, claimedBy = null, claimedAt = null, startedAt = null)
            event == "cancelled" -> existing.copy(status = TaskStatus.CANCELLED)
            event == "completed" -> existing.copy(status = TaskStatus.COMPLETED, completedAt = existing.completedAt ?: nowIso)
            event == "started" -> existing.copy(status = TaskStatus.IN_PROGRESS, startedAt = existing.startedAt ?: nowIso)
            event == "claimed" -> existing.copy(
                status = TaskStatus.CLAIMED,
                claimedAt = existing.claimedAt ?: nowIso,
                claimedBy = (payload?.get("claimed_by") as? JsonPrimitive)?.takeIf { it.isString }?.content ?: existing.claimedBy,
            )
            else -> return graph // progress without a task payload changes nothing structural
        }

        val others = graph.tasks.filter { !it.id.equals(id, ignoreCase = true) }
        val withLane = updated.copy(lane = laneFor(updated, graph.lanes, existing?.lane))
        var tasks = others + withLane
        // Recompute blockers and timings for everyone (a completion unblocks dependents).
        val byId = tasks.associateBy { it.id }
        tasks = tasks.map { t ->
            val segs = RoadmapModel.segmentsOf(t, nowMs)
            t.copy(
                blockedBy = TaskGraphMath.blockedBy(t, byId),
                waitingSeconds = segs.filter { it.kind == SegmentKind.WAITING }.sumOf { it.durationMs } / 1000,
                runningSeconds = segs.filter { it.kind == SegmentKind.RUNNING }.sumOf { it.durationMs } / 1000,
            )
        }
        val lanes = if (graph.lanes.any { it.id == withLane.lane }) graph.lanes else graph.lanes + newLane(withLane)
        val edges = TaskGraphMath.edges(tasks, graph.edges.filter { e -> e.to != id || e.from in withLane.dependsOn })
        val visible = tasks.filter { it.status.lowercase() != TaskStatus.CANCELLED }
        val critical = TaskGraphMath.criticalPath(visible, edges, nowMs).toSet()
        tasks = tasks.map { it.copy(critical = it.id in critical) }
        return graph.copy(now = nowIso, lanes = lanes, tasks = tasks, edges = edges)
    }

    private fun merge(existing: GraphTask?, t: Task): GraphTask = GraphTask(
        id = t.id,
        title = t.title,
        description = t.description,
        createdBy = t.createdBy,
        status = t.status,
        priority = t.priority,
        assignedTo = t.assignedTo,
        claimedBy = t.claimedBy,
        dependsOn = t.dependsOn,
        team = t.team,
        createdAt = t.createdAt ?: existing?.createdAt,
        claimedAt = t.claimedAt ?: existing?.claimedAt?.takeIf { t.status.lowercase() != TaskStatus.OPEN },
        startedAt = t.startedAt ?: existing?.startedAt?.takeIf { t.status.lowercase() != TaskStatus.OPEN },
        completedAt = t.completedAt ?: existing?.completedAt?.takeIf { t.status.lowercase() == TaskStatus.COMPLETED },
        lane = existing?.lane ?: "unassigned",
    )

    /** Lane of a task: its claimant or assignee, else its team, else unassigned. */
    fun laneFor(t: GraphTask, lanes: List<Lane>, previous: String?): String {
        val who = t.claimedBy ?: t.assignedTo
        val candidates = buildList {
            if (who != null) {
                add(who)
                add("${PrincipalKind.of(who).name.lowercase()}:$who")
            }
            if (t.team != null) {
                add("team:${t.team}")
                add(t.team)
            }
        }
        for (c in candidates) lanes.firstOrNull { it.id.equals(c, ignoreCase = true) }?.let { return it.id }
        if (who == null && t.team == null) {
            return lanes.firstOrNull { it.kind == "unassigned" }?.id ?: previous ?: "unassigned"
        }
        return candidates.first()
    }

    private fun newLane(t: GraphTask): Lane {
        val who = t.claimedBy ?: t.assignedTo
        return when {
            who != null -> Lane(t.lane, if (PrincipalKind.of(who) == PrincipalKind.AGENT) "agent" else "human", Identity.shortName(who))
            t.team != null -> Lane(t.lane, "team", Conversations.pretty(t.team))
            else -> Lane(t.lane, "unassigned", "Not assigned")
        }
    }
}
