package io.mellowmesh.core

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class JourneysTest {
    private val scout = "agent://nadia/scout"
    private val writer = "agent://nadia/writer"
    private val movers = "agent://team/movers"
    private val names = mapOf(scout to "Scout", writer to "Writer", movers to "Movers")
    private var clock = 0

    private fun at(min: Int) = Time.formatMs(1_800_000_000_000L + min * 60_000L)

    private fun task(
        title: String,
        status: String = TaskStatus.COMPLETED,
        who: String? = scout,
        journey: String = "move-house",
        decisions: List<String> = emptyList(),
        deps: List<String> = emptyList(),
        id: String = "t${++clock}",
    ) = Task(
        id = id,
        title = title,
        status = status,
        topics = listOf("work", Journeys.topic(journey)),
        claimedBy = if (status == TaskStatus.OPEN) null else who,
        assignedTo = who,
        decisions = decisions,
        dependsOn = deps,
        createdAt = at(clock),
        startedAt = if (status != TaskStatus.OPEN) at(clock) else null,
        completedAt = if (status == TaskStatus.COMPLETED) at(clock + 1) else null,
    )

    private fun placed(vararg t: Task, machine: String = "home", segment: String = "nadia") = t.map { PlacedTask(it, machine, segment) }

    @Test
    fun aJourneyIsItsStepsInTheOrderTheyHappened() {
        val tasks = placed(
            task("Find flats"),
            task("Visit flat Rue Oberkampf"),
            task("Sign the lease", status = TaskStatus.IN_PROGRESS, who = writer),
            task("Book movers", status = TaskStatus.OPEN, who = movers),
        )
        val j = Journeys.build(tasks, emptySet(), names).single()
        assertEquals("Move house", j.name)
        assertEquals(listOf(StationState.DONE, StationState.DONE, StationState.NOW, StationState.NEXT), j.stations.map { it.state })
        assertEquals(2, j.done)
        // T30: the dotted next station and its Mellow.
        assertEquals("Book movers", j.next!!.title)
        assertEquals("Movers", j.next!!.whoName)
        assertEquals("Writer: Sign the lease", j.line)
        assertEquals(listOf("Scout", "Writer", "Movers"), j.helpers)
    }

    @Test
    fun aQuestionOnAStepPutsTheJourneyFirst() {
        val calm = placed(task("Draft", journey = "blog"), task("Publish", status = TaskStatus.IN_PROGRESS, journey = "blog"))
        val asking = placed(task("Take the flat?", status = TaskStatus.BLOCKED, decisions = listOf("d1", "d0")))
        val all = Journeys.build(calm + asking, setOf("d1"), names)
        assertEquals(listOf("move-house", "blog"), all.map { it.slug })
        assertTrue(all.first().needsYou)
        assertEquals(listOf("d1"), all.first().stations.single().decisions)
        assertEquals("Take the flat? — needs you", all.first().line)
        // Answered: the step is simply under way again.
        assertFalse(Journeys.build(asking, emptySet(), names).single().needsYou)
    }

    @Test
    fun oneJourneyAcrossMachinesAndAddresses() {
        val a = task("Find flats")
        val b = task("Book movers", status = TaskStatus.OPEN, who = movers)
        val tasks = placed(a, machine = "home", segment = "nadia") +
            placed(b, machine = "team", segment = "movers co") +
            placed(a, machine = "home-tunnel", segment = "nadia") // the same hub, another address
        val j = Journeys.build(tasks, emptySet(), names).single()
        assertEquals(2, j.stations.size)
        assertEquals(setOf("nadia", "movers co"), j.segmentKeys)
        // A segment shows the journeys that touch it.
        assertEquals(1, Journeys.build(tasks, emptySet(), names, selected = "movers co").size)
        assertEquals(0, Journeys.build(tasks, emptySet(), names, selected = "elsewhere").size)
    }

    @Test
    fun nextStepsFollowTheirDependencies() {
        val pack = task("Pack boxes", status = TaskStatus.OPEN, id = "pack")
        val move = task("Move", status = TaskStatus.OPEN, deps = listOf("pack"), id = "move")
        // Created in the other order: "Move" was planned first.
        val j = Journeys.build(placed(move.copy(createdAt = at(-5)), pack), emptySet(), names).single()
        assertEquals(listOf("Pack boxes", "Move"), j.stations.map { it.title })
    }

    @Test
    fun whatRepeatsThreeTimesWithTheSameMellowsShows() {
        val rounds = (1..3).flatMap { n ->
            listOf(task("Visit flat $n rue des Lilas"), task("Report to Nadia, visit $n", who = writer))
        }
        val j = Journeys.build(placed(*(rounds + task("Sign the lease", status = TaskStatus.OPEN)).toTypedArray()), emptySet(), names).single()
        val r = j.repeat!!
        assertEquals(listOf("Visit flat", "Report to Nadia"), r.steps)
        assertEquals(listOf(scout, writer), r.who)
        assertEquals(3, r.times)
        assertEquals(6, j.stations.count { it.repeats })
        assertFalse(j.stations.last().repeats)
        // Twice is not a routine yet; another Mellow is another kind of step.
        assertNull(Journeys.withRepeat(j.stations.take(4)).second)
        val mixed = j.stations.take(5) + j.stations[5].copy(who = movers)
        assertEquals(1, Journeys.withRepeat(mixed).second!!.steps.size)
    }

    @Test
    fun aRoutineRunsOneMoreRoundOnTheJourney() {
        val rounds = (1..3).map { task("Weekly report $it", who = writer, journey = "acme-onboarding") }
        val j = Journeys.build(placed(*rounds.toTypedArray()), emptySet(), names).single()
        val routine = Journeys.routineOf(j, "r1")!!
        assertEquals(listOf(RoutineStep("Weekly report", writer)), routine.steps)
        var n = 0
        val two = Journeys.nextRound(routine.copy(steps = routine.steps + RoutineStep("Send it", scout))) { "task_x${++n}" }
        assertEquals(listOf("Weekly report", "Send it"), two.map { it.title })
        assertEquals(listOf(Journeys.topic("acme-onboarding")), two[0].topics)
        assertEquals(listOf("task_x1"), two[1].dependsOn)
        assertEquals(writer, two[0].assignedTo)
    }

    @Test
    fun namesAndSlugsAreClean() {
        assertEquals("demenagement-a-lyon", Journeys.slug("Déménagement à Lyon"))
        assertEquals("move-house", Journeys.slug("  Move house! "))
        assertNull(Journeys.slug("?!"))
        assertNull(Journeys.cleanName("   "))
        assertEquals("move-house", Journeys.slugOf(listOf("x", "JOURNEY.Move-House")))
        assertNull(Journeys.slugOf(listOf("journey.bad slug")))
        assertEquals("visit flat", Journeys.stem("Visit flat 3, rue Oberkampf"))
        assertEquals("Visit flat", Journeys.plainTitle("Visit flat 3, rue Oberkampf"))
        // Cancelled steps leave the line; a journey of only cancelled steps is gone.
        assertTrue(Journeys.build(placed(task("Oops", status = TaskStatus.CANCELLED)), emptySet(), names).isEmpty())
    }
}
