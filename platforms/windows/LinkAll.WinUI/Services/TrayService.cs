using System;
using System.Diagnostics;
using System.IO;
using System.IO.Pipes;
using Microsoft.UI.Xaml;

namespace LinkAll.WinUI.Services
{
    public class TrayService : IDisposable
    {
        public static TrayService? Current { get; private set; }

        private readonly System.Threading.Timer _watchdogTimer;
        private int _consecutiveIpcFailures = 0;

        public TrayService()
        {
            Current = this;
            EnsureTrayProcessRunning();
            StartCommandListener();

            // The tray helper must never stay dead or hung - poll its
            // liveness (process exists) and responsiveness (IPC pipe
            // accepts a connection) and respawn it if either check fails.
            _watchdogTimer = new System.Threading.Timer(
                _ => CheckTrayHealth(),
                null,
                TimeSpan.FromSeconds(20),
                TimeSpan.FromSeconds(20));
        }

        // Reverse channel from the Tray process (context-menu clicks) to
        // this main process - mirrors the outbound "LinkAll_Tray_Pipe"
        // protocol exactly, just in the other direction, so tray menu items
        // beyond "open the window" (Quick Access, Settings) can actually
        // reach real app state instead of only being able to foreground it.
        private void StartCommandListener()
        {
            var thread = new System.Threading.Thread(() =>
            {
                // One server instance for the life of the loop, disconnected
                // between commands, so the pipe never vanishes between two
                // clients (the tray would see ERROR_FILE_NOT_FOUND and fall
                // back to opening the main window). Rebuilt only after a fault.
                NamedPipeServerStream? server = null;
                while (!App.IsShuttingDown)
                {
                    try
                    {
                        server ??= new NamedPipeServerStream("LinkAll_Main_Commands", PipeDirection.In);
                        server.WaitForConnection();
                        string? line;
                        using (var reader = new StreamReader(server, leaveOpen: true))
                            line = reader.ReadLine();
                        server.Disconnect();
                        if (string.IsNullOrEmpty(line)) continue;

                        var queue = App.MainDispatcherQueue ?? Microsoft.UI.Dispatching.DispatcherQueue.GetForCurrentThread();
                        queue?.TryEnqueue(() =>
                        {
                            try
                            {
                                if (line == "QUICKACCESS")
                                {
                                    new QuickAccessWindow().Activate();
                                }
                                else if (line == "SETTINGS")
                                {
                                    ((App)App.Current).ShowMainWindowCommand?.Execute(null);
                                    DashboardWindow.Current?.NavigateTo("Settings");
                                }
                                else if (line == "RESCAN")
                                {
                                    DaemonActions.RunFireAndForget("Rescan", () => DaemonClient.RescanPeers());
                                }
                            }
                            catch (Exception ex) { App.HandleError(ex); }
                        });
                    }
                    catch
                    {
                        server?.Dispose();
                        server = null;
                        System.Threading.Thread.Sleep(1000);
                    }
                }
            })
            { IsBackground = true };
            thread.Start();
        }

        public TrayService(IntPtr unusedHwnd) : this()
        {
        }

        private void CheckTrayHealth()
        {
            try
            {
                var procs = Process.GetProcessesByName("LinkAll.Tray");
                if (procs.Length == 0)
                {
                    _consecutiveIpcFailures = 0;
                    EnsureTrayProcessRunning();
                    return;
                }

                if (IsIpcResponsive())
                {
                    _consecutiveIpcFailures = 0;
                    return;
                }

                _consecutiveIpcFailures++;
                // Require a couple of consecutive failures before acting -
                // a single missed connect can just be transient contention.
                if (_consecutiveIpcFailures >= 2)
                {
                    _consecutiveIpcFailures = 0;
                    foreach (var proc in procs)
                    {
                        try { proc.Kill(); } catch { }
                    }
                    EnsureTrayProcessRunning();
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private bool IsIpcResponsive()
        {
            try
            {
                using var client = new NamedPipeClientStream(".", "LinkAll_Tray_Pipe", PipeDirection.Out);
                client.Connect(500);
                return true;
            }
            catch { return false; }
        }

        private void EnsureTrayProcessRunning()
        {
            try
            {
                var procs = Process.GetProcessesByName("LinkAll.Tray");
                if (procs.Length == 0)
                {
                    var baseDir = AppContext.BaseDirectory;
                    // The native tray helper (platforms/windows/tray), copied into
                    // tray\ by the CopyTrayApp / PublishTrayApp build targets.
                    var trayExe = Path.Combine(baseDir, "tray", "LinkAll.Tray.exe");
                    if (!File.Exists(trayExe))
                    {
                        // Fallback: same directory (dev/debug scenario)
                        trayExe = Path.Combine(baseDir, "LinkAll.Tray.exe");
                    }
                    if (File.Exists(trayExe))
                    {
                        Process.Start(new ProcessStartInfo
                        {
                            FileName = trayExe,
                            UseShellExecute = true
                        });
                    }
                }
            }
            catch (Exception ex)
            {
                App.HandleError(ex);
            }
        }

        public void UpdateTooltip(string text)
        {
            SendIpcMessage("TIP:" + text);
        }

        public void ShowNotification(string title, string text)
        {
            SendIpcMessage("NOTIFY:" + text);
        }

        private void SendIpcMessage(string message)
        {
            System.Threading.Tasks.Task.Run(() =>
            {
                try
                {
                    using var client = new NamedPipeClientStream(".", "LinkAll_Tray_Pipe", PipeDirection.Out);
                    client.Connect(200);
                    using var writer = new StreamWriter(client);
                    writer.WriteLine(message);
                    writer.Flush();
                }
                catch { }
            });
        }

        public void Dispose()
        {
            try { _watchdogTimer?.Dispose(); } catch { }

            try
            {
                // Ask the tray process to exit gracefully first (synchronously,
                // so we don't race the fallback Kill() below against an
                // async fire-and-forget send).
                try
                {
                    using var client = new NamedPipeClientStream(".", "LinkAll_Tray_Pipe", PipeDirection.Out);
                    client.Connect(200);
                    using var writer = new StreamWriter(client);
                    writer.WriteLine("QUIT");
                    writer.Flush();
                }
                catch { }

                foreach (var proc in Process.GetProcessesByName("LinkAll.Tray"))
                {
                    try
                    {
                        if (!proc.WaitForExit(500))
                        {
                            proc.Kill();
                        }
                    }
                    catch { }
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
        }
    }
}
