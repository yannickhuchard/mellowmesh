package io.mellowmesh.app.ui.shops

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.mellowmesh.app.R
import io.mellowmesh.app.data.friendly
import io.mellowmesh.app.meshApp
import io.mellowmesh.app.ui.components.ErrorNote
import io.mellowmesh.app.ui.components.Gap
import io.mellowmesh.app.ui.components.HelperBlob
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.components.Mello
import io.mellowmesh.app.ui.components.NameWithTag
import io.mellowmesh.app.ui.components.TabHeader
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.Mood
import io.mellowmesh.core.ShopWindow
import io.mellowmesh.core.Shops
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue

/** Window-shopping for helpers, by floor (Popular · Study · Writing · Money · Work · Everything). */
@Composable
fun ShopsScreen(onHelp: () -> Unit, onChat: (String) -> Unit, onHelper: (String) -> Unit) {
    val app = LocalContext.current.meshApp
    val agents by app.repo.agents.collectAsStateWithLifecycle()
    val home by app.repo.home.collectAsStateWithLifecycle()
    var error by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(Unit) {
        try {
            app.repo.loadAgents()
            error = null
        } catch (e: Exception) {
            error = e.friendly()
        }
    }
    val floors = Shops.floors(agents, home?.me ?: app.repo.me)
    val moods = home?.helpers.orEmpty().associate { it.id to it.mood }

    Column(Modifier.fillMaxSize().background(Mesh.Paper)) {
        TabHeader(title = stringResource(R.string.shops_title), onHelp = onHelp, subtitle = stringResource(R.string.shops_intro))
        error?.let { ErrorNote(it, modifier = Modifier.padding(horizontal = 16.dp)) }
        if (agents.isEmpty()) {
            Column(Modifier.fillMaxSize().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                Gap(40.dp)
                Mello(size = 110.dp)
                Gap()
                Text(stringResource(R.string.shops_empty), color = Mesh.Ink2, style = MaterialTheme.typography.bodyLarge)
            }
        } else {
            LazyColumn(contentPadding = PaddingValues(bottom = 24.dp)) {
                for (floor in floors) {
                    item(key = "floor-" + floor.floor.name) {
                        Column(Modifier.padding(start = 20.dp, end = 20.dp, top = 18.dp, bottom = 8.dp)) {
                            Text(floor.floor.title, style = MaterialTheme.typography.headlineSmall, color = Mesh.Ink)
                            Text(floor.floor.blurb, style = MaterialTheme.typography.bodyMedium, color = Mesh.Ink2)
                        }
                    }
                    item(key = "row-" + floor.floor.name) {
                        LazyRow(
                            contentPadding = PaddingValues(horizontal = 16.dp),
                            horizontalArrangement = Arrangement.spacedBy(12.dp),
                        ) {
                            items(floor.shops, key = { floor.floor.name + it.agent.id }) { shop ->
                                ShopWindowCard(
                                    shop = shop,
                                    mood = moods[shop.agent.id] ?: Mood.IDLE,
                                    onOpen = { onHelper(shop.agent.id) },
                                    onHello = { shop.conversationId?.let(onChat) },
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun ShopWindowCard(shop: ShopWindow, mood: Mood, onOpen: () -> Unit, onHello: () -> Unit) {
    MeshCard(Modifier.width(220.dp).clickable(onClick = onOpen)) {
        Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
            HelperBlob(Mesh.accent(shop.accent), mood, 88.dp, description = "AI helper ${shop.name}")
        }
        Gap(8.dp)
        NameWithTag(shop.name, isHelper = true)
        // Where this helper comes from: its owner, and whether it is only
        // a demo example — so no helper is ever a mystery.
        if (shop.agent.capabilities.contains("demo")) {
            Text(
                stringResource(R.string.demo_badge),
                style = MaterialTheme.typography.bodySmall,
                color = Mesh.Amber,
                maxLines = 2,
            )
        } else if (shop.agent.owner.isNotEmpty()) {
            Text(
                stringResource(R.string.from_owner, shop.agent.owner),
                style = MaterialTheme.typography.bodySmall,
                color = Mesh.Ink2,
                maxLines = 1,
            )
        }
        if (shop.agent.capabilities.isNotEmpty()) {
            Text(
                shop.agent.capabilities.take(3).joinToString(" · "),
                style = MaterialTheme.typography.bodySmall,
                color = Mesh.Ink2,
                maxLines = 1,
            )
        }
        Gap(6.dp)
        for (example in shop.trySaying) {
            Text(stringResource(R.string.try_saying, example), style = MaterialTheme.typography.bodySmall, color = Mesh.Indigo)
        }
        Gap(8.dp)
        OutlinedButton(onClick = onHello, enabled = shop.conversationId != null, modifier = Modifier.fillMaxWidth()) {
            Text(stringResource(R.string.say_hello), color = Mesh.Ink)
        }
    }
}
