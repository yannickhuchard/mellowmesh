package io.mellowmesh.app.ui.plaza

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.ExperimentalTextApi
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.sp
import io.mellowmesh.app.ui.components.drawHelperBlob
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.app.ui.theme.rememberReduceMotion
import io.mellowmesh.core.Mood
import io.mellowmesh.core.Place
import kotlin.math.hypot

/** One building on the clay diorama. */
data class MapPlace(val place: Place, val label: String, val roof: Color, val gx: Float, val gy: Float)

/** Grid positions on a 3×3 isometric board; the fountain ("You are here") is in the middle. */
fun defaultPlaces(label: (Place) -> String): List<MapPlace> = listOf(
    MapPlace(Place.HELPERS, label(Place.HELPERS), Mesh.Indigo, 0f, 0f),
    MapPlace(Place.MEETING_ROOMS, label(Place.MEETING_ROOMS), Mesh.Sky, 1f, 0f),
    MapPlace(Place.CAFE, label(Place.CAFE), Mesh.Pink, 2f, 0f),
    MapPlace(Place.TEAMS, label(Place.TEAMS), Mesh.Mint, 0f, 1f),
    MapPlace(Place.OFFICES, label(Place.OFFICES), Mesh.Ink2, 2f, 1f),
    MapPlace(Place.TODO_DESK, label(Place.TODO_DESK), Mesh.Amber, 0f, 2f),
    MapPlace(Place.SHOPS, label(Place.SHOPS), Mesh.Violet, 1f, 2f),
    MapPlace(Place.HELP_DESK, label(Place.HELP_DESK), Mesh.Green, 2f, 2f),
)

/**
 * The Plaza mall map: an isometric clay diorama drawn with Canvas. The next
 * useful stop is lit with a glowing amber walkway; buildings carry badges;
 * a helper stands at the "Your helpers" door in the mood of the busiest one.
 */
@OptIn(ExperimentalTextApi::class)
@Composable
fun PlazaMap(
    places: List<MapPlace>,
    nextStop: Place,
    badges: Map<Place, Int>,
    youAreHere: String,
    helperColor: Color,
    helperMood: Mood,
    onPlace: (Place) -> Unit,
    modifier: Modifier = Modifier,
) {
    val reduceMotion = rememberReduceMotion()
    val transition = rememberInfiniteTransition(label = "map")
    val glow by transition.animateFloat(
        initialValue = 0.35f,
        targetValue = 1f,
        animationSpec = infiniteRepeatable(tween(1400, easing = LinearEasing), RepeatMode.Reverse),
        label = "glow",
    )
    val dash by transition.animateFloat(
        initialValue = 0f,
        targetValue = 24f,
        animationSpec = infiniteRepeatable(tween(1200, easing = LinearEasing)),
        label = "dash",
    )
    val measurer = rememberTextMeasurer()
    val labelStyle = TextStyle(fontSize = 11.sp, fontWeight = FontWeight.SemiBold, color = Mesh.Ink)
    val description = "Map of your mall. Next stop: " + (places.firstOrNull { it.place == nextStop }?.label ?: "")

    Canvas(
        modifier = modifier
            .fillMaxWidth()
            .aspectRatio(1.25f)
            .semantics { contentDescription = description }
            .pointerInput(places) {
                detectTapGestures { tap ->
                    val geo = MapGeometry(size.width.toFloat(), size.height.toFloat())
                    val hit = places.minByOrNull { p ->
                        val c = geo.point(p.gx, p.gy)
                        hypot(c.x - tap.x, (c.y - geo.tileH * 0.25f) - tap.y)
                    }
                    if (hit != null) {
                        val c = geo.point(hit.gx, hit.gy)
                        if (hypot(c.x - tap.x, (c.y - geo.tileH * 0.25f) - tap.y) < geo.tileW * 0.45f) onPlace(hit.place)
                    }
                }
            },
    ) {
        val geo = MapGeometry(size.width, size.height)

        // Ground: a big soft diamond with a path ring.
        val ground = Path().apply {
            val t = geo.point(-0.5f, -0.5f); val r = geo.point(2.5f, -0.5f)
            val b = geo.point(2.5f, 2.5f); val l = geo.point(-0.5f, 2.5f)
            moveTo(t.x, t.y); lineTo(r.x, r.y); lineTo(b.x, b.y); lineTo(l.x, l.y); close()
        }
        translate(top = geo.tileH * 0.18f) { drawPath(ground, Color.Black.copy(alpha = 0.06f)) }
        drawPath(ground, Mesh.Ground)
        // Walkways along the grid.
        for (i in 0..2) {
            drawLine(Mesh.Path, geo.point(i.toFloat(), -0.3f), geo.point(i.toFloat(), 2.3f), strokeWidth = geo.tileH * 0.16f, cap = StrokeCap.Round)
            drawLine(Mesh.Path, geo.point(-0.3f, i.toFloat()), geo.point(2.3f, i.toFloat()), strokeWidth = geo.tileH * 0.16f, cap = StrokeCap.Round)
        }

        // Glowing walkway from the fountain to the next stop.
        val target = places.firstOrNull { it.place == nextStop }
        if (target != null) {
            val from = geo.point(1f, 1f)
            val to = geo.point(target.gx, target.gy)
            val alpha = if (reduceMotion) 0.9f else glow
            drawLine(Mesh.Amber.copy(alpha = 0.25f * alpha), from, to, strokeWidth = geo.tileH * 0.34f, cap = StrokeCap.Round)
            drawLine(
                Mesh.Amber.copy(alpha = alpha),
                from, to,
                strokeWidth = geo.tileH * 0.09f,
                cap = StrokeCap.Round,
                pathEffect = PathEffect.dashPathEffect(floatArrayOf(10f, 14f), if (reduceMotion) 0f else -dash),
            )
        }

        // Trees at the corners of the board.
        for ((gx, gy) in listOf(-0.35f to 0.5f, 0.5f to -0.35f, 2.35f to 1.5f, 1.5f to 2.35f, -0.3f to 1.6f)) {
            val c = geo.point(gx, gy)
            drawCircle(Color.Black.copy(alpha = 0.08f), radius = geo.tileH * 0.16f, center = Offset(c.x, c.y + geo.tileH * 0.08f))
            drawCircle(Mesh.Tree, radius = geo.tileH * 0.18f, center = Offset(c.x, c.y - geo.tileH * 0.14f))
        }

        // Fountain with the MellowMesh "M" — You are here.
        drawFountain(geo)

        // Buildings, back to front so nearer ones overlap farther ones.
        places.sortedBy { it.gx + it.gy }.forEach { p ->
            val c = geo.point(p.gx, p.gy)
            val lit = p.place == nextStop
            drawBuilding(c, geo, p.roof, lit, if (reduceMotion) 1f else glow)
            if (p.place == Place.HELPERS) {
                val sizePx = geo.tileH * 0.55f
                translate(left = c.x + geo.tileW * 0.12f, top = c.y - sizePx * 0.55f) {
                    drawHelperBlob(helperColor, helperMood, areaSize = Size(sizePx, sizePx))
                }
            }
            val layout = measurer.measure(p.label, labelStyle)
            val tl = Offset(c.x - layout.size.width / 2f, c.y + geo.tileH * 0.22f)
            drawRoundRect(
                Color.White.copy(alpha = 0.85f),
                topLeft = Offset(tl.x - 6f, tl.y - 2f),
                size = Size(layout.size.width + 12f, layout.size.height + 4f),
                cornerRadius = androidx.compose.ui.geometry.CornerRadius(10f, 10f),
            )
            drawText(layout, topLeft = tl)
            val badge = badges[p.place] ?: 0
            if (badge > 0) {
                val bc = Offset(c.x + geo.tileW * 0.18f, c.y - geo.tileH * 0.95f)
                drawCircle(Mesh.Amber, radius = geo.tileH * 0.16f, center = bc)
                val t = measurer.measure(if (badge > 9) "9+" else badge.toString(), labelStyle.copy(fontWeight = FontWeight.Bold))
                drawText(t, topLeft = Offset(bc.x - t.size.width / 2f, bc.y - t.size.height / 2f))
            }
        }

        val here = measurer.measure(youAreHere, labelStyle.copy(color = Mesh.Indigo))
        val f = geo.point(1f, 1f)
        drawText(here, topLeft = Offset(f.x - here.size.width / 2f, f.y + geo.tileH * 0.3f))
    }
}

/** Isometric projection of grid coordinates into the canvas. */
class MapGeometry(width: Float, height: Float) {
    val tileW = width / 3.4f
    val tileH = tileW * 0.56f
    private val originX = width / 2f
    private val originY = height / 2f - tileH * 0.9f

    fun point(gx: Float, gy: Float): Offset =
        Offset(originX + (gx - gy) * tileW / 2f, originY + (gx + gy) * tileH / 2f)
}

private fun DrawScope.drawFountain(geo: MapGeometry) {
    val c = geo.point(1f, 1f)
    val rx = geo.tileW * 0.26f
    val ry = geo.tileH * 0.26f
    drawOval(Color.White, topLeft = Offset(c.x - rx, c.y - ry), size = Size(rx * 2, ry * 2))
    drawOval(Mesh.Water, topLeft = Offset(c.x - rx * 0.8f, c.y - ry * 0.8f), size = Size(rx * 1.6f, ry * 1.6f))
    // The "M".
    val m = Path().apply {
        val w = rx * 0.7f
        val h = ry * 1.5f
        moveTo(c.x - w / 2, c.y + h * 0.1f)
        lineTo(c.x - w / 2, c.y - h * 0.6f)
        lineTo(c.x, c.y - h * 0.1f)
        lineTo(c.x + w / 2, c.y - h * 0.6f)
        lineTo(c.x + w / 2, c.y + h * 0.1f)
    }
    drawPath(m, Mesh.Indigo, style = Stroke(width = geo.tileH * 0.07f, cap = StrokeCap.Round))
}

/** A white clay building with a colored roof; the next stop glows amber. */
private fun DrawScope.drawBuilding(c: Offset, geo: MapGeometry, roof: Color, lit: Boolean, glow: Float) {
    val a = geo.tileW * 0.3f
    val b = geo.tileH * 0.3f
    val h = geo.tileH * 0.55f
    fun pt(dx: Float, dy: Float) = Offset(c.x + dx, c.y + dy)
    // Shadow.
    drawOval(Color.Black.copy(alpha = 0.10f), topLeft = pt(-a * 1.05f, -b * 0.6f), size = Size(a * 2.3f, b * 2.0f))
    if (lit) {
        drawOval(Mesh.Amber.copy(alpha = 0.35f * glow), topLeft = pt(-a * 1.4f, -b * 1.2f), size = Size(a * 2.8f, b * 2.6f))
    }
    val left = Path().apply {
        moveTo(c.x - a, c.y); lineTo(c.x, c.y + b); lineTo(c.x, c.y + b - h); lineTo(c.x - a, c.y - h); close()
    }
    val right = Path().apply {
        moveTo(c.x, c.y + b); lineTo(c.x + a, c.y); lineTo(c.x + a, c.y - h); lineTo(c.x, c.y + b - h); close()
    }
    val top = Path().apply {
        moveTo(c.x, c.y - b - h * 1.25f)
        lineTo(c.x + a * 1.08f, c.y - h)
        lineTo(c.x, c.y + b - h)
        lineTo(c.x - a * 1.08f, c.y - h)
        close()
    }
    drawPath(left, Color(0xFFF7F6F2))
    drawPath(right, Color(0xFFE7E5DF))
    drawPath(top, roof)
    drawPath(top, Color.White.copy(alpha = 0.18f))
    // Door.
    val door = Path().apply {
        val dx = a * 0.18f
        moveTo(c.x - a * 0.5f - dx, c.y + b * 0.5f - dx * 0.5f)
        lineTo(c.x - a * 0.5f + dx, c.y + b * 0.5f + dx * 0.5f)
        lineTo(c.x - a * 0.5f + dx, c.y + b * 0.5f + dx * 0.5f - h * 0.45f)
        lineTo(c.x - a * 0.5f - dx, c.y + b * 0.5f - dx * 0.5f - h * 0.45f)
        close()
    }
    drawPath(door, if (lit) Mesh.Amber else roof.copy(alpha = 0.6f))
}
