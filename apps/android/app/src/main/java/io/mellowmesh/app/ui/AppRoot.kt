package io.mellowmesh.app.ui

import android.Manifest
import android.net.Uri
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Email
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.ShoppingCart
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationBarItemDefaults
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.currentBackStackEntryAsState
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import io.mellowmesh.app.R
import io.mellowmesh.app.meshApp
import io.mellowmesh.app.ui.calls.CallScreen
import io.mellowmesh.app.ui.calls.CallsScreen
import io.mellowmesh.app.notify.InAppNotice
import io.mellowmesh.app.ui.board.BoardScreen
import io.mellowmesh.app.ui.chats.ChatsScreen
import io.mellowmesh.app.ui.chats.ConversationScreen
import io.mellowmesh.app.ui.components.CountBadge
import io.mellowmesh.app.ui.components.HelperBlob
import io.mellowmesh.app.ui.components.MeshCard
import io.mellowmesh.app.ui.help.HelpScreen
import io.mellowmesh.app.ui.helper.HelperScreen
import io.mellowmesh.app.ui.network.NetworkScreen
import io.mellowmesh.app.ui.network.SpaceEditScreen
import io.mellowmesh.app.ui.network.TeamScreen
import io.mellowmesh.app.ui.plaza.PlazaScreen
import io.mellowmesh.app.ui.roadmap.RoadmapScreen
import io.mellowmesh.app.ui.settings.SettingsScreen
import io.mellowmesh.app.ui.shops.ShopsScreen
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.app.ui.todo.TodoScreen
import io.mellowmesh.app.ui.welcome.WelcomeScreen
import io.mellowmesh.core.Level
import io.mellowmesh.core.Mood
import io.mellowmesh.core.Place
import kotlinx.coroutines.delay

/** Route builders; every free-form argument is URL-encoded. */
object Routes {
    const val PLAZA = "plaza"
    const val CHATS = "chats"
    const val TODO = "todo"
    const val SHOPS = "shops"
    const val NETWORK = "network"
    const val SETTINGS = "settings"
    const val HELP = "help"
    const val CALLS = "calls"
    fun call(id: String) = "call/" + Uri.encode(id)
    fun conversation(id: String) = "conversation/" + Uri.encode(id)
    fun helper(id: String) = "helper/" + Uri.encode(id)
    fun board(scope: String) = "board/" + Uri.encode(scope)
    fun roadmap(scope: String) = "roadmap/" + Uri.encode(scope)
    fun space(id: String) = "space/" + Uri.encode(id)
    fun team(id: String) = "team/" + Uri.encode(id)
    const val NEW_SPACE = "new"
}

@Composable
fun AppRoot(
    pendingRoute: String?,
    onRouteConsumed: () -> Unit,
    pendingLink: String?,
    onLinkConsumed: () -> Unit,
) {
    val app = LocalContext.current.meshApp
    val config by app.repo.config.collectAsStateWithLifecycle()
    if (config == null) {
        WelcomeScreen(pendingLink = pendingLink, onLinkConsumed = onLinkConsumed)
    } else {
        MainScaffold(pendingRoute = pendingRoute, onRouteConsumed = onRouteConsumed)
    }
}

private data class Tab(val route: String, val label: Int, val icon: ImageVector)

private val Tabs = listOf(
    Tab(Routes.PLAZA, R.string.tab_plaza, Icons.Filled.Home),
    Tab(Routes.CHATS, R.string.tab_chats, Icons.Filled.Email),
    Tab(Routes.TODO, R.string.tab_todo, Icons.Filled.CheckCircle),
    Tab(Routes.SHOPS, R.string.tab_shops, Icons.Filled.ShoppingCart),
)

private fun NavHostController.goTab(route: String) {
    navigate(route) {
        popUpTo(Routes.PLAZA) { saveState = true }
        launchSingleTop = true
        restoreState = true
    }
}

@Composable
private fun MainScaffold(pendingRoute: String?, onRouteConsumed: () -> Unit) {
    val app = LocalContext.current.meshApp
    val nav = rememberNavController()
    val backStack by nav.currentBackStackEntryAsState()
    val current = backStack?.destination?.route
    val home by app.repo.home.collectAsStateWithLifecycle()
    val connected by app.repo.connected.collectAsStateWithLifecycle()

    // Android 13+: ask once for permission to show notifications.
    val permissionLauncher = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { }
    LaunchedEffect(Unit) {
        if (Build.VERSION.SDK_INT >= 33) permissionLauncher.launch(Manifest.permission.POST_NOTIFICATIONS)
        app.repo.refreshHome()
    }

    LaunchedEffect(pendingRoute) {
        if (pendingRoute != null) {
            nav.navigate(pendingRoute) { launchSingleTop = true }
            onRouteConsumed()
        }
    }

    // Whisper strips and taps on the shoulder.
    var notice by remember { mutableStateOf<InAppNotice?>(null) }
    LaunchedEffect(Unit) {
        app.interruptions.notices.collect { notice = it }
    }
    LaunchedEffect(notice) {
        val n = notice
        if (n != null && n.level == Level.WHISPER) {
            delay(4000)
            if (notice == n) notice = null
        }
    }

    val showBar = Tabs.any { it.route == current }
    Scaffold(
        containerColor = Mesh.Paper,
        bottomBar = {
            if (showBar) {
                NavigationBar(containerColor = Mesh.Card) {
                    Tabs.forEach { tab ->
                        val badge = when (tab.route) {
                            Routes.TODO -> home?.needsYou ?: 0
                            Routes.CHATS -> home?.conversations?.sumOf { it.unread } ?: 0
                            else -> 0
                        }
                        NavigationBarItem(
                            selected = current == tab.route,
                            onClick = { nav.goTab(tab.route) },
                            icon = {
                                Box {
                                    Icon(tab.icon, contentDescription = null)
                                    if (badge > 0) CountBadge(badge, Modifier.align(Alignment.TopEnd).padding(start = 14.dp))
                                }
                            },
                            label = { Text(stringResource(tab.label)) },
                            colors = NavigationBarItemDefaults.colors(
                                selectedIconColor = Mesh.Ink,
                                selectedTextColor = Mesh.Ink,
                                indicatorColor = Mesh.Paper,
                                unselectedIconColor = Mesh.Ink3,
                                unselectedTextColor = Mesh.Ink3,
                            ),
                        )
                    }
                }
            }
        },
    ) { padding ->
        Box(
            Modifier
                .fillMaxSize()
                .padding(bottom = padding.calculateBottomPadding()),
        ) {
            NavHost(navController = nav, startDestination = Routes.PLAZA, modifier = Modifier.fillMaxSize()) {
                val help = { nav.navigate(Routes.HELP) }
                composable(Routes.PLAZA) {
                    PlazaScreen(
                        onHelp = help,
                        onPlace = { place ->
                            when (place) {
                                Place.TODO_DESK -> nav.goTab(Routes.TODO)
                                Place.SHOPS -> nav.goTab(Routes.SHOPS)
                                Place.HELP_DESK -> nav.navigate(Routes.HELP)
                                Place.TEAMS -> nav.navigate(Routes.NETWORK)
                                else -> nav.goTab(Routes.CHATS)
                            }
                        },
                        onHelper = { nav.navigate(Routes.helper(it)) },
                        onBoard = { nav.navigate(Routes.board("all")) },
                        onRoadmap = { nav.navigate(Routes.roadmap("all")) },
                        onNetwork = { nav.navigate(Routes.NETWORK) },
                        onSettings = { nav.navigate(Routes.SETTINGS) },
                        onCalls = { nav.navigate(Routes.CALLS) },
                        onCall = { nav.navigate(Routes.call(it)) },
                    )
                }
                composable(Routes.CALLS) {
                    CallsScreen(onBack = { nav.popBackStack() }, onCall = { nav.navigate(Routes.call(it)) })
                }
                composable("call/{id}", arguments = listOf(navArgument("id") { type = NavType.StringType })) { entry ->
                    CallScreen(
                        agentId = entry.arguments?.getString("id").orEmpty(),
                        onHangUp = { nav.popBackStack() },
                        onOpenChat = { id ->
                            nav.popBackStack()
                            nav.navigate(Routes.conversation(id))
                        },
                    )
                }
                composable(Routes.CHATS) {
                    ChatsScreen(
                        onHelp = help,
                        onOpen = { nav.navigate(Routes.conversation(it)) },
                        onFindHelpers = { nav.goTab(Routes.SHOPS) },
                    )
                }
                composable(Routes.TODO) {
                    TodoScreen(
                        onHelp = help,
                        onBoard = { nav.navigate(Routes.board("all")) },
                        onRoadmap = { nav.navigate(Routes.roadmap("mine")) },
                    )
                }
                composable(Routes.SHOPS) {
                    ShopsScreen(
                        onHelp = help,
                        onChat = { nav.navigate(Routes.conversation(it)) },
                        onHelper = { nav.navigate(Routes.helper(it)) },
                        onMake = { nav.navigate(Routes.NETWORK) },
                    )
                }
                composable(
                    "conversation/{id}",
                    arguments = listOf(navArgument("id") { type = NavType.StringType }),
                ) { entry ->
                    ConversationScreen(
                        conversationId = entry.arguments?.getString("id").orEmpty(),
                        onBack = { nav.popBackStack() },
                        onTeam = { nav.navigate(Routes.team(it)) },
                        onHelper = { nav.navigate(Routes.helper(it)) },
                    )
                }
                composable("helper/{id}", arguments = listOf(navArgument("id") { type = NavType.StringType })) { entry ->
                    val id = entry.arguments?.getString("id").orEmpty()
                    HelperScreen(
                        agentId = id,
                        onBack = { nav.popBackStack() },
                        onChat = { nav.navigate(Routes.conversation(it)) },
                        onBoard = { nav.navigate(Routes.board("agent:$id")) },
                    )
                }
                composable("board/{scope}", arguments = listOf(navArgument("scope") { type = NavType.StringType })) { entry ->
                    val scope = entry.arguments?.getString("scope") ?: "all"
                    BoardScreen(
                        scope = scope,
                        onBack = { nav.popBackStack() },
                        onRoadmap = { nav.navigate(Routes.roadmap(scope)) },
                    )
                }
                composable("roadmap/{scope}", arguments = listOf(navArgument("scope") { type = NavType.StringType })) { entry ->
                    RoadmapScreen(scope = entry.arguments?.getString("scope") ?: "all", onBack = { nav.popBackStack() })
                }
                composable(Routes.NETWORK) {
                    NetworkScreen(
                        onBack = { nav.popBackStack() },
                        onSpace = { nav.navigate(Routes.space(it)) },
                        onTeam = { nav.navigate(Routes.team(it)) },
                        onChat = { nav.navigate(Routes.conversation(it)) },
                    )
                }
                composable("space/{id}", arguments = listOf(navArgument("id") { type = NavType.StringType })) { entry ->
                    val id = entry.arguments?.getString("id").orEmpty()
                    SpaceEditScreen(spaceId = if (id == Routes.NEW_SPACE) null else id, onBack = { nav.popBackStack() })
                }
                composable("team/{id}", arguments = listOf(navArgument("id") { type = NavType.StringType })) { entry ->
                    val id = entry.arguments?.getString("id").orEmpty()
                    TeamScreen(
                        teamId = id,
                        onBack = { nav.popBackStack() },
                        onChat = { nav.navigate(Routes.conversation(it)) },
                        onBoard = { nav.navigate(Routes.board("team:$id")) },
                    )
                }
                composable(Routes.SETTINGS) {
                    SettingsScreen(onBack = { nav.popBackStack() })
                }
                composable(Routes.HELP) {
                    HelpScreen(onBack = { nav.popBackStack() })
                }
            }

            if (!connected && home != null) {
                Text(
                    stringResource(R.string.offline_banner),
                    style = MaterialTheme.typography.labelMedium,
                    color = Mesh.Ink2,
                    modifier = Modifier
                        .align(Alignment.TopCenter)
                        .statusBarsPadding()
                        .background(Mesh.AmberTint)
                        .padding(horizontal = 12.dp, vertical = 4.dp),
                )
            }

            NoticeOverlay(
                notice = notice,
                onOpen = { n ->
                    notice = null
                    val id = n.interruption.conversationId
                    if (n.interruption.decisionId != null) nav.goTab(Routes.TODO)
                    else if (id != null) nav.navigate(Routes.conversation(id))
                },
                onLater = {
                    notice = null
                    app.interruptions.snooze()
                },
                onDismiss = { notice = null },
            )
        }
    }
}

/** Whisper: a quiet strip at the top. Tap: the helper peeks from the edge with Now / Later. */
@Composable
private fun NoticeOverlay(
    notice: InAppNotice?,
    onOpen: (InAppNotice) -> Unit,
    onLater: () -> Unit,
    onDismiss: () -> Unit,
) {
    Box(Modifier.fillMaxSize()) {
        AnimatedVisibility(
            visible = notice?.level == Level.WHISPER,
            enter = slideInVertically { -it } + fadeIn(),
            exit = slideOutVertically { -it } + fadeOut(),
            modifier = Modifier.align(Alignment.TopCenter),
        ) {
            val n = notice
            if (n != null) {
                MeshCard(
                    modifier = Modifier
                        .statusBarsPadding()
                        .padding(horizontal = 16.dp, vertical = 8.dp)
                        .fillMaxWidth(),
                    contentPadding = 12.dp,
                ) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        HelperBlob(Mesh.Indigo, Mood.DONE, 32.dp)
                        Spacer(Modifier.width(10.dp))
                        Column(Modifier.weight(1f)) {
                            Text(n.interruption.title, style = MaterialTheme.typography.titleSmall, color = Mesh.Ink)
                            if (n.interruption.body.isNotBlank()) {
                                Text(n.interruption.body, style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2, maxLines = 2)
                            }
                        }
                        TextButton(onClick = { onOpen(n) }) { Text(stringResource(R.string.open), color = Mesh.Indigo) }
                    }
                }
            }
        }
        AnimatedVisibility(
            visible = notice?.level == Level.TAP,
            enter = slideInHorizontally { it } + fadeIn(),
            exit = slideOutHorizontally { it } + fadeOut(),
            modifier = Modifier.align(Alignment.BottomEnd),
        ) {
            val n = notice
            if (n != null) {
                MeshCard(
                    modifier = Modifier.padding(16.dp).width(300.dp),
                    color = Mesh.AmberTint,
                    contentPadding = 12.dp,
                ) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        HelperBlob(Mesh.Amber, Mood.WAITING, 44.dp)
                        Spacer(Modifier.width(10.dp))
                        Column(Modifier.weight(1f)) {
                            Text(n.interruption.title, style = MaterialTheme.typography.titleSmall, color = Mesh.Ink)
                            Text(n.interruption.body, style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2, maxLines = 2)
                        }
                    }
                    Row {
                        TextButton(onClick = { onOpen(n) }) { Text(stringResource(R.string.now), color = Mesh.Ink) }
                        TextButton(onClick = onLater) { Text(stringResource(R.string.later), color = Mesh.Ink2) }
                        Spacer(Modifier.weight(1f))
                        TextButton(onClick = onDismiss) { Text(stringResource(R.string.close), color = Mesh.Ink3) }
                    }
                }
            }
        }
    }
}
