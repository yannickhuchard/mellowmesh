package io.mellowmesh.app.ui.theme

import android.provider.Settings
import androidx.compose.animation.core.SpringSpec
import androidx.compose.animation.core.spring
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import io.mellowmesh.core.Accent

/** Design tokens of "Mesh Mall in daylight" (docs/design/design-language.md). */
object Mesh {
    val Paper = Color(0xFFF4F3EF)
    val Card = Color(0xFFFFFFFF)
    val Hairline = Color(0x0F111114)
    val Ink = Color(0xFF111114)
    val Ink2 = Color(0xFF55555C)
    val Ink3 = Color(0xFF8A8A90)
    val Indigo = Color(0xFF4F46E5)
    val IndigoTint = Color(0xFFEEEDFC)
    val Amber = Color(0xFFF59E0B)
    val AmberTint = Color(0xFFFFF6E0)
    val Green = Color(0xFF10B981)
    val GreenTint = Color(0xFFE7F8F0)
    val Rose = Color(0xFFE11D48)
    val RoseTint = Color(0xFFFFF1F5)

    // Helper colors (color = identity). Grey means asleep.
    val Violet = Color(0xFF8B5CF6)
    val Sky = Color(0xFF38BDF8)
    val Pink = Color(0xFFEC4899)
    val Mint = Color(0xFF34D399)
    val Sleep = Color(0xFFB8B8BE)
    val Cheek = Color(0xFFFF8FB1)

    // Clay diorama
    val Ground = Color(0xFFE9F0E1)
    val Path = Color(0xFFF1EBDD)
    val Tree = Color(0xFF9ED8A7)
    val Water = Color(0xFFBFE3F6)

    fun accent(a: Accent): Color = when (a) {
        Accent.INDIGO -> Indigo
        Accent.SKY -> Sky
        Accent.PINK -> Pink
        Accent.AMBER -> Amber
        Accent.MINT -> Mint
        Accent.VIOLET -> Violet
    }

    /** Parse "#RRGGBB" (team colors); falls back to indigo. */
    fun parseHex(hex: String?): Color {
        val h = hex?.trim()?.removePrefix("#") ?: return Indigo
        if (h.length != 6) return Indigo
        val v = h.toLongOrNull(16) ?: return Indigo
        return Color(0xFF000000 or v)
    }
}

/** Motion springs (design language §5). */
object Motion {
    fun <T> snap(): SpringSpec<T> = spring(dampingRatio = 0.86f, stiffness = 700f)
    fun <T> glide(): SpringSpec<T> = spring(dampingRatio = 0.9f, stiffness = 300f)
    fun <T> bloom(): SpringSpec<T> = spring(dampingRatio = 0.7f, stiffness = 400f)
}

/** True when the user turned animations off (Settings → Accessibility → Remove animations). */
@Composable
fun rememberReduceMotion(): Boolean {
    val context = LocalContext.current
    return remember(context) {
        try {
            Settings.Global.getFloat(context.contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f
        } catch (_: Exception) {
            false
        }
    }
}

private val Colors = lightColorScheme(
    primary = Mesh.Ink,
    onPrimary = Color.White,
    primaryContainer = Mesh.IndigoTint,
    onPrimaryContainer = Mesh.Ink,
    secondary = Mesh.Indigo,
    onSecondary = Color.White,
    secondaryContainer = Mesh.IndigoTint,
    onSecondaryContainer = Mesh.Ink,
    tertiary = Mesh.Amber,
    onTertiary = Mesh.Ink,
    tertiaryContainer = Mesh.AmberTint,
    onTertiaryContainer = Mesh.Ink,
    background = Mesh.Paper,
    onBackground = Mesh.Ink,
    surface = Mesh.Card,
    onSurface = Mesh.Ink,
    surfaceVariant = Mesh.Paper,
    onSurfaceVariant = Mesh.Ink2,
    surfaceContainer = Mesh.Card,
    surfaceContainerLow = Mesh.Card,
    surfaceContainerHigh = Mesh.Card,
    surfaceContainerHighest = Color(0xFFEDECE7),
    outline = Mesh.Ink3,
    outlineVariant = Color(0x1F111114),
    error = Mesh.Rose,
    onError = Color.White,
    errorContainer = Mesh.RoseTint,
    onErrorContainer = Mesh.Ink,
)

private val Sans = FontFamily.SansSerif

private val MeshTypography = Typography(
    displayLarge = TextStyle(fontFamily = Sans, fontWeight = FontWeight.Bold, fontSize = 52.sp, lineHeight = 56.sp, letterSpacing = (-0.035).em),
    displayMedium = TextStyle(fontFamily = Sans, fontWeight = FontWeight.Bold, fontSize = 40.sp, lineHeight = 44.sp, letterSpacing = (-0.035).em),
    displaySmall = TextStyle(fontFamily = Sans, fontWeight = FontWeight.Bold, fontSize = 32.sp, lineHeight = 36.sp, letterSpacing = (-0.035).em),
    headlineLarge = TextStyle(fontFamily = Sans, fontWeight = FontWeight.Bold, fontSize = 30.sp, lineHeight = 34.sp, letterSpacing = (-0.03).em),
    headlineMedium = TextStyle(fontFamily = Sans, fontWeight = FontWeight.Bold, fontSize = 26.sp, lineHeight = 30.sp, letterSpacing = (-0.025).em),
    headlineSmall = TextStyle(fontFamily = Sans, fontWeight = FontWeight.SemiBold, fontSize = 22.sp, lineHeight = 28.sp, letterSpacing = (-0.02).em),
    titleLarge = TextStyle(fontFamily = Sans, fontWeight = FontWeight.SemiBold, fontSize = 20.sp, lineHeight = 26.sp, letterSpacing = (-0.015).em),
    titleMedium = TextStyle(fontFamily = Sans, fontWeight = FontWeight.SemiBold, fontSize = 17.sp, lineHeight = 22.sp, letterSpacing = (-0.01).em),
    titleSmall = TextStyle(fontFamily = Sans, fontWeight = FontWeight.SemiBold, fontSize = 15.sp, lineHeight = 20.sp),
    bodyLarge = TextStyle(fontFamily = Sans, fontWeight = FontWeight.Normal, fontSize = 17.sp, lineHeight = 24.sp),
    bodyMedium = TextStyle(fontFamily = Sans, fontWeight = FontWeight.Normal, fontSize = 15.sp, lineHeight = 21.sp),
    bodySmall = TextStyle(fontFamily = Sans, fontWeight = FontWeight.Normal, fontSize = 13.sp, lineHeight = 18.sp),
    labelLarge = TextStyle(fontFamily = Sans, fontWeight = FontWeight.SemiBold, fontSize = 15.sp, lineHeight = 20.sp),
    labelMedium = TextStyle(fontFamily = Sans, fontWeight = FontWeight.Medium, fontSize = 13.sp, lineHeight = 16.sp),
    labelSmall = TextStyle(fontFamily = Sans, fontWeight = FontWeight.Medium, fontSize = 11.sp, lineHeight = 14.sp, letterSpacing = 0.02.em),
)

private val MeshShapes = Shapes(
    extraSmall = RoundedCornerShape(8.dp),
    small = RoundedCornerShape(12.dp),
    medium = RoundedCornerShape(18.dp),
    large = RoundedCornerShape(24.dp),
    extraLarge = RoundedCornerShape(32.dp),
)

@Composable
fun MellowMeshTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = Colors,
        typography = MeshTypography,
        shapes = MeshShapes,
        content = content,
    )
}
