package io.mellowmesh.app

import android.content.Intent
import android.graphics.Color
import android.os.Bundle
import androidx.activity.SystemBarStyle
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.mutableStateOf
import androidx.fragment.app.FragmentActivity
import io.mellowmesh.app.ui.AppRoot
import io.mellowmesh.app.ui.theme.MellowMeshTheme

/**
 * Single activity. It is a FragmentActivity because BiometricPrompt needs
 * one; everything on screen is Compose.
 */
class MainActivity : FragmentActivity() {
    /** A route asked for by a notification ("todo", "settings", "conversation/<id>"). */
    private val pendingRoute = mutableStateOf<String?>(null)

    /** A `mellowmesh-pair:` link opened from elsewhere. */
    private val pendingLink = mutableStateOf<String?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.light(Color.TRANSPARENT, Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.light(Color.TRANSPARENT, Color.TRANSPARENT),
        )
        super.onCreate(savedInstanceState)
        readIntent(intent)
        setContent {
            MellowMeshTheme {
                AppRoot(
                    pendingRoute = pendingRoute.value,
                    onRouteConsumed = { pendingRoute.value = null },
                    pendingLink = pendingLink.value,
                    onLinkConsumed = { pendingLink.value = null },
                )
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        readIntent(intent)
    }

    private fun readIntent(intent: Intent?) {
        if (intent == null) return
        intent.getStringExtra(EXTRA_ROUTE)?.let { pendingRoute.value = it }
        val data = intent.data
        if (intent.action == Intent.ACTION_VIEW && data != null && data.scheme.equals("mellowmesh-pair", ignoreCase = true)) {
            pendingLink.value = data.toString()
        }
    }

    companion object {
        const val EXTRA_ROUTE = "io.mellowmesh.app.ROUTE"
    }
}
