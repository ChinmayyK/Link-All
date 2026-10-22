using System;
using System.IO;
using System.Threading.Tasks;

namespace LinkAll.WinUI.Services
{
    public class ScreenshotObserver : IDisposable
    {
        private readonly ClipboardManager _clipboardManager;
        private FileSystemWatcher? _desktopWatcher;
        private FileSystemWatcher? _screenshotsWatcher;
        private readonly string _desktopPath;
        private readonly string _screenshotsPath;
        // Windows creates PicturesScreenshots on the first Win+PrtScn, and a watcher
        // stops after a buffer overflow or a folder change; check back now and then.
        private readonly System.Threading.Timer _recheck;

        public ScreenshotObserver(ClipboardManager clipboardManager)
        {
            _clipboardManager = clipboardManager;

            string desktopPath = Environment.GetFolderPath(Environment.SpecialFolder.Desktop);
            string picturesPath = Environment.GetFolderPath(Environment.SpecialFolder.MyPictures);
            string screenshotsPath = Path.Combine(picturesPath, "Screenshots");

            _desktopPath = desktopPath;
            _screenshotsPath = screenshotsPath;
            EnsureWatchers();
            _recheck = new System.Threading.Timer(_ => EnsureWatchers(), null, 60_000, 60_000);
        }

        private readonly object _watchLock = new();

        private void EnsureWatchers()
        {
            lock (_watchLock)
            {
                _desktopWatcher ??= SetupWatcher(_desktopPath);
                _screenshotsWatcher ??= SetupWatcher(_screenshotsPath);
            }
        }

        private void OnWatcherError(object sender, ErrorEventArgs e)
        {
            lock (_watchLock)
            {
                if (sender == _desktopWatcher) { _desktopWatcher.Dispose(); _desktopWatcher = null; }
                else if (sender == _screenshotsWatcher) { _screenshotsWatcher.Dispose(); _screenshotsWatcher = null; }
            }
            EnsureWatchers();
        }

        private FileSystemWatcher? SetupWatcher(string path)
        {
            try
            {
                if (!Directory.Exists(path)) return null;
                var watcher = new FileSystemWatcher(path)
                {
                    NotifyFilter = NotifyFilters.FileName | NotifyFilters.LastWrite | NotifyFilters.CreationTime,
                    Filter = "*.*",
                    EnableRaisingEvents = true
                };
                watcher.Created += OnFileCreated;
                watcher.Error += OnWatcherError;
                return watcher;
            }
            catch
            {
                return null;
            }
        }

        // The file is still being written when Created fires; wait until it opens for
        // reading and its size has stopped changing.
        private static async Task<bool> WaitUntilWritten(string path)
        {
            long last = -1;
            for (int i = 0; i < 20; i++)
            {
                await Task.Delay(500);
                try
                {
                    if (!File.Exists(path)) return false;
                    using var fs = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.ReadWrite);
                    if (fs.Length > 0 && fs.Length == last) return true;
                    last = fs.Length;
                }
                catch (IOException) { last = -1; }
            }
            return false;
        }

        private void OnFileCreated(object sender, FileSystemEventArgs e)
        {
            try
            {
                if (string.IsNullOrEmpty(e.FullPath)) return;
                string ext = Path.GetExtension(e.FullPath)?.ToLowerInvariant() ?? "";
                if (ext == ".png" || ext == ".jpg" || ext == ".jpeg")
                {
                    bool isScreenshotFolder = e.FullPath.Contains("Screenshots", StringComparison.OrdinalIgnoreCase);
                    string name = Path.GetFileNameWithoutExtension(e.FullPath)?.ToLowerInvariant() ?? "";
                    
                    if (isScreenshotFolder || name.Contains("screenshot") || name.Contains("screen shot"))
                    {
                        Task.Run(async () =>
                        {
                            try
                            {
                                if (await WaitUntilWritten(e.FullPath))
                                {
                                    _clipboardManager.PushFile(e.FullPath);
                                    
                                    App.MainDispatcherQueue?.TryEnqueue(() => {
                                        NotificationHelper.ShowToast("Screenshot Synced", "Sent screenshot to your device.");
                                    });
                                }
                            }
                            catch (Exception ex) { App.HandleError(ex); } 
                        });
                    }
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        public void Dispose()
        {
            _recheck.Dispose();
            if (_desktopWatcher != null)
            {
                _desktopWatcher.EnableRaisingEvents = false;
                _desktopWatcher.Dispose();
            }
            if (_screenshotsWatcher != null)
            {
                _screenshotsWatcher.EnableRaisingEvents = false;
                _screenshotsWatcher.Dispose();
            }
        }
    }
}



