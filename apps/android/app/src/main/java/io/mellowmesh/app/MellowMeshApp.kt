package io.mellowmesh.app

import android.app.Application
import android.content.Context
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import io.mellowmesh.app.data.ConfigStore
import io.mellowmesh.app.data.MeshRepository
import io.mellowmesh.app.notify.InterruptionCenter
import io.mellowmesh.app.notify.Notifier
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob

/** Application: owns the few long-lived objects (no DI framework). */
class MellowMeshApp : Application() {
    val appScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    lateinit var store: ConfigStore
        private set
    lateinit var repo: MeshRepository
        private set
    lateinit var notifier: Notifier
        private set
    lateinit var interruptions: InterruptionCenter
        private set

    override fun onCreate() {
        super.onCreate()
        store = ConfigStore(this)
        repo = MeshRepository(store, appScope)
        notifier = Notifier(this)
        notifier.createChannels()
        interruptions = InterruptionCenter(repo, store, notifier, appScope)
        interruptions.start()

        ProcessLifecycleOwner.get().lifecycle.addObserver(object : DefaultLifecycleObserver {
            override fun onStart(owner: LifecycleOwner) {
                repo.setForeground(true)
                interruptions.checkDigest()
            }

            override fun onStop(owner: LifecycleOwner) {
                repo.setForeground(false)
            }
        })
    }
}

/** Shortcut from any Context (Activity, Composable's LocalContext) to the app object. */
val Context.meshApp: MellowMeshApp get() = applicationContext as MellowMeshApp
