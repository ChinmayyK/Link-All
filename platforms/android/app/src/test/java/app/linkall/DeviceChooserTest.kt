package app.linkall

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class DeviceChooserTest {
    private val answer = """
        {"offers_all":true,"choices":[
          {"id":"a","name":"Desk","platform":"Windows","available":true,"reason":null,"last_used":true},
          {"id":"b","name":"Mac","platform":"macOS","available":true,"reason":null,"last_used":false},
          {"id":"c","name":"Old PC","platform":null,"available":false,"reason":"Needs Link All 2.1 or later","last_used":false}
        ]}
    """.trimIndent()

    @Test fun theCoresAnswerIsParsedInOrder() {
        val got = DeviceChooser.parse(answer)!!
        assertTrue(got.offersAll)
        assertEquals(listOf("Desk", "Mac"), got.available.map { it.name })
        assertTrue(got.available.first().lastUsed)
    }

    @Test fun anUnavailableDeviceKeepsItsReason() {
        val old = DeviceChooser.parse(answer)!!.unavailable.single()
        assertFalse(old.available)
        assertEquals("Needs Link All 2.1 or later", old.reason)
        assertNull(old.platform)
    }

    @Test fun garbageIsNotAnAnswer() {
        assertNull(DeviceChooser.parse("not json"))
        assertNull(DeviceChooser.parse("""{"offers_all":true}"""))
    }
}
