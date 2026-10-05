package app.linkall

import android.content.Intent
import android.service.quicksettings.Tile
import android.service.quicksettings.TileService
import android.widget.Toast

class PushClipboardTileService : TileService() {
    override fun onClick() {
        super.onClick()
        
        val intent = Intent(this, LinkAllService::class.java).apply {
            action = LinkAllService.ACTION_PUSH_CLIPBOARD
        }
        
        try {
            startService(intent)
        } catch (e: Exception) {
            e.printStackTrace()
        }
        
        val tile = qsTile
        tile.state = Tile.STATE_INACTIVE
        tile.updateTile()
        
        // Re-enable after a short delay so it acts like a button rather than a toggle
        Thread {
            Thread.sleep(1000)
            tile.state = Tile.STATE_ACTIVE
            tile.updateTile()
        }.start()
    }

    override fun onStartListening() {
        super.onStartListening()
        val tile = qsTile
        tile.state = Tile.STATE_ACTIVE
        tile.label = "Send clipboard"
        tile.updateTile()
    }
}
