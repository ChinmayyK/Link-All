package app.linkall.ui.theme

import android.os.Build
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asComposeRenderEffect
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp

fun Modifier.glassmorphism(
    cornerRadius: Dp = 16.dp,
    blurRadius: Float = 50f,
    overlayColor: Color = Color(0x33FFFFFF), // Semi-transparent white
    borderColor: Color = Color(0x40FFFFFF) // Semi-transparent white border
): Modifier = composed {
    this.then(
        Modifier
            .clip(RoundedCornerShape(cornerRadius))
            .graphicsLayer {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    renderEffect = android.graphics.RenderEffect.createBlurEffect(
                        blurRadius,
                        blurRadius,
                        android.graphics.Shader.TileMode.DECAL
                    ).asComposeRenderEffect()
                }
            }
            .background(overlayColor)
            .border(1.dp, borderColor, RoundedCornerShape(cornerRadius))
    )
}
