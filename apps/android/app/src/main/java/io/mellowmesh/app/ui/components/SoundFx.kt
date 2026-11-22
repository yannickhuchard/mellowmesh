package io.mellowmesh.app.ui.components

import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import java.util.concurrent.Executors
import kotlin.math.PI
import kotlin.math.exp
import kotlin.math.sin

/**
 * The sound family of the design language: short, soft, warm sine notes
 * (a marimba-like envelope), made in code. Rare by design: one cue per
 * meaningful moment, never one per message in a busy room. Silent when the
 * phone is on silent or vibrate, and when the person turned sounds off.
 */
object SoundFx {
    enum class Cue(internal val notes: List<Pair<Double, Double>>) {
        /** You sent: a light tick. */
        SENT(listOf(1318.5 to 0.05)),

        /** A Mellow answered in the open conversation: two soft notes. */
        RECEIVED(listOf(784.0 to 0.09, 1046.5 to 0.12)),

        /** Something needs you: a gentle rising pair. */
        NEEDS_YOU(listOf(659.3 to 0.10, 880.0 to 0.16)),

        /** A Mellow joined a group: a pop. */
        JOINED(listOf(523.3 to 0.05, 1046.5 to 0.07)),

        /** A journey step is done: a chime. */
        STEP_DONE(listOf(1046.5 to 0.12, 1318.5 to 0.12, 1568.0 to 0.22)),
    }

    private const val RATE = 44_100
    private val player = Executors.newSingleThreadExecutor { r -> Thread(r, "mesh-sound").apply { isDaemon = true } }
    private val cache = HashMap<Cue, ShortArray>()

    fun play(context: Context, cue: Cue) {
        val app = context.applicationContext
        val store = (app as? io.mellowmesh.app.MellowMeshApp)?.store ?: return
        if (!store.sounds) return
        val audio = app.getSystemService(Context.AUDIO_SERVICE) as? AudioManager ?: return
        if (audio.ringerMode != AudioManager.RINGER_MODE_NORMAL) return
        player.execute {
            runCatching {
                val pcm = synchronized(cache) { cache.getOrPut(cue) { render(cue) } }
                val track = AudioTrack.Builder()
                    .setAudioAttributes(
                        AudioAttributes.Builder()
                            .setUsage(AudioAttributes.USAGE_ASSISTANCE_SONIFICATION)
                            .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                            .build(),
                    )
                    .setAudioFormat(
                        AudioFormat.Builder()
                            .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                            .setSampleRate(RATE)
                            .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                            .build(),
                    )
                    .setTransferMode(AudioTrack.MODE_STATIC)
                    .setBufferSizeInBytes(pcm.size * 2)
                    .build()
                track.write(pcm, 0, pcm.size)
                track.play()
                Thread.sleep(pcm.size * 1000L / RATE + 60)
                track.release()
            }
        }
    }

    /** Notes overlap a little (each starts at 80 % of the previous one), with a soft attack and a warm decay. */
    internal fun render(cue: Cue): ShortArray {
        val tail = 0.12
        var start = 0.0
        val starts = cue.notes.map { (_, d) -> start.also { start += d * 0.8 } }
        val total = (starts.last() + cue.notes.last().second + tail) * RATE
        val out = DoubleArray(total.toInt() + 1)
        cue.notes.forEachIndexed { i, (f, d) ->
            val s0 = (starts[i] * RATE).toInt()
            val n = ((d + tail) * RATE).toInt()
            for (k in 0 until n) {
                val t = k.toDouble() / RATE
                val attack = (t / 0.008).coerceAtMost(1.0)
                val env = attack * exp(-t / (d * 0.55 + 0.02))
                // A touch of the octave makes it rounder, like a mallet.
                val v = sin(2 * PI * f * t) + 0.18 * sin(4 * PI * f * t)
                val idx = s0 + k
                if (idx < out.size) out[idx] += 0.16 * env * v
            }
        }
        return ShortArray(out.size) { (out[it].coerceIn(-1.0, 1.0) * Short.MAX_VALUE).toInt().toShort() }
    }
}
