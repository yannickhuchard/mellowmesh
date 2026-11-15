package io.mellowmesh.app.ui.chats

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import io.mellowmesh.app.R
import io.mellowmesh.app.ui.theme.Mesh
import io.mellowmesh.core.ChartKind
import io.mellowmesh.core.MeshUiCard
import io.mellowmesh.core.MiniMarkdown
import io.mellowmesh.core.UiAction
import io.mellowmesh.core.UiActionStyle
import io.mellowmesh.core.UiBlock

/** Markdown subset → styled text. Links never open directly: `onLink` asks first. */
fun markdownText(markdown: String, onLink: (String) -> Unit): AnnotatedString = buildAnnotatedString {
    for (span in MiniMarkdown.parse(markdown)) {
        val style = SpanStyle(
            fontWeight = if (span.bold) FontWeight.SemiBold else null,
            fontStyle = if (span.italic) FontStyle.Italic else null,
            fontFamily = if (span.code) FontFamily.Monospace else null,
            background = if (span.code) Color(0xFFEDECE7) else Color.Unspecified,
        )
        val url = span.url
        if (url != null) {
            withLink(
                LinkAnnotation.Clickable(
                    tag = url,
                    styles = TextLinkStyles(style = SpanStyle(color = Mesh.Indigo, textDecoration = TextDecoration.Underline)),
                    linkInteractionListener = { onLink(url) },
                ),
            ) { append(span.text) }
        } else {
            withStyle(style) { append(span.text) }
        }
    }
}

/**
 * A MeshUI card rendered with native Compose components (same block kinds
 * as app-core). Actions only publish a reply into the conversation; they
 * can never answer a decision.
 */
@Composable
fun MeshUiCardView(
    card: MeshUiCard,
    onAction: (UiAction) -> Unit,
    onLink: (String) -> Unit,
    modifier: Modifier = Modifier,
) {
    Column(modifier = modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        if (card.title.isNotBlank()) {
            Text(card.title, style = MaterialTheme.typography.titleMedium, color = Mesh.Ink)
        }
        for (block in card.blocks) {
            when (block) {
                is UiBlock.Text -> Text(
                    markdownText(block.markdown, onLink),
                    style = MaterialTheme.typography.bodyMedium,
                    color = Mesh.Ink,
                )
                is UiBlock.KeyValue -> Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    for (p in block.pairs) {
                        Row {
                            Text(p.key, style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2, modifier = Modifier.weight(0.4f))
                            Text(p.value, style = MaterialTheme.typography.bodySmall, color = Mesh.Ink, modifier = Modifier.weight(0.6f))
                        }
                    }
                }
                is UiBlock.Progress -> Column {
                    if (block.label.isNotBlank()) {
                        Row(Modifier.fillMaxWidth()) {
                            Text(block.label, style = MaterialTheme.typography.bodySmall, color = Mesh.Ink2, modifier = Modifier.weight(1f))
                            Text("${(block.value * 100).toInt()}%", style = MaterialTheme.typography.bodySmall, color = Mesh.Indigo)
                        }
                    }
                    LinearProgressIndicator(
                        progress = { block.value.toFloat() },
                        modifier = Modifier.fillMaxWidth().height(8.dp).padding(top = 4.dp),
                        color = Mesh.Indigo,
                        trackColor = Mesh.IndigoTint,
                    )
                }
                is UiBlock.Metric -> Column {
                    Text(block.label, style = MaterialTheme.typography.labelMedium, color = Mesh.Ink2)
                    Row(verticalAlignment = Alignment.Bottom) {
                        Text(block.value, style = MaterialTheme.typography.displaySmall, color = Mesh.Ink)
                        val d = block.delta
                        if (d != null) {
                            Text(
                                d,
                                style = MaterialTheme.typography.titleSmall,
                                color = if (d.startsWith("-")) Mesh.Rose else Mesh.Green,
                                modifier = Modifier.padding(start = 8.dp, bottom = 6.dp),
                            )
                        }
                    }
                }
                is UiBlock.ListBlock -> Column(verticalArrangement = Arrangement.spacedBy(2.dp)) {
                    for (item in block.items) {
                        Text("•  $item", style = MaterialTheme.typography.bodyMedium, color = Mesh.Ink)
                    }
                }
                is UiBlock.Chart -> MiniChart(block)
                is UiBlock.Image -> {
                    // Remote images are not loaded automatically; the user can open them.
                    OutlinedButton(onClick = { onLink(block.url) }) {
                        Text(stringResource(R.string.image_from_helper, block.alt), color = Mesh.Ink)
                    }
                }
                is UiBlock.Actions -> Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    for (a in block.actions) {
                        when (a.style) {
                            UiActionStyle.PRIMARY -> Button(
                                onClick = { onAction(a) },
                                shape = RoundedCornerShape(14.dp),
                                colors = ButtonDefaults.buttonColors(containerColor = Mesh.Ink, contentColor = Color.White),
                            ) { Text(a.label) }
                            UiActionStyle.DESTRUCTIVE -> OutlinedButton(onClick = { onAction(a) }, shape = RoundedCornerShape(14.dp)) {
                                Text(a.label, color = Mesh.Rose)
                            }
                            UiActionStyle.SECONDARY -> OutlinedButton(onClick = { onAction(a) }, shape = RoundedCornerShape(14.dp)) {
                                Text(a.label, color = Mesh.Ink)
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun MiniChart(block: UiBlock.Chart) {
    val values = block.series
    val min = values.minOrNull() ?: 0.0
    val max = values.maxOrNull() ?: 1.0
    val span = if (max - min == 0.0) 1.0 else max - min
    Column {
        Canvas(
            Modifier
                .fillMaxWidth()
                .height(120.dp)
                .semantics { contentDescription = "Chart with ${values.size} points" },
        ) {
            val w = size.width
            val h = size.height
            val n = values.size
            when (block.kind) {
                ChartKind.BAR -> {
                    val slot = w / n
                    val base = if (min >= 0) 0.0 else min
                    val range = if (max - base == 0.0) 1.0 else max - base
                    values.forEachIndexed { i, v ->
                        val bh = ((v - base) / range * h).toFloat()
                        drawRoundRect(
                            Mesh.Indigo,
                            topLeft = Offset(i * slot + slot * 0.15f, h - bh),
                            size = Size(slot * 0.7f, bh),
                            cornerRadius = androidx.compose.ui.geometry.CornerRadius(6f, 6f),
                        )
                    }
                }
                ChartKind.LINE -> {
                    val path = Path()
                    values.forEachIndexed { i, v ->
                        val x = if (n == 1) w / 2 else i * w / (n - 1)
                        val y = (h - (v - min) / span * h).toFloat()
                        if (i == 0) path.moveTo(x, y) else path.lineTo(x, y)
                    }
                    drawPath(path, Mesh.Indigo, style = Stroke(width = 5f, cap = StrokeCap.Round))
                }
            }
        }
        if (block.labels.isNotEmpty()) {
            Row(Modifier.fillMaxWidth()) {
                val step = maxOf(1, block.labels.size / 5)
                block.labels.forEachIndexed { i, l ->
                    if (i % step == 0) {
                        Text(l, style = MaterialTheme.typography.labelSmall, color = Mesh.Ink3, modifier = Modifier.weight(1f))
                    }
                }
            }
        }
        // Keep a little air below charts.
        androidx.compose.foundation.layout.Spacer(Modifier.width(1.dp).height(2.dp))
    }
}
