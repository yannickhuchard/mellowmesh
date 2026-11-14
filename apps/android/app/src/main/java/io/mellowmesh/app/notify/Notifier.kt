package io.mellowmesh.app.notify

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.content.ContextCompat
import io.mellowmesh.app.MainActivity
import io.mellowmesh.app.R
import io.mellowmesh.core.Interruption
import io.mellowmesh.core.Level

/**
 * Android notifications for the interruption ladder: one channel per level
 * (so people can also tune them in system settings), plus the daily digest.
 * Every notification carries a "Quieter…" action that opens the ladder settings.
 */
class Notifier(private val context: Context) {
    private val manager = NotificationManagerCompat.from(context)

    fun createChannels() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
        val nm = context.getSystemService(NotificationManager::class.java) ?: return
        val channels = listOf(
            NotificationChannel(CH_WHISPER, context.getString(R.string.channel_whisper), NotificationManager.IMPORTANCE_LOW),
            NotificationChannel(CH_TAP, context.getString(R.string.channel_tap), NotificationManager.IMPORTANCE_DEFAULT),
            NotificationChannel(CH_KNOCK, context.getString(R.string.channel_knock), NotificationManager.IMPORTANCE_HIGH),
            NotificationChannel(CH_ALARM, context.getString(R.string.channel_alarm), NotificationManager.IMPORTANCE_HIGH),
            NotificationChannel(CH_DIGEST, context.getString(R.string.channel_digest), NotificationManager.IMPORTANCE_LOW),
        )
        nm.createNotificationChannels(channels)
    }

    private fun channelFor(level: Level): String = when (level) {
        Level.GLANCE, Level.WHISPER -> CH_WHISPER
        Level.TAP -> CH_TAP
        Level.KNOCK -> CH_KNOCK
        Level.ALARM -> CH_ALARM
    }

    private fun canPost(): Boolean =
        Build.VERSION.SDK_INT < 33 ||
            ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) == PackageManager.PERMISSION_GRANTED

    private fun openIntent(route: String, requestCode: Int): PendingIntent {
        val intent = Intent(context, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
            .putExtra(MainActivity.EXTRA_ROUTE, route)
        return PendingIntent.getActivity(
            context,
            requestCode,
            intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
    }

    /** Post a Knock / Alarm (or other) notification for an interruption. */
    fun post(interruption: Interruption, level: Level) {
        if (!canPost()) return
        val id = interruption.key.hashCode()
        val route = when {
            interruption.decisionId != null -> "todo"
            interruption.conversationId != null -> "conversation/" + android.net.Uri.encode(interruption.conversationId)
            else -> "plaza"
        }
        val builder = NotificationCompat.Builder(context, channelFor(level))
            .setSmallIcon(R.drawable.ic_launcher_foreground)
            .setContentTitle(interruption.title)
            .setContentText(interruption.body.ifBlank { null })
            .setStyle(NotificationCompat.BigTextStyle().bigText(interruption.body))
            .setAutoCancel(true)
            .setContentIntent(openIntent(route, id))
            .addAction(0, context.getString(R.string.quieter), openIntent("settings", id + 1))
            .setPriority(if (level.rank >= Level.KNOCK.rank) NotificationCompat.PRIORITY_HIGH else NotificationCompat.PRIORITY_LOW)
            .setCategory(if (interruption.decisionId != null) NotificationCompat.CATEGORY_REMINDER else NotificationCompat.CATEGORY_MESSAGE)
        if (level == Level.ALARM) builder.setOngoing(true)
        try {
            manager.notify(id, builder.build())
        } catch (_: SecurityException) {
            // Permission revoked between the check and the call: nothing to do.
        }
    }

    fun postDigest(count: Int) {
        if (!canPost() || count <= 0) return
        val builder = NotificationCompat.Builder(context, CH_DIGEST)
            .setSmallIcon(R.drawable.ic_launcher_foreground)
            .setContentTitle(context.getString(R.string.digest_title))
            .setContentText(context.getString(R.string.digest_body, count))
            .setAutoCancel(true)
            .setContentIntent(openIntent("plaza", DIGEST_ID))
            .addAction(0, context.getString(R.string.quieter), openIntent("settings", DIGEST_ID + 1))
        try {
            manager.notify(DIGEST_ID, builder.build())
        } catch (_: SecurityException) {
        }
    }

    fun cancel(key: String) = manager.cancel(key.hashCode())

    companion object {
        const val CH_WHISPER = "ladder_whisper"
        const val CH_TAP = "ladder_tap"
        const val CH_KNOCK = "ladder_knock"
        const val CH_ALARM = "ladder_alarm"
        const val CH_DIGEST = "digest"
        private const val DIGEST_ID = 4242
    }
}
