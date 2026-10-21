using System;
using System.Threading.Tasks;

namespace LinkAll.WinUI.Services
{
    /// <summary>
    /// "Insert from Phone": a picture asked of the phone is put on the clipboard and
    /// pasted where the user was typing, when it arrives within ten minutes.
    /// </summary>
    public static class NotificationHelperPictures
    {
        private static DateTime _askedAt = DateTime.MinValue;

        public static void MarkAsked() => _askedAt = DateTime.UtcNow;

        /// <summary>True once for a picture file named by the phone that arrives after being asked for.</summary>
        public static bool TakePending(string fileName)
        {
            if (!fileName.StartsWith("LinkAll Capture ", StringComparison.Ordinal)) return false;
            if (DateTime.UtcNow - _askedAt > TimeSpan.FromMinutes(10)) return false;
            _askedAt = DateTime.MinValue;
            return true;
        }

        [System.Runtime.InteropServices.DllImport("user32.dll")]
        private static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);

        /// <summary>Puts the picture on the clipboard and presses Ctrl+V in the focused app.</summary>
        public static async Task InsertAsync(string path)
        {
            var file = await Windows.Storage.StorageFile.GetFileFromPathAsync(path);
            var package = new Windows.ApplicationModel.DataTransfer.DataPackage();
            package.SetBitmap(Windows.Storage.Streams.RandomAccessStreamReference.CreateFromFile(file));
            Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(package);
            // Let the clipboard take the picture first.
            await Task.Delay(500);
            const byte VK_CONTROL = 0x11, VK_V = 0x56;
            const uint KEYUP = 0x2;
            keybd_event(VK_CONTROL, 0, 0, UIntPtr.Zero);
            keybd_event(VK_V, 0, 0, UIntPtr.Zero);
            keybd_event(VK_V, 0, KEYUP, UIntPtr.Zero);
            keybd_event(VK_CONTROL, 0, KEYUP, UIntPtr.Zero);
        }
    }
}
