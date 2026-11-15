package io.mellowmesh.app.ui.components

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.keyframes
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.size
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.lerp
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.app.ui.theme.rememberReduceMotion
import io.mellowmesh.core.Mood
import kotlin.math.PI
import kotlin.math.sin

/**
 * A helper drawn as a soft clay blob (design language §2): one rounded body,
 * glossy highlight, pink cheeks, two eyes, and a mood. Animations (blink,
 * thinking dots, progress spinner, waving hand) become a still pose when the
 * user turned animations off.
 */
@Composable
fun HelperBlob(
    color: Color,
    mood: Mood,
    size: Dp,
    modifier: Modifier = Modifier,
    progress: Float? = null,
    description: String? = null,
) {
    val reduceMotion = rememberReduceMotion()
    val transition = rememberInfiniteTransition(label = "blob")
    val blink by transition.animateFloat(
        initialValue = 1f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(
            animation = keyframes<Float> {
                durationMillis = 4200
                1f at 0
                1f at 3900
                0.1f at 4000
                1f at 4120
            },
        ),
        label = "blink",
    )
    val phase by transition.animateFloat(
        initialValue = 0f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(animation = tween(durationMillis = 1200, easing = LinearEasing)),
        label = "phase",
    )
    val semanticsModifier = if (description != null) {
        Modifier.semantics { contentDescription = description }
    } else {
        Modifier
    }
    Canvas(modifier = modifier.size(size).then(semanticsModifier)) {
        drawHelperBlob(
            base = color,
            mood = mood,
            eyeOpen = if (reduceMotion) 1f else blink,
            phase = if (reduceMotion) 0.25f else phase,
            progress = progress,
        )
    }
}

/** Mello, the violet guide. */
@Composable
fun Mello(size: Dp, modifier: Modifier = Modifier, mood: Mood = Mood.IDLE) {
    HelperBlob(color = Mesh.Violet, mood = mood, size = size, modifier = modifier, description = "Mello, your guide")
}

/** The blob body path of the 100×100 design box, scaled by `s`. */
private fun blobPath(s: Float): Path = Path().apply {
    moveTo(50f * s, 9f * s)
    cubicTo(77f * s, 9f * s, 92f * s, 25f * s, 92f * s, 53f * s)
    cubicTo(92f * s, 80f * s, 76f * s, 93f * s, 50f * s, 93f * s)
    cubicTo(24f * s, 93f * s, 8f * s, 80f * s, 8f * s, 53f * s)
    cubicTo(8f * s, 25f * s, 23f * s, 9f * s, 50f * s, 9f * s)
    close()
}

/**
 * Draw a helper blob filling the current draw area (square-fitted).
 * `eyeOpen` 0..1 (blink), `phase` 0..1 looping (dots, spinner, wave).
 * Usable from any Canvas (map, roadmap lane heads).
 */
fun DrawScope.drawHelperBlob(
    base: Color,
    mood: Mood,
    eyeOpen: Float = 1f,
    phase: Float = 0.25f,
    progress: Float? = null,
    areaSize: Size = size,
) {
    val w = areaSize.minDimension
    val inset = w * 0.09f
    val s = (w - 2 * inset) / 100f
    val left = (areaSize.width - w) / 2f + inset
    val top = (areaSize.height - w) / 2f + inset
    fun p(x: Float, y: Float) = Offset(x * s, y * s)
    val body = if (mood == Mood.ASLEEP) Mesh.Sleep else base
    val ink = Mesh.Ink

    translate(left = left, top = top) {
        // Soft contact shadow.
        drawOval(color = Color.Black.copy(alpha = 0.10f), topLeft = p(22f, 90f), size = Size(56f * s, 8f * s))

        // "Needs you": soft amber pulse, never a siren.
        if (mood == Mood.WAITING) {
            drawCircle(
                color = Mesh.Amber.copy(alpha = 0.35f * (1f - phase)),
                radius = (46f + 8f * phase) * s,
                center = p(50f, 52f),
                style = Stroke(width = 3f * s),
            )
        }

        // Working: progress ring (or a calm spinner when progress is unknown).
        if (mood == Mood.WORKING) {
            val ringTopLeft = p(-2f, 3f)
            val ringSize = Size(104f * s, 104f * s)
            drawArc(
                color = Mesh.Indigo.copy(alpha = 0.15f),
                startAngle = 0f, sweepAngle = 360f, useCenter = false,
                topLeft = ringTopLeft, size = ringSize,
                style = Stroke(width = 4f * s),
            )
            if (progress != null) {
                drawArc(
                    color = Mesh.Indigo,
                    startAngle = -90f, sweepAngle = 360f * progress.coerceIn(0f, 1f), useCenter = false,
                    topLeft = ringTopLeft, size = ringSize,
                    style = Stroke(width = 4f * s, cap = StrokeCap.Round),
                )
            } else {
                drawArc(
                    color = Mesh.Indigo,
                    startAngle = phase * 360f - 90f, sweepAngle = 80f, useCenter = false,
                    topLeft = ringTopLeft, size = ringSize,
                    style = Stroke(width = 4f * s, cap = StrokeCap.Round),
                )
            }
        }

        // Body with a clay radial gradient.
        drawPath(
            path = blobPath(s),
            brush = Brush.radialGradient(
                colors = listOf(lerp(body, Color.White, 0.38f), body, lerp(body, Color.Black, 0.16f)),
                center = p(36f, 30f),
                radius = 78f * s,
            ),
        )

        // Waving hand (needs you).
        if (mood == Mood.WAITING) {
            val angle = sin(phase * 2f * PI.toFloat()) * 18f
            rotate(degrees = angle, pivot = p(84f, 58f)) {
                drawCircle(color = lerp(body, Color.White, 0.15f), radius = 8f * s, center = p(93f, 38f))
                drawCircle(color = Color.White.copy(alpha = 0.35f), radius = 3f * s, center = p(91f, 35f))
            }
        }

        // Gloss highlight.
        drawOval(color = Color.White.copy(alpha = 0.45f), topLeft = p(25f, 17f), size = Size(21f * s, 11f * s))

        // Cheeks.
        drawOval(color = Mesh.Cheek.copy(alpha = 0.45f), topLeft = p(20f, 59f), size = Size(12f * s, 7f * s))
        drawOval(color = Mesh.Cheek.copy(alpha = 0.45f), topLeft = p(68f, 59f), size = Size(12f * s, 7f * s))

        val stroke = Stroke(width = 3f * s, cap = StrokeCap.Round)
        when (mood) {
            Mood.DONE -> {
                // Happy closed eyes: ^ ^
                drawArc(ink, 200f, 140f, false, topLeft = p(33f, 46f), size = Size(12f * s, 10f * s), style = stroke)
                drawArc(ink, 200f, 140f, false, topLeft = p(55f, 46f), size = Size(12f * s, 10f * s), style = stroke)
                drawArc(ink, 20f, 140f, false, topLeft = p(43f, 58f), size = Size(14f * s, 9f * s), style = stroke)
            }
            Mood.ASLEEP -> {
                drawArc(ink.copy(alpha = 0.7f), 20f, 140f, false, topLeft = p(33f, 46f), size = Size(12f * s, 8f * s), style = stroke)
                drawArc(ink.copy(alpha = 0.7f), 20f, 140f, false, topLeft = p(55f, 46f), size = Size(12f * s, 8f * s), style = stroke)
                // A small "z".
                val z = Path().apply {
                    moveTo(76f * s, 12f * s); lineTo(86f * s, 12f * s); lineTo(76f * s, 22f * s); lineTo(86f * s, 22f * s)
                }
                drawPath(z, color = Mesh.Ink3, style = Stroke(width = 2.5f * s, cap = StrokeCap.Round))
            }
            Mood.THINKING -> {
                // Eyes look up, three dots float above.
                drawEyes(s, eyeOpen, cy = 46f, pupilDy = -3f)
                drawCircle(ink, radius = 2.6f * s, center = p(50f, 66f), style = Stroke(width = 2.2f * s))
                for (i in 0 until 3) {
                    val alpha = 0.25f + 0.75f * (((phase * 3f) - i).let { if (it in 0f..1f) 1f - it else 0f })
                    drawCircle(Mesh.Ink2.copy(alpha = alpha.coerceIn(0.25f, 1f)), radius = 3.2f * s, center = p(66f + i * 9f, 6f - i * 2f))
                }
            }
            else -> {
                drawEyes(s, eyeOpen, cy = 50f, pupilDy = 0f)
                drawArc(ink, 25f, 130f, false, topLeft = p(43f, 58f), size = Size(14f * s, 9f * s), style = stroke)
            }
        }
    }
}

private fun DrawScope.drawEyes(s: Float, eyeOpen: Float, cy: Float, pupilDy: Float) {
    val h = (11f * eyeOpen).coerceAtLeast(1.4f)
    for (cx in listOf(39f, 61f)) {
        drawOval(
            color = Mesh.Ink,
            topLeft = Offset((cx - 3.6f) * s, (cy - h / 2f + pupilDy) * s),
            size = Size(7.2f * s, h * s),
        )
        if (eyeOpen > 0.5f) {
            drawCircle(Color.White, radius = 1.4f * s, center = Offset((cx + 1.2f) * s, (cy - 2.5f + pupilDy) * s))
        }
    }
}
