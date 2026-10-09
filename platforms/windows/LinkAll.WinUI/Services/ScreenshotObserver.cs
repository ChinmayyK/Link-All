using System;
using System.IO;
using System.Threading.Tasks;

namespace LinkAll.WinUI.Services
{
    public class ScreenshotObserver : IDisposable
    {
        private readonly ClipboardManager _clipboardManager;
        private readonly FileSystemWatcher? _desktopWatcher;
        private readonly FileSystemWatcher? _screenshotsWatcher;

        public ScreenshotObserver(ClipboardManager clipboardManager)
        {
            _clipboardManager = clipboardManager;

            string desktopPath = Environment.GetFolderPath(Environment.SpecialFolder.Desktop);
            string picturesPath = Environment.GetFolderPath(Environment.SpecialFolder.MyPictures);
            string screenshotsPath = Path.Combine(picturesPath, "Screenshots");

            _desktopWatcher = SetupWatcher(desktopPath);
            if (Directory.Exists(screenshotsPath))
            {
                _screenshotsWatcher = SetupWatcher(screenshotsPath);
            }
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
                return watcher;
            }
            catch
            {
                return null;
            }
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
                        Task.Delay(1000).ContinueWith(_ =>
                        {
                            try
                            {
                                if (File.Exists(e.FullPath))
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



