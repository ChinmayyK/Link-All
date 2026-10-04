using Microsoft.UI.Dispatching;
using Windows.ApplicationModel;
using Windows.ApplicationModel.Activation;
using Windows.Foundation;
using Windows.Foundation.Collections;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Controls.Primitives;
using Microsoft.UI.Xaml.Data;
using Microsoft.UI.Xaml.Input;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Navigation;
using Microsoft.UI.Xaml.Shapes;
using System;

namespace Deskdrop.WinUI;

public partial class App : Application
{
    public static Window MainWindow { get; private set; }
    private Window? _window;
    
    // Tray Icon Properties
    public static Microsoft.UI.Dispatching.DispatcherQueue? MainDispatcherQueue { get; private set; }
    public static bool IsShuttingDown { get; private set; } = false;
    
    // Commands
    public System.Windows.Input.ICommand ShowMainWindowCommand { get; }
    public System.Windows.Input.ICommand ExitApplicationCommand { get; }

    private static IntPtr _engineHandle = IntPtr.Zero;
    public static IntPtr EngineHandle => _engineHandle;
    public static Deskdrop.WinUI.Services.ClipboardManager? Clipboard { get; private set; }
    public static System.Threading.Tasks.TaskCompletionSource<Deskdrop.WinUI.Services.ClipboardManager> ClipboardReady { get; } = new();
    private static Deskdrop.WinUI.Services.GlobalDragMonitor? _dragMonitor;
    private static Deskdrop.WinUI.Services.ScreenshotObserver? _screenshotObserver;

    // Lets Settings flip screenshot auto-sync on/off live, without an app
    // restart, while still persisting the choice for next launch.
    public static void SetScreenshotSyncEnabled(bool enabled)
    {
        try
        {
            Deskdrop.WinUI.Services.LocalSettingsStore.SetBool("ScreenshotSyncEnabled", enabled);
            if (enabled && _screenshotObserver == null && Clipboard != null)
            {
                _screenshotObserver = new Deskdrop.WinUI.Services.ScreenshotObserver(Clipboard);
            }
            else if (!enabled && _screenshotObserver != null)
            {
                _screenshotObserver.Dispose();
                _screenshotObserver = null;
            }
        }
        catch (Exception ex) { App.HandleError(ex); }
    }

    // Show phone notifications mirrored from Android as Windows toasts. On by default, as on macOS.
    public static bool PhoneNotificationMirroringEnabled =>
        Deskdrop.WinUI.Services.LocalSettingsStore.GetBool("PhoneNotificationMirroringEnabled", true);

    public static void SetPhoneNotificationMirroringEnabled(bool enabled) =>
        Deskdrop.WinUI.Services.LocalSettingsStore.SetBool("PhoneNotificationMirroringEnabled", enabled);

    public static bool ScreenshotSyncEnabled =>
        Deskdrop.WinUI.Services.LocalSettingsStore.GetBool("ScreenshotSyncEnabled");

    [System.Runtime.InteropServices.DllImport("shell32.dll", CharSet = System.Runtime.InteropServices.CharSet.Unicode)]
    private static extern int SetCurrentProcessExplicitAppUserModelID(string AppID);

    public App()
    {
        MainDispatcherQueue = Microsoft.UI.Dispatching.DispatcherQueue.GetForCurrentThread();

        TraceLog.Write("App constructor started");

        // Unpackaged Win32/WinUI3 apps need to explicitly claim an
        // AppUserModelID before the notification platform will reliably
        // deliver toasts for that ID (NotificationHelper.AppUserModelID) -
        // without this, ToastNotifier.Show() can silently no-op instead of
        // throwing, which is very easy to mistake for "notifications are
        // broken" when it's really just missing identity registration.
        try { SetCurrentProcessExplicitAppUserModelID(NotificationHelper.AppUserModelID); }
        catch (Exception ex) { App.HandleError(ex); }

        this.UnhandledException += (s, e) =>
        {
            e.Handled = true;
            try { TraceLog.Write($"WinUI UnhandledException: {e.Exception?.Message}\n{e.Exception?.StackTrace}"); TraceLog.Flush(); } catch (Exception ex) { App.HandleError(ex); }
        };
        AppDomain.CurrentDomain.UnhandledException += (s, e) =>
        {
            try { TraceLog.Write($"AppDomain UnhandledException: {(e.ExceptionObject as Exception)?.Message}"); TraceLog.Flush(); } catch (Exception ex) { App.HandleError(ex); }
        };
        System.Threading.Tasks.TaskScheduler.UnobservedTaskException += (s, e) =>
        {
            e.SetObserved();
            try { TraceLog.Write($"TaskScheduler UnobservedTaskException: {e.Exception?.Message}"); TraceLog.Flush(); } catch (Exception ex) { App.HandleError(ex); }
        };

        InitializeComponent();

        TraceLog.Write("App InitializeComponent finished");

        ShowMainWindowCommand = new RelayCommand(() =>
        {
            var queue = MainDispatcherQueue ?? Microsoft.UI.Dispatching.DispatcherQueue.GetForCurrentThread();
            queue?.TryEnqueue(() =>
            {
                try
                {
                    if (MainWindow == null || DashboardWindow.Current == null)
                    {
                        MainWindow = new DashboardWindow();
                        _window = MainWindow;
                        MainWindow.Activate();
                    }
                    else
                    {
                        try
                        {
                            var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(MainWindow);
                            ShowWindow(hwnd, 9 /* SW_RESTORE */);
                            SetForegroundWindow(hwnd);
                            MainWindow.Activate();
                        }
                        catch
                        {
                            MainWindow = new DashboardWindow();
                            _window = MainWindow;
                            MainWindow.Activate();
                        }
                    }
                }
                catch (Exception ex)
                {
                    App.HandleError(ex);
                }
            });
        });
        
        ExitApplicationCommand = new RelayCommand(() =>
        {
            IsShuttingDown = true;
            var queue = MainDispatcherQueue ?? Microsoft.UI.Dispatching.DispatcherQueue.GetForCurrentThread();
            queue?.TryEnqueue(() =>
            {
                try { Deskdrop.WinUI.Services.TrayService.Current?.Dispose(); } catch (Exception ex) { App.HandleError(ex); }
                if (_engineHandle != IntPtr.Zero)
                {
                    try { Deskdrop.WinUI.NativeCore.deskdrop_stop(_engineHandle); _engineHandle = IntPtr.Zero; } catch (Exception ex) { App.HandleError(ex); }
                }
                try { GlobalHotKeyManager.Shared.Dispose(); } catch (Exception ex) { App.HandleError(ex); }
                try { Application.Current.Exit(); } catch (Exception ex) { App.HandleError(ex); }
                TraceLog.Flush();
                Environment.Exit(0);
            });
        });
    }

    protected override void OnLaunched(Microsoft.UI.Xaml.LaunchActivatedEventArgs args)
    {
        TraceLog.Write("OnLaunched started");

        try
        {
            // Single instance via AppInstance key redirection (not a raw
            // Mutex): a Mutex can only tell a second launch "someone's
            // already running" and exit - it has no way to hand that
            // second launch's activation args (e.g. the file path from
            // Explorer's "Send via Deskdrop") to the first instance. This
            // redirects the whole AppActivationArguments to the existing
            // instance's OnAppActivated, so ProcessActivationArgs actually
            // sees it instead of the file silently getting dropped whenever
            // Deskdrop was already running in the tray (the common case).
            _keyInstance = Microsoft.Windows.AppLifecycle.AppInstance.FindOrRegisterForKey("Deskdrop_Main_Instance");
            var activatedArgs = Microsoft.Windows.AppLifecycle.AppInstance.GetCurrent().GetActivatedEventArgs();

            if (!_keyInstance.IsCurrent)
            {
                TraceLog.Write("Secondary instance detected. Redirecting activation to existing instance.");
                _keyInstance.RedirectActivationToAsync(activatedArgs).AsTask().Wait();
                var existingHwnd = FindWindowW(null, "Link All");
                if (existingHwnd == IntPtr.Zero) existingHwnd = FindWindowW(null, "DeskDrop Dashboard");
                if (existingHwnd != IntPtr.Zero)
                {
                    ShowWindow(existingHwnd, 9 /* SW_RESTORE */);
                    SetForegroundWindow(existingHwnd);
                }
                TraceLog.Flush();
                Environment.Exit(0);
                return;
            }

            _keyInstance.Activated += OnAppActivated;

            Deskdrop.WinUI.Native.ContextMenuIntegration.RegisterContextMenu();
            Deskdrop.WinUI.Native.ContextMenuIntegration.RegisterUriProtocol();
            NotificationHelper.EnsureRegistered();

            // Process initial launch arguments
            ProcessActivationArgs(activatedArgs);

            // Best-effort: add the firewall rules Deskdrop needs for LAN
            // discovery/transfer if they're missing. May trigger a UAC
            // prompt on first run - never block startup on it.
            System.Threading.Tasks.Task.Run(() =>
            {
                try { Deskdrop.WinUI.FirewallHelper.EnsureRules(); }
                catch (Exception ex) { App.HandleError(ex); }
            });

            // Initialize the in-process native core FFI
            System.Threading.Tasks.Task.Run(() =>
            {
                try
                {
                    _engineHandle = Deskdrop.WinUI.NativeCore.deskdrop_start(Deskdrop.WinUI.Services.LocalSettingsStore.DeviceName, 0);
                    TraceLog.Write("Native engine started gracefully. Handle: " + _engineHandle);
                }
                catch (Exception ex)
                {
                    TraceLog.Write("Native engine start failed: " + ex.Message);
                    TraceLog.Flush();
                }
            });

            Clipboard = new Deskdrop.WinUI.Services.ClipboardManager();
            ClipboardReady.TrySetResult(Clipboard);

            try
            {
                _ = new Deskdrop.WinUI.Services.TrayService();
            }
            catch (Exception ex) { App.HandleError(ex); }

            try
            {
                _dragMonitor = new Deskdrop.WinUI.Services.GlobalDragMonitor(Clipboard);
            }
            catch (Exception ex) { App.HandleError(ex); }

            MainDispatcherQueue ??= Microsoft.UI.Dispatching.DispatcherQueue.GetForCurrentThread();

            try
            {
                UpgradeStartupEntry();
                if (IsBackgroundLaunch(activatedArgs))
                {
                    // Started at sign-in: stay in the tray, like the Mac app in
                    // the menu bar. The tray or a second launch opens the window.
                    TraceLog.Write("Background launch: engine and tray only, no window");
                }
                else
                {
                    MainWindow = new DashboardWindow();
                    _window = MainWindow;
                    _window.Activate();

                    var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(MainWindow);
                    ShowWindow(hwnd, 5 /* SW_SHOW */);
                    SetForegroundWindow(hwnd);

                    TraceLog.Write("MainWindow created, activated, and displayed successfully");
                    ShowOnboarding(force: false);
                }
            }
            catch (Exception ex)
            {
                TraceLog.Write("MainWindow crash: " + ex.ToString());
                if (ex.InnerException != null) TraceLog.Write("Inner: " + ex.InnerException.ToString());
                TraceLog.Flush();
            }

            try
            {
                if (ScreenshotSyncEnabled)
                {
                    _screenshotObserver = new Deskdrop.WinUI.Services.ScreenshotObserver(Clipboard);
                }
            }
            catch (Exception ex) { App.HandleError(ex); }

            try
            {
                // Ctrl+Shift+V: Quick Access (clipboard timeline + device list)
                GlobalHotKeyManager.Shared.Register(true, true, false, false, 0x56, () => {
                    var queue = MainDispatcherQueue ?? Microsoft.UI.Dispatching.DispatcherQueue.GetForCurrentThread();
                    queue?.TryEnqueue(() => { try { new QuickAccessWindow().Activate(); } catch (Exception ex) { App.HandleError(ex); } });
                });
                // Ctrl+K: bring the main Dashboard window to the front
                GlobalHotKeyManager.Shared.Register(true, false, false, false, 0x4B, () => {
                    ShowMainWindowCommand?.Execute(null);
                });
            }
            catch (Exception ex) { App.HandleError(ex); }
        }
        catch (Exception ex)
        {
            TraceLog.Write("Exception in OnLaunched: " + ex.Message + "\n" + ex.StackTrace);
            TraceLog.Flush();
        }
    }
    private const string OnboardingDoneKey = "HasCompletedOnboarding";
    private static OnboardingWindow? _onboarding;

    // First run: the welcome guide over the dashboard, so a new user sees how
    // to pair instead of an empty page. Once done (finished, skipped, or a
    // device paired) it stays away unless Settings asks for it again. People
    // who paired before this guide existed never see it: it closes itself as
    // soon as the dashboard reports a paired device.
    public static void ShowOnboarding(bool force)
    {
        try
        {
            if (!force && Deskdrop.WinUI.Services.LocalSettingsStore.GetBool(OnboardingDoneKey, false)) return;
            if (_onboarding != null) { _onboarding.Activate(); return; }
            var window = new OnboardingWindow(closeWhenPaired: !force);
            window.Closed += (_, _) =>
            {
                Deskdrop.WinUI.Services.LocalSettingsStore.SetBool(OnboardingDoneKey, true);
                _onboarding = null;
            };
            _onboarding = window;
            window.Activate();
        }
        catch (Exception ex) { App.HandleError(ex); }
    }

    private void OnAppActivated(object? sender, Microsoft.Windows.AppLifecycle.AppActivationArguments e)
    {
        ProcessActivationArgs(e);
        // Opening Deskdrop again while it runs in the tray (it may have started
        // hidden at sign-in) shows the window; a second process can't, since
        // there may be no window yet for it to find.
        if (e.Kind == Microsoft.Windows.AppLifecycle.ExtendedActivationKind.Launch && !IsBackgroundLaunch(e))
            ShowMainWindowCommand?.Execute(null);
    }

    // "--background" (what the sign-in entry passes), "--startup" or
    // "--minimized": start the engine and tray, but no window.
    private static readonly string[] BackgroundFlags = { "--background", "--startup", "--minimized" };

    private static bool IsBackgroundLaunch(Microsoft.Windows.AppLifecycle.AppActivationArguments? args)
    {
        string? line = (args?.Data as Windows.ApplicationModel.Activation.ILaunchActivatedEventArgs)?.Arguments;
        var words = (line ?? "").Split(' ', StringSplitOptions.RemoveEmptyEntries)
            .Concat(Environment.GetCommandLineArgs().Skip(1));
        return words.Any(w => BackgroundFlags.Contains(w.Trim('"'), StringComparer.OrdinalIgnoreCase));
    }

    // Older builds wrote the sign-in entry without --background, so every
    // sign-in opened the dashboard. Rewrite it once if it's there.
    private static void UpgradeStartupEntry()
    {
        try
        {
            using var key = Microsoft.Win32.Registry.CurrentUser.OpenSubKey(@"Software\Microsoft\Windows\CurrentVersion\Run", writable: true);
            if (key?.GetValue("Deskdrop") is string value && !value.Contains("--background"))
                key.SetValue("Deskdrop", value.TrimEnd() + " --background");
        }
        catch (Exception ex) { App.HandleError(ex); }
    }

    // Shared dispatch for deskdrop://accept/{id} and deskdrop://reject/{id}
    // (still used as plain argument strings even though the notification
    // API that carries them changed) - called from both a cold launch
    // (ProcessActivationArgs, AppNotification/Protocol kinds) and a click
    // while already running (NotificationHelper.OnNotificationInvoked).
    public static void HandleDeskdropUri(string uriString)
    {
        if (!Uri.TryCreate(uriString, UriKind.Absolute, out var uri) || uri.Scheme != "deskdrop") return;

        if (uri.Host == "pair-accept" || uri.Host == "pair-reject")
        {
            var deviceId = uri.AbsolutePath.Trim('/');
            if (!string.IsNullOrEmpty(deviceId))
                DeskdropStore.Shared.RespondToPairing(deviceId, uri.Host == "pair-accept");
            return;
        }

        if (uri.Host != "accept" && uri.Host != "reject") return;

        var transferId = uri.AbsolutePath.Trim('/');
        if (string.IsNullOrEmpty(transferId)) return;

        if (uri.Host == "accept")
            DaemonActions.RunFireAndForget("Accept Transfer", () => DaemonClient.AcceptFileTransfer(transferId));
        else
            DaemonActions.RunFireAndForget("Reject Transfer", () => DaemonClient.RejectFileTransfer(transferId, "user_declined"));
    }

    private void ProcessActivationArgs(Microsoft.Windows.AppLifecycle.AppActivationArguments activatedArgs)
    {
        if (activatedArgs.Kind == Microsoft.Windows.AppLifecycle.ExtendedActivationKind.Protocol)
        {
            var protocolArgs = activatedArgs.Data as Windows.ApplicationModel.Activation.IProtocolActivatedEventArgs;
            var uri = protocolArgs?.Uri;
            if (uri != null)
            {
                TraceLog.Write("Activated via protocol: " + uri.ToString());
                HandleDeskdropUri(uri.ToString());
            }
        }
        else if (activatedArgs.Kind == Microsoft.Windows.AppLifecycle.ExtendedActivationKind.AppNotification)
        {
            // Cold-launch equivalent of NotificationHelper.OnNotificationInvoked:
            // the app wasn't running when an Accept/Reject notification
            // button was clicked, so Windows launched it fresh with this
            // activation kind instead of raising that in-process event.
            if (activatedArgs.Data is Microsoft.Windows.AppNotifications.AppNotificationActivatedEventArgs notifArgs
                && notifArgs.Arguments.TryGetValue("action", out var action) && !string.IsNullOrEmpty(action))
            {
                TraceLog.Write("Activated via AppNotification: " + action);
                HandleDeskdropUri(action);
            }
        }
        else if (activatedArgs.Kind == Microsoft.Windows.AppLifecycle.ExtendedActivationKind.CommandLineLaunch)
        {
            var cmdLineArgs = activatedArgs.Data as Windows.ApplicationModel.Activation.ICommandLineActivatedEventArgs;
            if (cmdLineArgs != null)
            {
                string argsStr = cmdLineArgs.Operation.Arguments;
                TraceLog.Write("Activated via CommandLine: " + argsStr);

                var matches = System.Text.RegularExpressions.Regex.Matches(argsStr, @"[\""].+?[\""]|[^ ]+");
                if (matches.Count >= 2)
                {
                    string path = matches[matches.Count - 1].Value.Trim('"');
                    if (System.IO.File.Exists(path) || System.IO.Directory.Exists(path))
                    {
                        TraceLog.Write("Context Menu Trigger: Sending " + path);

                        System.Threading.Tasks.Task.Run(async () =>
                        {
                            var clipboard = await ClipboardReady.Task;
                            try
                            {
                                clipboard.PushFile(path);
                            }
                            catch (Exception e)
                            {
                                TraceLog.Write($"Error pushing context menu file: {e.Message}");
                                TraceLog.Flush();
                            }
                        });
                    }
                }
            }
        }
    }

    private static Microsoft.Windows.AppLifecycle.AppInstance? _keyInstance;

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern bool SetForegroundWindow(IntPtr hWnd);

    [System.Runtime.InteropServices.DllImport("user32.dll")]
    private static extern bool ShowWindow(IntPtr hWnd, int nCmdShow);

    [System.Runtime.InteropServices.DllImport("user32.dll", SetLastError = true, CharSet = System.Runtime.InteropServices.CharSet.Unicode)]
    private static extern IntPtr FindWindowW(string? lpClassName, string? lpWindowName);

    public static void HandleError(Exception ex, [System.Runtime.CompilerServices.CallerMemberName] string callerName = "")
    {
        try
        {
            TraceLog.Write($"Swallowed Exception in {callerName}: {ex.Message}\n{ex.StackTrace}");
            TraceLog.Flush();
        }
        catch { } // Failsafe
    }
}

public class RelayCommand : System.Windows.Input.ICommand
{
    private readonly Action _execute;
    public RelayCommand(Action execute) => _execute = execute;
    public event EventHandler? CanExecuteChanged { add { } remove { } }
    public bool CanExecute(object? parameter) => true;
    public void Execute(object? parameter) => _execute();
}

