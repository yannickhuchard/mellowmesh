package io.mellowmesh.app.ui.components

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.spring
import androidx.compose.animation.core.tween
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.dp
import io.mellowmesh.app.ui.theme.rememberReduceMotion

/**
 * The motion language: what appears comes from somewhere. [RiseIn] lifts
 * a message up from below with a soft spring, leaning in from the side it
 * comes from; [HopIn] makes a Mellow hop into its place. With *reduce
 * motion*, both become a short fade.
 */
@Composable
fun RiseIn(enabled: Boolean, fromStart: Boolean, modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    val still = rememberReduceMotion()
    val p = remember { Animatable(if (enabled) 0f else 1f) }
    val dy = with(LocalDensity.current) { 18.dp.toPx() }
    val dx = with(LocalDensity.current) { 10.dp.toPx() }
    LaunchedEffect(enabled) {
        if (enabled && p.value < 1f) {
            p.animateTo(1f, if (still) tween(160) else spring(dampingRatio = 0.62f, stiffness = Spring.StiffnessMediumLow))
        }
    }
    Box(
        modifier.graphicsLayer {
            val v = p.value
            alpha = v.coerceIn(0f, 1f)
            if (!still) {
                translationY = (1f - v) * dy
                translationX = (1f - v) * (if (fromStart) -dx else dx)
                val s = 0.96f + 0.04f * v
                scaleX = s
                scaleY = s
            }
        },
    ) { content() }
}

/** A Mellow hops into its place (rosters, a group it just joined). [order] staggers a row. */
@Composable
fun HopIn(order: Int = 0, modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    val still = rememberReduceMotion()
    val p = remember { Animatable(0f) }
    val dy = with(LocalDensity.current) { 14.dp.toPx() }
    LaunchedEffect(Unit) {
        kotlinx.coroutines.delay((order.coerceAtMost(10) * 45L))
        p.animateTo(1f, if (still) tween(160) else spring(dampingRatio = 0.5f, stiffness = Spring.StiffnessMedium))
    }
    Box(
        modifier.graphicsLayer {
            val v = p.value
            alpha = v.coerceIn(0f, 1f)
            if (!still) {
                translationY = (1f - v) * dy
                val s = 0.6f + 0.4f * v
                scaleX = s
                scaleY = s
            }
        },
    ) { content() }
}
