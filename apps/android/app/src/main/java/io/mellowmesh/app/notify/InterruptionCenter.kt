package io.mellowmesh.app.notify

import io.mellowmesh.app.data.ConfigStore
import io.mellowmesh.app.data.MeshRepository
import io.mellowmesh.core.Interruption
import io.mellowmesh.core.Ladder
import io.mellowmesh.core.Level
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.filter
import kotlinx.coroutines.launch
import java.time.LocalDate
import java.time.LocalTime

/** Something shown inside the app (Whisper strip or Tap peek). */
data class InAppNotice(val level: Level, val interruption: Interruption)

/**
 * Applies the interruption ladder to every live message: Glance → badges
 * (via a home refresh), Whisper / Tap → in-app strip or peek, Knock / Alarm
 * → Android notification; small things go to the daily digest.
 */
class InterruptionCenter(
    private val repo: MeshRepository,
    private val store: ConfigStore,
    private val notifier: Notifier,
    private val scope: CoroutineScope,
) {
    private val _notices = MutableSharedFlow<InAppNotice>(extraBufferCapacity = 16)
    val notices: SharedFlow<InAppNotice> = _notices.asSharedFlow()

    private val seen = LinkedHashSet<String>()

    @OptIn(FlowPreview::class)
    fun start() {
        scope.launch {
            repo.live.collect { msg ->
                val interruption = Ladder.classify(msg, repo.me, repo.directory) ?: return@collect
                if (!seen.add(interruption.key)) return@collect
                if (seen.size > 500) seen.remove(seen.first())
                handle(interruption)
            }
        }
        // Anything that changes what the Plaza shows triggers a (debounced) refresh.
        scope.launch {
            repo.live
                .filter { m ->
                    val t = m.topic.lowercase()
                    t.startsWith("_decision.") || t.startsWith("_task.") || t.startsWith("_agent.") ||
                        t.startsWith("_chat.") || t.startsWith("_project.") || t.startsWith("_forum.") ||
                        t.startsWith("_team.") || t.startsWith("_org.")
                }
                .debounce(800)
                .collect { repo.refreshHome() }
        }
    }

    private fun handle(interruption: Interruption) {
        val settings = store.ladder
        val now = LocalTime.now()
        val outcome = Ladder.decide(interruption.requested, settings, now.hour * 60 + now.minute, System.currentTimeMillis())
        if (outcome.toDigest) store.digestCount = store.digestCount + 1
        when {
            outcome.notify -> notifier.post(interruption, outcome.level)
            outcome.level == Level.WHISPER || outcome.level == Level.TAP -> _notices.tryEmit(InAppNotice(outcome.level, interruption))
            else -> Unit // Glance: the badge on the place is enough.
        }
        checkDigest()
    }

    /** Show the daily digest once a day after its time, if anything was collected. */
    fun checkDigest() {
        val settings = store.ladder
        val now = LocalTime.now()
        val today = LocalDate.now().toEpochDay()
        val count = store.digestCount
        if (Ladder.digestDue(settings, today, now.hour * 60 + now.minute, store.lastDigestDay, count)) {
            notifier.postDigest(count)
            store.lastDigestDay = today
            store.digestCount = 0
        }
    }

    /** "Later" on a tap on the shoulder: no interruptions (except alarms) for 10 minutes. */
    fun snooze(minutes: Int = 10) {
        store.ladder = store.ladder.copy(snoozedUntilMs = System.currentTimeMillis() + minutes * 60_000L)
    }
}
