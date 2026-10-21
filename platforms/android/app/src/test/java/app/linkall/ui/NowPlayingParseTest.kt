package app.linkall.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class NowPlayingParseTest {
    @Test fun readsEachTrackAndSkipsOnesWithoutATitle() {
        val json = """[
            {"device_id":"a","title":"Weightless","artist":"Marconi Union","album":"","app":"Spotify","playing":true,"position_ms":1000,"duration_ms":480000},
            {"device_id":"b","title":"","playing":false}
        ]"""
        val items = parseNowPlaying(json)
        assertEquals(1, items.size)
        val track = items[0]
        assertEquals("a", track.deviceId)
        assertEquals("Weightless", track.title)
        assertEquals("Spotify", track.app)
        assertTrue(track.playing)
        assertEquals(480_000L, track.durationMs)
    }

    @Test fun nothingOrBrokenJsonIsAnEmptyList() {
        assertTrue(parseNowPlaying(null).isEmpty())
        assertTrue(parseNowPlaying("not json").isEmpty())
        assertTrue(parseNowPlaying("[]").isEmpty())
    }
}
