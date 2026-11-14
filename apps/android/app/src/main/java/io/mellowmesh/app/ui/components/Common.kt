package io.mellowmesh.app.ui.components

import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.Access
import io.mellowmesh.core.Identity
import io.mellowmesh.core.Mood
import io.mellowmesh.core.Participant
import io.mellowmesh.core.PrincipalKind

/** White card with a hairline and a soft shadow. */
@Composable
fun MeshCard(
    modifier: Modifier = Modifier,
    color: Color = Mesh.Card,
    contentPadding: Dp = 16.dp,
    content: @Composable ColumnScope.() -> Unit,
) {
    Surface(
        modifier = modifier,
        shape = RoundedCornerShape(22.dp),
        color = color,
        border = BorderStroke(1.dp, Mesh.Hairline),
        shadowElevation = 1.dp,
    ) {
        Column(modifier = Modifier.padding(contentPadding), content = content)
    }
}

/** Black primary action (design: black ink carries every primary action). */
@Composable
fun InkButton(text: String, onClick: () -> Unit, modifier: Modifier = Modifier, enabled: Boolean = true) {
    Button(
        onClick = onClick,
        modifier = modifier.height(52.dp),
        enabled = enabled,
        shape = RoundedCornerShape(16.dp),
        colors = ButtonDefaults.buttonColors(containerColor = Mesh.Ink, contentColor = Color.White),
    ) {
        Text(text, style = MaterialTheme.typography.labelLarge)
    }
}

@Composable
fun SectionTitle(text: String, modifier: Modifier = Modifier) {
    Text(
        text = text,
        style = MaterialTheme.typography.titleMedium,
        color = Mesh.Ink,
        modifier = modifier.padding(top = 20.dp, bottom = 8.dp),
    )
}

/** The "AI" tag that always sits next to a helper's name (honesty rule). */
@Composable
fun AiTag(modifier: Modifier = Modifier) {
    Box(
        modifier = modifier
            .clip(RoundedCornerShape(6.dp))
            .background(Mesh.IndigoTint)
            .padding(horizontal = 5.dp, vertical = 1.dp),
    ) {
        Text("AI", color = Mesh.Indigo, fontSize = 10.sp, fontWeight = FontWeight.Bold)
    }
}

/** People are never drawn: round initials. */
@Composable
fun PersonAvatar(name: String, size: Dp, modifier: Modifier = Modifier, color: Color = Color(0xFFE6E4DE)) {
    Box(
        modifier = modifier
            .size(size)
            .clip(CircleShape)
            .background(color)
            .semantics { contentDescription = "Person $name" },
        contentAlignment = Alignment.Center,
    ) {
        Text(
            text = Identity.initials(name),
            color = Mesh.Ink,
            fontWeight = FontWeight.SemiBold,
            fontSize = (size.value * 0.38f).sp,
        )
    }
}

/** Helper → clay blob, anyone else → initials. */
@Composable
fun ParticipantAvatar(p: Participant, size: Dp, modifier: Modifier = Modifier, mood: Mood = Mood.IDLE) {
    if (p.kind == PrincipalKind.AGENT) {
        HelperBlob(Mesh.accent(p.accent), mood, size, modifier, description = "AI helper ${p.displayName}")
    } else {
        PersonAvatar(p.displayName, size, modifier)
    }
}

/** Avatar for a principal URI. */
@Composable
fun PrincipalAvatar(id: String, name: String, size: Dp, modifier: Modifier = Modifier) {
    ParticipantAvatar(Participant(id, PrincipalKind.of(id), name, Identity.accentOf(id)), size, modifier)
}

/** Name followed by the AI tag for helpers. */
@Composable
fun NameWithTag(name: String, isHelper: Boolean, modifier: Modifier = Modifier) {
    Row(modifier = modifier, verticalAlignment = Alignment.CenterVertically) {
        Text(name, style = MaterialTheme.typography.titleSmall, color = Mesh.Ink, maxLines = 1)
        if (isHelper) {
            Spacer(Modifier.width(6.dp))
            AiTag()
        }
    }
}

/** Small colored pill (status, counts). */
@Composable
fun Pill(text: String, background: Color, foreground: Color, modifier: Modifier = Modifier) {
    Box(
        modifier = modifier
            .clip(RoundedCornerShape(50))
            .background(background)
            .padding(horizontal = 10.dp, vertical = 4.dp),
    ) {
        Text(text, color = foreground, style = MaterialTheme.typography.labelMedium)
    }
}

/** Badge dot with a count (Glance level). */
@Composable
fun CountBadge(count: Int, modifier: Modifier = Modifier) {
    if (count <= 0) return
    Box(
        modifier = modifier
            .size(20.dp)
            .clip(CircleShape)
            .background(Mesh.Amber),
        contentAlignment = Alignment.Center,
    ) {
        Text(if (count > 9) "9+" else count.toString(), color = Mesh.Ink, fontSize = 11.sp, fontWeight = FontWeight.Bold)
    }
}

/** Lock (closed), key (private) or open door (open), drawn natively. */
@Composable
fun AccessIcon(access: String, size: Dp = 22.dp, modifier: Modifier = Modifier) {
    val label = when (access) {
        Access.PRIVATE -> "Private"
        Access.OPEN -> "Open"
        else -> "Closed"
    }
    Canvas(modifier = modifier.size(size).semantics { contentDescription = label }) {
        val w = this.size.width
        val h = this.size.height
        val stroke = Stroke(width = w * 0.09f, cap = StrokeCap.Round)
        when (access) {
            Access.PRIVATE -> {
                // Key: ring + shaft + teeth.
                drawCircle(Mesh.Indigo, radius = w * 0.2f, center = Offset(w * 0.3f, h * 0.5f), style = stroke)
                drawLine(Mesh.Indigo, Offset(w * 0.5f, h * 0.5f), Offset(w * 0.92f, h * 0.5f), strokeWidth = w * 0.09f, cap = StrokeCap.Round)
                drawLine(Mesh.Indigo, Offset(w * 0.78f, h * 0.5f), Offset(w * 0.78f, h * 0.68f), strokeWidth = w * 0.09f, cap = StrokeCap.Round)
                drawLine(Mesh.Indigo, Offset(w * 0.9f, h * 0.5f), Offset(w * 0.9f, h * 0.64f), strokeWidth = w * 0.09f, cap = StrokeCap.Round)
            }
            Access.OPEN -> {
                // Open door: frame + a door leaf swung open.
                drawRect(Mesh.Green, topLeft = Offset(w * 0.2f, h * 0.12f), size = Size(w * 0.6f, h * 0.8f), style = stroke)
                val leaf = Path().apply {
                    moveTo(w * 0.2f, h * 0.12f)
                    lineTo(w * 0.52f, h * 0.22f)
                    lineTo(w * 0.52f, h * 0.98f)
                    lineTo(w * 0.2f, h * 0.92f)
                    close()
                }
                drawPath(leaf, Mesh.GreenTint)
                drawPath(leaf, Mesh.Green, style = stroke)
            }
            else -> {
                // Lock: shackle + body.
                drawArc(Mesh.Ink, 180f, 180f, false, topLeft = Offset(w * 0.28f, h * 0.1f), size = Size(w * 0.44f, h * 0.5f), style = stroke)
                drawRoundRect(Mesh.Ink, topLeft = Offset(w * 0.18f, h * 0.38f), size = Size(w * 0.64f, h * 0.52f),
                    cornerRadius = androidx.compose.ui.geometry.CornerRadius(w * 0.1f, w * 0.1f))
                drawCircle(Color.White, radius = w * 0.06f, center = Offset(w * 0.5f, h * 0.62f))
            }
        }
    }
}

/** Friendly error box with a retry. */
@Composable
fun ErrorNote(message: String, onRetry: (() -> Unit)? = null, modifier: Modifier = Modifier) {
    MeshCard(modifier = modifier.fillMaxWidth(), color = Mesh.RoseTint) {
        Text(message, color = Mesh.Ink, style = MaterialTheme.typography.bodyMedium)
        if (onRetry != null) {
            Spacer(Modifier.height(8.dp))
            androidx.compose.material3.TextButton(onClick = onRetry) { Text("Try again", color = Mesh.Ink) }
        }
    }
}

/** Simple vertical spacing helper. */
@Composable
fun Gap(height: Dp = 12.dp) = Spacer(Modifier.height(height))

/** A row of evenly spaced items. */
@Composable
fun SpacedRow(modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    Row(modifier = modifier, horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
        content()
    }
}
