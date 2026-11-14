package io.mellowmesh.core

import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNotNull
import org.junit.jupiter.api.Assertions.assertSame
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class TaskBoardTest {
    private val t0 = Time.parseMs("2026-11-14T08:00:00Z")!!
    private val now = t0 + 4 * 3_600_000L
    private fun at(min: Long) = Time.formatMs(t0 + min * 60_000)

    private val coder = "agent://y/coder"
    private val writer = "agent://y/writer"

    private val graph = TaskGraph(
        now = Time.formatMs(now),
        lanes = listOf(
            Lane(coder, "agent", "Coder"),
            Lane(writer, "agent", "Writer"),
            Lane("unassigned", "unassigned", "Not assigned"),
        ),
        tasks = listOf(
            GraphTask("a", "Design", status = TaskStatus.COMPLETED, claimedBy = coder, lane = coder,
                createdAt = at(0), claimedAt = at(10), startedAt = at(20), completedAt = at(80)),
            GraphTask("b", "Build", status = TaskStatus.IN_PROGRESS, claimedBy = coder, lane = coder, dependsOn = listOf("a"),
                createdAt = at(5), startedAt = at(90)),
            GraphTask("c", "Write docs", status = TaskStatus.OPEN, lane = "unassigned", dependsOn = listOf("b"), blockedBy = listOf("b"),
                createdAt = at(30)),
            GraphTask("d", "Proofread", status = TaskStatus.CLAIMED, claimedBy = writer, lane = writer, createdAt = at(60), claimedAt = at(70), priority = "high"),
            GraphTask("e", "Old idea", status = TaskStatus.CANCELLED, lane = "unassigned", createdAt = at(1)),
            GraphTask("f", "Waiting on legal", status = TaskStatus.BLOCKED, lane = writer, createdAt = at(2)),
            GraphTask("g", "Quick fix", status = TaskStatus.OPEN, lane = "unassigned", createdAt = at(3), team = "home"),
        ),
        edges = listOf(GraphEdge("a", "b"), GraphEdge("b", "c")),
    )

    @Test
    fun boardColumns() {
        val board = BoardModel.build(graph)
        assertEquals(listOf("g"), board.cards(BoardColumn.TODO).map { it.task.id })
        assertEquals(listOf("d"), board.cards(BoardColumn.CLAIMED).map { it.task.id })
        assertEquals(listOf("b"), board.cards(BoardColumn.DOING).map { it.task.id })
        assertEquals(setOf("c", "f"), board.cards(BoardColumn.WAITING).map { it.task.id }.toSet())
        assertEquals(listOf("a"), board.cards(BoardColumn.DONE).map { it.task.id })
        assertEquals(6, board.total) // cancelled hidden
        assertEquals(listOf("Build"), board.cards(BoardColumn.WAITING).first { it.task.id == "c" }.blockedByTitles)
        assertEquals("Coder", board.cards(BoardColumn.DOING).single().laneLabel)

        assertEquals(listOf("d", "f"), BoardModel.build(graph, laneFilter = setOf(writer)).columns.values.flatten().map { it.task.id }.sorted())
        assertEquals(listOf("g"), BoardModel.build(graph, teamFilter = "home").columns.values.flatten().map { it.task.id })
        assertEquals(TaskStatus.IN_PROGRESS, BoardColumn.DOING.targetStatus)
        assertEquals(TaskStatus.BLOCKED, BoardColumn.WAITING.targetStatus)
    }

    @Test
    fun segmentsWaitThenRun() {
        val a = RoadmapModel.segmentsOf(graph.tasks[0], now)
        assertEquals(2, a.size)
        assertEquals(SegmentKind.WAITING, a[0].kind)
        assertEquals(20 * 60_000L, a[0].durationMs) // created → started
        assertEquals(60 * 60_000L, a[1].durationMs) // started → completed
        val c = RoadmapModel.segmentsOf(graph.tasks[2], now)
        assertEquals(1, c.size)
        assertEquals(now, c[0].endMs) // still waiting
        val d = RoadmapModel.segmentsOf(graph.tasks[3], now)
        assertEquals(t0 + 70 * 60_000, d[1].startMs) // claimed counts as start
        assertTrue(RoadmapModel.segmentsOf(graph.tasks[4], now).isEmpty())
    }

    @Test
    fun roadmapLanesArrowsAndCriticalPath() {
        val r = RoadmapModel.build(graph)
        assertEquals(now, r.nowMs)
        assertEquals(t0, r.startMs)
        assertEquals(listOf(coder, writer, "unassigned"), r.lanes.map { it.lane.id })
        // a and b overlap in time in the coder lane → two rows.
        assertEquals(2, r.lanes[0].rows)
        assertEquals(listOf("a", "b", "c"), r.criticalPath)
        assertTrue(r.arrows.all { it.critical })
        assertEquals(2, r.arrows.size)
        assertTrue(r.bar("b")!!.critical)
        assertFalse(r.bar("d")!!.critical)
        assertEquals(null, r.bar("e"))
        assertEquals(60 * 60_000L, r.bar("a")!!.runningMs)
    }

    @Test
    fun criticalPathPicksTheLongestChainAndSurvivesCycles() {
        val tasks = listOf(
            GraphTask("x", createdAt = at(0), completedAt = at(10), status = TaskStatus.COMPLETED),
            GraphTask("y", createdAt = at(0), completedAt = at(100), status = TaskStatus.COMPLETED),
            GraphTask("z", createdAt = at(0), completedAt = at(110), status = TaskStatus.COMPLETED, dependsOn = listOf("x", "y")),
        )
        assertEquals(listOf("y", "z"), TaskGraphMath.criticalPath(tasks, emptyList(), now))
        val cyclic = listOf(
            GraphTask("p", createdAt = at(0), dependsOn = listOf("q")),
            GraphTask("q", createdAt = at(0), dependsOn = listOf("p")),
        )
        assertNotNull(TaskGraphMath.criticalPath(cyclic, emptyList(), now))
        assertTrue(TaskGraphMath.criticalPath(listOf(tasks[0]), emptyList(), now).isEmpty())
    }

    @Test
    fun timeScale() {
        val s = TimeScale.fit(t0, now, 1000f)
        assertTrue(s.x(t0) > 0f)
        assertTrue(s.x(now) < 1000f)
        assertEquals(t0.toDouble(), s.timeAt(s.x(t0)).toDouble(), 5.0)
        val z = s.zoom(2.0, 500f)
        assertEquals(s.timeAt(500f).toDouble(), z.timeAt(500f).toDouble(), 5.0)
        assertEquals(s.pxPerMs * 2, z.pxPerMs, 1e-12)
        val p = s.pan(100f)
        assertEquals(s.x(t0) + 100f, p.x(t0), 0.5f)
        val ticks = s.ticks(1000f)
        assertTrue(ticks.size in 2..20)
        assertTrue(ticks.zipWithNext().all { (a, b) -> b - a == s.tickStepMs() })
        assertTrue(ticks.all { it % s.tickStepMs() == 0L })
    }

    @Test
    fun reducerPatchesFromLiveEvents() {
        // b completes → c is no longer blocked.
        val bDone = Message(
            topic = "_task.b.completed", from = coder,
            payload = MeshJson.parseToJsonElement(
                """{"id":"b","title":"Build","status":"completed","claimed_by":"$coder","depends_on":["a"],
                   "created_at":"${at(5)}","started_at":"${at(90)}","completed_at":"${at(200)}"}""",
            ),
        )
        val g1 = TaskGraphReducer.apply(graph, bDone, now)
        val c = g1.tasks.first { it.id == "c" }
        assertTrue(c.blockedBy.isEmpty())
        assertEquals(BoardColumn.TODO, BoardModel.columnOf(c))
        assertEquals(BoardColumn.DONE, BoardModel.columnOf(g1.tasks.first { it.id == "b" }))
        assertEquals(coder, g1.tasks.first { it.id == "b" }.lane)

        // A brand-new task for a new person creates a lane.
        val created = Message(
            topic = "_task.n.created", from = "human://zoe",
            payload = buildJsonObject {
                put("id", "n"); put("title", "Call the bank"); put("status", "claimed")
                put("claimed_by", "human://zoe"); put("created_at", at(230)); put("claimed_at", at(235))
            },
        )
        val g2 = TaskGraphReducer.apply(g1, created, now)
        val n = g2.tasks.first { it.id == "n" }
        assertEquals("human://zoe", n.lane)
        assertTrue(g2.lanes.any { it.id == "human://zoe" && it.kind == "human" && it.label == "zoe" })
        assertEquals(5L * 60, n.waitingSeconds)

        // Reclaim without a task payload reopens the task and clears the claim.
        val reclaim = Message(
            topic = "_task.d.reclaimed", from = "node://hub",
            payload = buildJsonObject { put("task_id", "d"); put("previous_claimant", writer); put("status", "open") },
        )
        val g3 = TaskGraphReducer.apply(g2, reclaim, now)
        val d = g3.tasks.first { it.id == "d" }
        assertEquals(TaskStatus.OPEN, d.status)
        assertEquals(null, d.claimedBy)
        assertEquals("unassigned", d.lane)

        // Cancel hides it from the board.
        val g4 = TaskGraphReducer.apply(g3, Message(topic = "_task.g.cancelled", from = "human://y"), now)
        assertEquals(null, BoardModel.columnOf(g4.tasks.first { it.id == "g" }))

        // Unknown task / non-task topics change nothing.
        assertSame(g4, TaskGraphReducer.apply(g4, Message(topic = "_task.zzz.progress", from = coder), now))
        assertSame(g4, TaskGraphReducer.apply(g4, Message(topic = "_project.x", from = coder), now))
        assertEquals("b" to "completed", TaskGraphReducer.parseTopic("_task.b.completed"))
    }
}
