package io.mellowmesh.app.ui.components

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.app.ui.theme.rememberReduceMotion

/**
 * A machine's health: the answering, resting and not-asked dots, plus an
 * amber ring for an address that no longer leads to the hub and a rose dot
 * for a refused key.
 */
@Composable
fun MachineDot(health: io.mellowmesh.core.MachineHealth, size: Dp = 12.dp, modifier: Modifier = Modifier) {
    when (health) {
        io.mellowmesh.core.MachineHealth.ANSWERS -> MachineDot(true, size, modifier)
        io.mellowmesh.core.MachineHealth.RESTING -> MachineDot(false, size, modifier)
        io.mellowmesh.core.MachineHealth.UNKNOWN -> MachineDot(null, size, modifier)
        io.mellowmesh.core.MachineHealth.ADDRESS_LOST -> Canvas(
            modifier.size(size * 1.8f).semantics { contentDescription = "Address lost" },
        ) {
            drawCircle(Mesh.AmberTint, radius = size.toPx() / 2f)
            drawCircle(Mesh.Amber, radius = size.toPx() / 2f, style = Stroke(width = 2.dp.toPx()))
        }
        io.mellowmesh.core.MachineHealth.KEY_REFUSED -> Canvas(
            modifier.size(size * 1.8f).semantics { contentDescription = "Key refused" },
        ) { drawCircle(Mesh.Rose, radius = size.toPx() / 2f) }
    }
}

/**
 * A machine's state at a glance: green with a ring pulsing outward when it
 * answers, grey inside a still dashed ring when it rests, hollow when we
 * have not asked yet.
 */
@Composable
fun MachineDot(reachable: Boolean?, size: Dp = 12.dp, modifier: Modifier = Modifier) {
    val still = rememberReduceMotion()
    val t = rememberInfiniteTransition(label = "machine")
    val p by t.animateFloat(0f, 1f, infiniteRepeatable(tween(1800, easing = LinearEasing)), label = "pulse")
    val label = when (reachable) {
        true -> "Answers"
        false -> "Resting"
        null -> "Not checked yet"
    }
    Canvas(modifier.size(size * 1.8f).semantics { contentDescription = label }) {
        val r = size.toPx() / 2f
        when (reachable) {
            true -> {
                if (!still) {
                    drawCircle(Mesh.Green.copy(alpha = (1f - p) * 0.45f), radius = r * (1f + 0.8f * p))
                }
                drawCircle(Mesh.Green, radius = r)
            }
            false -> {
                drawCircle(Color(0xFF9A98A8), radius = r * 0.8f)
                drawCircle(
                    Color(0xFF6F6D86),
                    radius = r,
                    style = Stroke(width = 1.5.dp.toPx(), pathEffect = PathEffect.dashPathEffect(floatArrayOf(4f, 4f))),
                )
            }
            null -> drawCircle(Mesh.Ink3, radius = r * 0.85f, style = Stroke(width = 1.5.dp.toPx()))
        }
    }
}
