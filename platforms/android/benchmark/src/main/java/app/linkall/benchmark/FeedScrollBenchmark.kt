package app.linkall.benchmark

import android.content.Intent
import androidx.benchmark.macro.CompilationMode
import androidx.benchmark.macro.FrameTimingMetric
import androidx.benchmark.macro.StartupMode
import androidx.benchmark.macro.junit4.MacrobenchmarkRule
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.uiautomator.By
import androidx.test.uiautomator.Direction
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith

@RunWith(AndroidJUnit4::class)
class FeedScrollBenchmark {

    @get:Rule
    val benchmarkRule = MacrobenchmarkRule()

    @Test
    fun scrollFeed() {
        benchmarkRule.measureRepeated(
            packageName = "app.linkall",
            metrics = listOf(FrameTimingMetric()),
            compilationMode = CompilationMode.DEFAULT,
            startupMode = StartupMode.COLD,
            iterations = 5,
            setupBlock = {
                pressHome()
            }
        ) {
            val intent = Intent("android.intent.action.MAIN")
            intent.setPackage("app.linkall")
            intent.setClassName("app.linkall", "app.linkall.MainActivity")
            intent.putExtra("benchmark", true)
            intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TASK)
            
            startActivityAndWait(intent)

            // Find the list by its scrollable state and scroll it
            val list = device.findObject(By.scrollable(true))
            if (list != null) {
                list.setGestureMargin(device.displayWidth / 5)
                list.scroll(Direction.DOWN, 1f)
                list.scroll(Direction.UP, 1f)
            } else {
                throw AssertionError("Could not find a scrollable list on screen")
            }
        }
    }
}
