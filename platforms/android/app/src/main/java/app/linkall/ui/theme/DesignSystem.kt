package app.linkall.ui.theme

import android.os.Build
import android.graphics.RuntimeShader
import androidx.compose.ui.graphics.ShaderBrush
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.platform.LocalHapticFeedback
import org.intellij.lang.annotations.Language
import androidx.compose.animation.animateColorAsState
import androidx.compose.animation.core.*

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.draw.blur
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.scale
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlin.random.Random

object CRTheme {
    // Structural Neutrals (Proper Contrast)
    val bgLight = Color(0xFFF5F6FA)
    val bgDark = Color(0xFF000000)
    
    val surfaceLight = Color.White.copy(alpha = 0.90f)
    val surfaceDark = Color(0xFF1C1C1E).copy(alpha = 0.85f)
    
    val surfaceElevatedLight = Color.White.copy(alpha = 1.0f)
    val surfaceElevatedDark = Color(0xFF2C2C2E).copy(alpha = 1.0f)
    
    val textHighLight = Color(0xFF111827)
    val textHighDark = Color(0xFFF9FAFB)
    val textMediumLight = Color(0xFF4B5563) // Darkened for contrast
    val textMediumDark = Color(0xFFD4D4D8)  // Lightened for contrast
    
    val strokeLight = Color(0xFFD1D5DB)     // Darkened stroke
    val strokeDark = Color(0xFF3F3F46)      // Lightened stroke
    
    // STRICT ACCENT PALETTE
    val blueSoft = Color(0xFF0066FF)
    val cyanSoft = Color(0xFF06B6D4)
    val blueGray = Color(0xFF4B5563)

    // Status
    val statusGreen = Color(0xFF10B981)
    val statusAmber = Color(0xFFF59E0B)
    val statusRed = Color(0xFFEF4444)

    fun bg(isDark: Boolean) = if (isDark) bgDark else bgLight
    fun surface(isDark: Boolean) = if (isDark) surfaceDark else surfaceLight
    fun surfaceElevated(isDark: Boolean) = if (isDark) surfaceElevatedDark else surfaceElevatedLight
    fun textHigh(isDark: Boolean) = if (isDark) textHighDark else textHighLight
    fun textMedium(isDark: Boolean) = if (isDark) textMediumDark else textMediumLight
    fun stroke(isDark: Boolean) = if (isDark) strokeDark else strokeLight
    fun glass(isDark: Boolean) = if (isDark) Color.White.copy(alpha = 0.03f) else Color.White.copy(alpha = 0.6f)

    // Legacy Aliases moved to Composable extensions at the end of the file
    fun textLow(isDark: Boolean) = textMedium(isDark).copy(alpha = 0.5f)
    
    // Ambient Ecosystem Glow Colors
    val ambientLavender = Color(0xFF93C5FD) // soft blue
    val ambientMint = Color(0xFF6EE7B7) // soft mint
    val ambientSky = Color(0xFF7DD3FC) // soft sky blue
}

object CRTypography {
    val h1 = TextStyle(
        fontFamily = OutfitFontFamily,
        fontWeight = FontWeight.Bold,
        fontSize = 28.sp,
        letterSpacing = (-0.5).sp
    )
    val h2 = TextStyle(
        fontFamily = OutfitFontFamily,
        fontWeight = FontWeight.SemiBold,
        fontSize = 18.sp,
        letterSpacing = (-0.3).sp
    )
    val bodyMedium = TextStyle(
        fontFamily = InterFontFamily,
        fontWeight = FontWeight.Medium,
        fontSize = 15.sp
    )
    val label = TextStyle(
        fontFamily = InterFontFamily,
        fontWeight = FontWeight.Medium,
        fontSize = 14.sp,
        letterSpacing = 0.2.sp
    )
    val caption = TextStyle(
        fontFamily = InterFontFamily,
        fontWeight = FontWeight.Medium,
        fontSize = 12.sp,
        letterSpacing = 0.3.sp,
        fontFeatureSettings = "tnum"
    )
}

fun Modifier.crPressScale(
    targetScale: Float = 0.96f,
    onClick: (() -> Unit)? = null
): Modifier = composed {
    val interactionSource = remember { MutableInteractionSource() }
    val isPressed by interactionSource.collectIsPressedAsState()
    val scale by animateFloatAsState(
        targetValue = if (isPressed) targetScale else 1f,
        animationSpec = spring(dampingRatio = 0.6f, stiffness = 400f),
        label = "press_scale"
    )

    val haptic = LocalHapticFeedback.current
    LaunchedEffect(isPressed) {
        if (isPressed) {
            haptic.performHapticFeedback(HapticFeedbackType.TextHandleMove)
        }
    }

    var mod = this.scale(scale)
    if (onClick != null) {
        mod = mod.clickable(
            interactionSource = interactionSource,
            indication = androidx.compose.material.ripple.rememberRipple(),
            onClick = onClick
        )
    }
    mod
}


object CRMotion {
    val snappy = spring<Float>(dampingRatio = 0.95f, stiffness = 800f)
    val fluid = spring<Float>(dampingRatio = 0.85f, stiffness = 600f)
}

@Language("AGSL")
private const val NOISE_SHADER = """
    uniform float2 iResolution;
    uniform float iTime;
    uniform half alpha;
    
    float hash(float2 p) {
        float3 p3  = fract(float3(p.xyx) * .1031);
        p3 += dot(p3, p3.yzx + 33.33);
        return fract((p3.x + p3.y) * p3.z);
    }

    half4 main(float2 fragCoord) {
        float n = hash(fragCoord + iTime);
        return half4(n, n, n, alpha * n);
    }
"""

@Composable
fun SubtleNoiseOverlay(isDark: Boolean) {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        val time by produceState(0f) {
            while (true) {
                withFrameNanos { value = it / 1_000_000_000f }
            }
        }
        val alpha = if (isDark) 0.07f else 0.035f
        val shader = remember { RuntimeShader(NOISE_SHADER) }
        
        androidx.compose.foundation.Canvas(modifier = Modifier.fillMaxSize()) {
            shader.setFloatUniform("iResolution", size.width, size.height)
            shader.setFloatUniform("iTime", time)
            shader.setFloatUniform("alpha", alpha)
            drawRect(brush = ShaderBrush(shader))
        }
    }
}

@Composable
fun CRBackground(
    isDark: Boolean,
    hasConnectedDevices: Boolean = false,
    flat: Boolean = false,
    content: @Composable () -> Unit
) {
    val electricColor = CRTheme.brandElectric
    val violetColor = CRTheme.brandViolet

    Box(
        modifier = Modifier
            .fillMaxSize()
            .background(
                if (isDark || flat) {
                    androidx.compose.ui.graphics.SolidColor(CRTheme.bg(isDark))
                } else {
                    Brush.linearGradient(
                        colors = listOf(Color(0xFFE5E5EA), Color(0xFFF2F4F8), Color(0xFFFFFFFF)),
                        start = Offset(0f, 0f),
                        end = Offset(Float.POSITIVE_INFINITY, Float.POSITIVE_INFINITY)
                    )
                }
            )
    ) {
        if (!isDark && !flat) {
            // Only animate when the mesh is drawn: an infinite transition
            // requests a frame every vsync even if nothing reads it.
            val infiniteTransition = rememberInfiniteTransition(label = "mesh")
            val breatheShift by infiniteTransition.animateFloat(
                initialValue = 0f, targetValue = 1f,
                animationSpec = infiniteRepeatable(tween(8000, easing = LinearEasing), RepeatMode.Reverse),
                label = "mesh_breathe"
            )
            androidx.compose.foundation.Canvas(modifier = Modifier.fillMaxSize()) {
                val centerOffset = Offset(size.width * 0.3f, size.height * (0.2f + (breatheShift * 0.1f)))
                val centerOffset2 = Offset(size.width * 0.8f, size.height * (0.6f - (breatheShift * 0.1f)))
                
                if (hasConnectedDevices) {
                    drawCircle(
                        brush = androidx.compose.ui.graphics.Brush.radialGradient(
                            colors = listOf(electricColor.copy(alpha = 0.15f), Color.Transparent),
                            center = centerOffset,
                            radius = size.width * 0.8f
                        ),
                        radius = size.width * 0.8f,
                        center = centerOffset
                    )
                    drawCircle(
                        brush = androidx.compose.ui.graphics.Brush.radialGradient(
                            colors = listOf(violetColor.copy(alpha = 0.12f), Color.Transparent),
                            center = centerOffset2,
                            radius = size.width * 0.7f
                        ),
                        radius = size.width * 0.7f,
                        center = centerOffset2
                    )
                } else {
                    drawCircle(
                        brush = androidx.compose.ui.graphics.Brush.radialGradient(
                            colors = listOf(Color.Black.copy(alpha = 0.06f), Color.Transparent),
                            center = centerOffset,
                            radius = size.width * 0.8f
                        ),
                        radius = size.width * 0.8f,
                        center = centerOffset
                    )
                }
            }
        }
        if (!flat) SubtleNoiseOverlay(isDark = isDark)
        content()
    }
}

fun Modifier.crGlassCard(
    isDark: Boolean,
    cornerRadius: Dp = 20.dp,
    highlighted: Boolean = false,
    dashed: Boolean = false,
    onClick: (() -> Unit)? = null,
    elevated: Boolean = false
): Modifier = composed {
    val shape = RoundedCornerShape(cornerRadius)
    val interactionSource = remember { MutableInteractionSource() }
    val isPressed by interactionSource.collectIsPressedAsState()
    
    val scale by animateFloatAsState(
        targetValue = if (isPressed && onClick != null) 0.96f else 1f,
        animationSpec = CRMotion.snappy,
        label = "press_scale"
    )

    val shadowColor = if (isDark) Color.Black.copy(alpha = 0.8f) else Color.Black.copy(alpha = 0.12f)
    val borderColor = if (highlighted) CRTheme.blueSoft.copy(alpha = 0.5f) else CRTheme.stroke(isDark).copy(alpha = if (isDark) 0.8f else 0.5f)
    val bgColor = if (elevated) CRTheme.surfaceElevated(isDark) else CRTheme.surface(isDark)

    var modifier = this
        .scale(scale)
        .shadow(
            elevation = if (onClick != null) (if (isPressed) 2.dp else 12.dp) else (if (elevated) 16.dp else 8.dp),
            shape = shape,
            ambientColor = shadowColor,
            spotColor = shadowColor
        )
        .clip(shape)
        .background(bgColor)
        .background(CRTheme.glass(isDark))

    if (onClick != null) {
        modifier = modifier.clickable(
            interactionSource = interactionSource,
            indication = null,
            onClick = onClick
        )
    }

    if (dashed) {
        modifier.border(1.dp, SolidColor(borderColor.copy(alpha = 0.5f)), shape)
    } else {
        // Gradient border for a top-inner light highlight
        val borderBrush = Brush.linearGradient(
            colors = listOf(
                if (highlighted) CRTheme.blueSoft else (if (isDark) Color.White.copy(0.15f) else Color.White.copy(0.7f)),
                borderColor
            ),
            start = Offset(0f, 0f),
            end = Offset(0f, Float.POSITIVE_INFINITY)
        )
        modifier.border(1.dp, borderBrush, shape)
    }
}

// Legacy Alias
fun Modifier.crCard(
    isDark: Boolean,
    cornerRadius: Dp = 24.dp,
    highlighted: Boolean = false,
    accentColor: Color = CRTheme.blueSoft,
    onClick: (() -> Unit)? = null
): Modifier = crGlassCard(
    isDark = isDark,
    cornerRadius = cornerRadius,
    highlighted = highlighted,
    dashed = false,
    onClick = onClick
)

@Composable
fun CRSwitch(checked: Boolean, isDark: Boolean) {
    val thumbColor = if (checked) CRTheme.bg(isDark) else CRTheme.textMedium(isDark)
    val trackColor = if (checked) CRTheme.textHigh(isDark) else Color.Transparent

    Box(
        modifier = Modifier
            .width(44.dp)
            .height(24.dp)
            .border(1.dp, if (checked) Color.Transparent else CRTheme.stroke(isDark), RoundedCornerShape(12.dp))
            .background(trackColor, RoundedCornerShape(12.dp))
            .padding(2.dp)
    ) {
        Box(
            modifier = Modifier
                .offset(x = if (checked) 20.dp else 0.dp)
                .size(20.dp)
                .background(thumbColor, RoundedCornerShape(10.dp))
        )
    }
}

val CRTheme.brandElectric: Color
    @Composable get() = androidx.compose.material3.MaterialTheme.colorScheme.primary
val CRTheme.brandViolet: Color
    @Composable get() = androidx.compose.material3.MaterialTheme.colorScheme.secondary
val CRTheme.brandCyan: Color
    @Composable get() = androidx.compose.material3.MaterialTheme.colorScheme.tertiary
val CRTheme.brandPink: Color
    @Composable get() = androidx.compose.material3.MaterialTheme.colorScheme.error
val CRTheme.accentGreen: Color
    @Composable get() = Color(0xFF10B981)
val CRTheme.accentRed: Color
    @Composable get() = androidx.compose.material3.MaterialTheme.colorScheme.error
val CRTheme.accentAmber: Color
    @Composable get() = Color(0xFFF59E0B)
