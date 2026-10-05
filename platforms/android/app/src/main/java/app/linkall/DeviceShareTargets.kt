package app.linkall

import android.content.Context
import android.content.Intent
import androidx.core.content.pm.ShortcutInfoCompat
import androidx.core.content.pm.ShortcutManagerCompat
import androidx.core.graphics.drawable.IconCompat

/**
 * Publishes each connected computer as a Direct Share target, so it shows up
 * by name in Android's share sheet and a file reaches it in one tap.
 * See res/xml/shortcuts.xml and LinkAllShareTarget.
 */
object DeviceShareTargets {
    private const val CATEGORY = "app.linkall.category.SEND_TO_DEVICE"
    private const val ID_PREFIX = "device:"

    /** What was last published, so unchanged peer lists cost nothing. */
    @Volatile private var published: Map<String, String> = emptyMap()

    /** The peer a share-sheet shortcut stands for, or null. */
    fun peerIdOf(shortcutId: String?): String? =
        shortcutId?.takeIf { it.startsWith(ID_PREFIX) }?.removePrefix(ID_PREFIX)

    fun publish(context: Context, peers: List<PeerSnapshot>) {
        val max = ShortcutManagerCompat.getMaxShortcutCountPerActivity(context)
        val wanted = peers.filter { it.trusted && it.isConnected }
            .take(max)
            .associate { it.id to it.name }
        if (wanted == published) return

        runCatching {
            val gone = published.keys - wanted.keys
            if (gone.isNotEmpty()) {
                ShortcutManagerCompat.removeLongLivedShortcuts(context, gone.map { ID_PREFIX + it })
            }
            val icon = IconCompat.createWithResource(context, R.mipmap.ic_launcher)
            val shortcuts = wanted.map { (id, name) ->
                ShortcutInfoCompat.Builder(context, ID_PREFIX + id)
                    .setShortLabel(name)
                    .setLongLabel("Send to $name")
                    .setIcon(icon)
                    .setCategories(setOf(CATEGORY))
                    .setLongLived(true)
                    .setIntent(Intent(context, MainActivity::class.java).setAction(Intent.ACTION_VIEW))
                    .build()
            }
            ShortcutManagerCompat.setDynamicShortcuts(context, shortcuts)
            published = wanted
        }
    }
}
