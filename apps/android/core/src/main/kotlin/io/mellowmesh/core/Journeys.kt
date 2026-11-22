package io.mellowmesh.core

import kotlinx.serialization.Serializable
import java.text.Normalizer

/**
 * Journeys: the path of a person, a team or a customer, where Mellows help
 * along the way. A journey is not planned up front: it is the line of steps
 * (tasks on a `journey.<slug>` topic, on any machine) as they happen. Done
 * steps are stations on the line, the step under way is where the line
 * stops, and the next ones are dotted. When the same kind of steps with
 * the same Mellows comes back three times, the journey says so — that part
 * can become a routine.
 */

/** A task and the machine (and network segment) it lives on. */
data class PlacedTask(val task: Task, val machineId: String, val segmentKey: String)

enum class StationState {
    /** Finished. */
    DONE,

    /** Someone is on it. */
    NOW,

    /** A question waits for the person on this step. */
    NEEDS_YOU,

    /** Not started yet: the dotted part of the line. */
    NEXT,
}

data class Station(
    val taskId: String,
    val machineId: String,
    val title: String,
    val state: StationState,
    /** The Mellow or person on it (claimant, else assignee). */
    val who: String?,
    val whoName: String?,
    /** When it happened (done → finished, under way → started, next → created). */
    val atMs: Long,
    /** Pending decisions of this step (ids). */
    val decisions: List<String> = emptyList(),
    /** Part of the repeating pattern. */
    val repeats: Boolean = false,
)

/** "These steps came back [times] times with the same Mellows." */
data class RepeatHint(
    /** The steps of one round, as plain titles ("Visit flat", "Report"). */
    val steps: List<String>,
    /** Who did each step (same order as [steps]; null = nobody). */
    val who: List<String?>,
    val times: Int,
)

data class Journey(
    val slug: String,
    val name: String,
    val stations: List<Station>,
    val machineIds: Set<String>,
    val segmentKeys: Set<String>,
    val repeat: RepeatHint?,
    val lastMs: Long,
) {
    val done: Int get() = stations.count { it.state == StationState.DONE }

    /** The first dotted station: what comes next, and who helps. */
    val next: Station? get() = stations.firstOrNull { it.state == StationState.NEXT }

    /** Where the line is now (the step under way, else the last one done). */
    val current: Station?
        get() = stations.lastOrNull { it.state == StationState.NOW || it.state == StationState.NEEDS_YOU }
            ?: stations.lastOrNull { it.state == StationState.DONE }

    val needsYou: Boolean get() = stations.any { it.state == StationState.NEEDS_YOU }
    val finished: Boolean get() = stations.isNotEmpty() && stations.all { it.state == StationState.DONE }

    /** Everyone who helped or helps, in order of appearance. */
    val helpers: List<String> get() = stations.mapNotNull { it.whoName }.distinct()

    /** The machine to create new steps on: where the latest step lives. */
    val home: String? get() = stations.maxByOrNull { it.atMs }?.machineId

    /** One line for the list: what needs you, else where it is, else what is next. */
    val line: String
        get() = stations.firstOrNull { it.state == StationState.NEEDS_YOU }?.let { "${it.title} — needs you" }
            ?: stations.lastOrNull { it.state == StationState.NOW }?.let { s -> s.whoName?.let { "$it: ${s.title}" } ?: s.title }
            ?: next?.let { s -> "Next: ${s.title}" + (s.whoName?.let { " · $it" } ?: "") }
            ?: if (finished) "Arrived" else "Nothing yet"
}

/** A repeating part the person kept, to run again in one tap. */
@Serializable
data class Routine(
    val id: String,
    val journey: String,
    val steps: List<RoutineStep>,
)

@Serializable
data class RoutineStep(val title: String, val who: String? = null)

object Journeys {
    const val PREFIX = "journey."
    const val MAX_SLUG = 48
    const val MAX_NAME = 48
    const val REPEAT_AT = 3

    /** Lowercase ASCII letters and digits joined by `-` (the hub makes the same). */
    fun slug(name: String): String? {
        val plain = Normalizer.normalize(name.trim().lowercase(), Normalizer.Form.NFD).replace(Regex("\\p{M}+"), "")
        val sb = StringBuilder()
        for (c in plain) {
            if (c in 'a'..'z' || c in '0'..'9') sb.append(c)
            else if (sb.isNotEmpty() && sb.last() != '-') sb.append('-')
            if (sb.length >= MAX_SLUG) break
        }
        return sb.toString().trimEnd('-').takeIf { it.isNotEmpty() }
    }

    fun topic(slug: String): String = PREFIX + slug

    /** The journey a task is on, from its topics. */
    fun slugOf(topics: List<String>): String? = topics.firstNotNullOfOrNull { t ->
        if (t.startsWith(PREFIX, ignoreCase = true)) t.substring(PREFIX.length).lowercase().takeIf { s -> slug(s) == s } else null
    }

    /** "move-house" → "Move house". */
    fun nameOf(slug: String): String = slug.replace('-', ' ').replaceFirstChar { it.uppercase() }

    /** A clean journey name, or null when it is not usable. */
    fun cleanName(raw: String): String? = Sanitize.line(raw).trim().take(MAX_NAME).takeIf { slug(it) != null }

    fun stateOf(t: Task, pending: Set<String>): StationState? = when (t.status.lowercase()) {
        TaskStatus.CANCELLED -> null
        TaskStatus.COMPLETED, "done" -> StationState.DONE
        else -> when {
            t.decisions.any { it in pending } -> StationState.NEEDS_YOU
            t.status.lowercase() == TaskStatus.OPEN -> StationState.NEXT
            else -> StationState.NOW
        }
    }

    /**
     * Every journey of every machine. The same journey on two machines is
     * one journey (a path does not care where its Mellows run).
     * [pendingDecisions]: ids of decisions still waiting for the person.
     */
    fun build(
        tasks: List<PlacedTask>,
        pendingDecisions: Set<String>,
        names: Map<String, String>,
        selected: String? = null,
    ): List<Journey> {
        val bySlug = LinkedHashMap<String, MutableList<PlacedTask>>()
        val seen = HashSet<String>()
        for (p in tasks) {
            val slug = slugOf(p.task.topics) ?: continue
            // A task reached through two addresses of one hub counts once.
            if (!seen.add(p.task.id.lowercase())) continue
            bySlug.getOrPut(slug) { mutableListOf() } += p
        }
        val journeys = bySlug.mapNotNull { (slug, placed) ->
            if (selected != null && placed.none { it.segmentKey == selected }) return@mapNotNull null
            val stations = order(placed, pendingDecisions, names)
            if (stations.isEmpty()) return@mapNotNull null
            val (marked, hint) = withRepeat(stations)
            Journey(
                slug = slug,
                name = nameOf(slug),
                stations = marked,
                machineIds = placed.map { it.machineId }.toSet(),
                segmentKeys = placed.map { it.segmentKey }.toSet(),
                repeat = hint,
                lastMs = marked.maxOf { it.atMs },
            )
        }
        // What needs the person first, then what moves, newest first; arrived journeys last.
        return journeys.sortedWith(
            compareByDescending<Journey> { it.needsYou }
                .thenBy { it.finished }
                .thenByDescending { it.lastMs }
                .thenBy { it.slug },
        )
    }

    private fun order(placed: List<PlacedTask>, pending: Set<String>, names: Map<String, String>): List<Station> {
        val byId = placed.associateBy { it.task.id }
        val depth = HashMap<String, Int>()
        fun depthOf(id: String, guard: Set<String> = emptySet()): Int = depth[id] ?: run {
            val t = byId[id]?.task ?: return 0
            val d = t.dependsOn.filter { it in byId && it !in guard }.maxOfOrNull { depthOf(it, guard + id) + 1 } ?: 0
            depth[id] = d
            d
        }
        val stations = placed.mapNotNull { p ->
            val t = p.task
            val state = stateOf(t, pending) ?: return@mapNotNull null
            val who = t.claimedBy ?: t.assignedTo
            val at = when (state) {
                StationState.DONE -> Time.parseMs(t.completedAt)
                StationState.NEXT -> Time.parseMs(t.createdAt)
                else -> Time.parseMs(t.startedAt) ?: Time.parseMs(t.claimedAt)
            } ?: Time.parseMs(t.createdAt) ?: 0L
            Station(
                taskId = t.id,
                machineId = p.machineId,
                title = Sanitize.line(t.title).trim().ifEmpty { "Step" },
                state = state,
                who = who,
                whoName = who?.let { names[it.lowercase()] ?: Identity.shortName(it) },
                atMs = at,
                decisions = t.decisions.filter { it in pending },
            )
        }
        val done = stations.filter { it.state == StationState.DONE }.sortedWith(compareBy<Station> { it.atMs }.thenBy { it.taskId })
        val now = stations.filter { it.state == StationState.NOW || it.state == StationState.NEEDS_YOU }
            .sortedWith(compareBy<Station> { it.atMs }.thenBy { it.taskId })
        val next = stations.filter { it.state == StationState.NEXT }
            .sortedWith(compareBy<Station> { depthOf(it.taskId) }.thenBy { it.atMs }.thenBy { it.taskId })
        return done + now + next
    }

    /** Small words that do not tell a kind of step apart. */
    private val SMALL = setOf(
        "to", "the", "of", "for", "on", "in", "at", "an", "and", "with",
        "de", "du", "des", "la", "le", "les", "un", "une", "et", "au", "aux", "en", "pour", "sur",
    )

    private fun words(title: String): List<String> =
        Normalizer.normalize(title.lowercase(), Normalizer.Form.NFD).replace(Regex("\\p{M}+"), "")
            .split(Regex("[^a-z]+")).filter { it.length >= 2 && it !in SMALL }

    /** The kind of a step: its first two meaningful words, without numbers or what follows. */
    fun stem(title: String): String = words(title).take(2).joinToString(" ")

    /**
     * The longest run of steps (up to three) that came back at least
     * [REPEAT_AT] times with the same Mellows, among the steps that happened.
     */
    fun withRepeat(stations: List<Station>): Pair<List<Station>, RepeatHint?> {
        val happened = stations.filter { it.state != StationState.NEXT }
        val sigs = happened.map { stem(it.title) + "|" + (it.who?.lowercase() ?: "") }
        for (len in 3 downTo 1) {
            var best: Pair<Int, List<Int>>? = null // (start of first match, covered indices)
            var bestCount = 0
            val tried = HashSet<List<String>>()
            for (i in 0..sigs.size - len) {
                val pattern = sigs.subList(i, i + len)
                if (pattern.any { it.startsWith("|") } || !tried.add(pattern)) continue
                if (len > 1 && pattern.toSet().size == 1) continue // "A A A" is the 1-step pattern
                val covered = ArrayList<Int>()
                var j = 0
                var count = 0
                while (j <= sigs.size - len) {
                    if (sigs.subList(j, j + len) == pattern) {
                        count++
                        covered += (j until j + len)
                        j += len
                    } else {
                        j++
                    }
                }
                if (count >= REPEAT_AT && count > bestCount) {
                    bestCount = count
                    best = i to covered
                }
            }
            val b = best ?: continue
            val coveredIds = b.second.map { happened[it].taskId }.toSet()
            // Name the steps after the latest round.
            val lastStart = b.second.takeLast(len).first()
            val round = happened.subList(lastStart, lastStart + len)
            val hint = RepeatHint(
                steps = round.map { s -> plainTitle(s.title) },
                who = round.map { it.who },
                times = bestCount,
            )
            return stations.map { if (it.taskId in coveredIds) it.copy(repeats = true) else it } to hint
        }
        return stations to null
    }

    /** "Visit flat 12 rue Oberkampf" → "Visit flat"; "Report to Nadia, visit 2" → "Report to Nadia". */
    fun plainTitle(title: String): String {
        val n = stem(title).split(' ').filter { it.isNotEmpty() }.size
        val out = ArrayList<String>()
        var kept = 0
        for (w in title.trim().split(Regex("\\s+"))) {
            if (kept >= n) break
            val clean = w.trimEnd(',', '.', ':', ';')
            if (clean.none { it.isLetter() }) continue
            out += clean
            if (words(clean).isNotEmpty()) kept++
        }
        return out.joinToString(" ").ifEmpty { title.trim() }
    }

    /** A routine from the repeating part of [journey]. */
    fun routineOf(journey: Journey, id: String): Routine? {
        val r = journey.repeat ?: return null
        return Routine(id, journey.slug, r.steps.zip(r.who) { t, w -> RoutineStep(t, w) })
    }

    /**
     * The tasks of one more round of [routine]: each step on the journey,
     * given to the same Mellow, waiting for the one before.
     */
    fun nextRound(routine: Routine, newId: () -> String): List<Task> {
        var previous: String? = null
        return routine.steps.map { s ->
            val id = newId()
            Task(
                id = id,
                title = s.title,
                topics = listOf(topic(routine.journey)),
                assignedTo = s.who,
                dependsOn = listOfNotNull(previous),
            ).also { previous = id }
        }
    }

    /** The first step of a new journey (or a new step on one). */
    fun step(journeySlug: String, title: String, who: String?, id: String = ""): Task =
        Task(id = id, title = Sanitize.line(title).trim().take(200), topics = listOf(topic(journeySlug)), assignedTo = who)
}
