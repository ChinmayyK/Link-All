using System;
using System.Linq;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;

namespace LinkAll.WinUI
{
    /// <summary>A call-state report from a paired phone.</summary>
    /// <param name="State">"ringing", "offhook" or "idle".</param>
    public sealed record PhoneCall(string DeviceId, string DeviceName, string State, string Number, string ContactName);

    // One banner per call, driven by the phone's call-state reports: shown
    // while ringing, switched to an "on call" row with only End once the call
    // is picked up, closed when the phone goes idle. Answer and Decline act on
    // the phone itself; the audio stays there.
    public sealed partial class IncomingCallBannerWindow : Window
    {
        private static IncomingCallBannerWindow? _current;

        // If the phone drops off mid-ring, no "idle" ever arrives.
        private static readonly TimeSpan RingTimeout = TimeSpan.FromSeconds(75);

        private PhoneCall _call;
        private readonly Microsoft.UI.Windowing.AppWindow _appWindow;
        private readonly DispatcherTimer _ringTimer = new() { Interval = RingTimeout };
        // Ticks the "On call · 01:23" line once the call is picked up.
        private readonly DispatcherTimer _clock = new() { Interval = TimeSpan.FromSeconds(1) };
        private DateTime? _answeredAt;
        private readonly Microsoft.UI.Xaml.Media.FontFamily _initialsFont;

        public static void OnCallState(PhoneCall call)
        {
            switch (call.State)
            {
                case "ringing":
                    if (_current == null)
                    {
                        _current = new IncomingCallBannerWindow(call);
                        _current.ShowWithoutFocus();
                    }
                    else
                    {
                        _current.Update(call);
                    }
                    break;
                case "offhook":
                    // Only follow a call we announced; an outgoing call placed
                    // on the phone needs no banner here.
                    _current?.Update(call);
                    break;
                default:
                    _current?.Close();
                    break;
            }
        }

        private IncomingCallBannerWindow(PhoneCall call)
        {
            this.InitializeComponent();
            _call = call;
            _initialsFont = TxtInitials.FontFamily;
            // No ExtendsContentIntoTitleBar: with it, Windows still drew the
            // minimise, maximise and close buttons over the banner even though
            // the presenter has no title bar.
            LinkAll.WinUI.Services.ThemeService.Register(this);

            // Acrylic, like Windows' own notifications; Mica where it isn't there.
            if (Microsoft.UI.Composition.SystemBackdrops.DesktopAcrylicController.IsSupported())
            {
                this.SystemBackdrop = new Microsoft.UI.Xaml.Media.DesktopAcrylicBackdrop();
            }
            else if (Microsoft.UI.Composition.SystemBackdrops.MicaController.IsSupported())
            {
                this.SystemBackdrop = new Microsoft.UI.Xaml.Media.MicaBackdrop();
            }

            var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
            var windowId = Microsoft.UI.Win32Interop.GetWindowIdFromWindow(hwnd);
            _appWindow = Microsoft.UI.Windowing.AppWindow.GetFromWindowId(windowId);
            LinkAll.WinUI.Services.WindowIconHelper.Apply(_appWindow);
            _appWindow.IsShownInSwitchers = false;

            // Set to overlap without borders and un-resizable
            if (_appWindow.Presenter is Microsoft.UI.Windowing.OverlappedPresenter presenter)
            {
                presenter.IsAlwaysOnTop = true;
                presenter.IsResizable = false;
                presenter.IsMaximizable = false;
                presenter.IsMinimizable = false;
                presenter.SetBorderAndTitleBar(false, false);
            }
            // SetBorderAndTitleBar alone left a 32 px caption ("Incoming call"
            // and the icon) across the top, which squashed the card below it:
            // the avatar and buttons drew as pills and two lines of text were
            // cut off. Make it a plain popup with no caption or frame.
            var style = (long)GetWindowLongPtr(hwnd, GWL_STYLE);
            style &= ~(WS_CAPTION | WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX);
            style |= WS_POPUP;
            SetWindowLongPtr(hwnd, GWL_STYLE, (IntPtr)style);
            SetWindowPos(hwnd, IntPtr.Zero, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED);

            // A borderless window has square corners; ask Windows 11 for the
            // usual rounded ones (ignored on Windows 10).
            int round = DWMWCP_ROUND;
            DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, ref round, sizeof(int));
            LinkAll.WinUI.Services.WindowIconHelper.ResizeAndPlaceTopRightDips(_appWindow, hwnd, 400, 84, 16);

            _ringTimer.Tick += (_, _) => Close();
            _clock.Tick += (_, _) => ShowCallTime();
            Closed += (_, _) =>
            {
                _ringTimer.Stop();
                _clock.Stop();
                if (_current == this) _current = null;
            };

            Update(call);
        }

        private const int DWMWA_WINDOW_CORNER_PREFERENCE = 33;
        private const int DWMWCP_ROUND = 2;

        [System.Runtime.InteropServices.DllImport("dwmapi.dll")]
        private static extern int DwmSetWindowAttribute(IntPtr hwnd, int attribute, ref int value, int size);

        private const int GWL_STYLE = -16;
        private const long WS_POPUP = 0x80000000L;
        private const long WS_CAPTION = 0x00C00000L;
        private const long WS_THICKFRAME = 0x00040000L;
        private const long WS_SYSMENU = 0x00080000L;
        private const long WS_MINIMIZEBOX = 0x00020000L;
        private const long WS_MAXIMIZEBOX = 0x00010000L;
        private const uint SWP_NOSIZE = 0x0001, SWP_NOMOVE = 0x0002, SWP_NOZORDER = 0x0004,
            SWP_NOACTIVATE = 0x0010, SWP_FRAMECHANGED = 0x0020;

        [System.Runtime.InteropServices.DllImport("user32.dll", EntryPoint = "GetWindowLongPtrW")]
        private static extern IntPtr GetWindowLongPtr(IntPtr hWnd, int nIndex);
        [System.Runtime.InteropServices.DllImport("user32.dll", EntryPoint = "SetWindowLongPtrW")]
        private static extern IntPtr SetWindowLongPtr(IntPtr hWnd, int nIndex, IntPtr value);
        [System.Runtime.InteropServices.DllImport("user32.dll")]
        private static extern bool SetWindowPos(IntPtr hWnd, IntPtr after, int x, int y, int cx, int cy, uint flags);

        // A call banner must not take focus from whatever the user is typing in.
        private void ShowWithoutFocus()
        {
            _appWindow.Show(false);
        }

        private void Update(PhoneCall call)
        {
            // Android reports each state twice, the second time without the
            // number, so keep whatever caller details arrived first.
            _call = call with
            {
                Number = string.IsNullOrEmpty(call.Number) ? _call.Number : call.Number,
                ContactName = string.IsNullOrEmpty(call.ContactName) ? _call.ContactName : call.ContactName,
            };

            var phone = FirstNonEmpty(_call.DeviceName, "your phone");
            // A phone that can see neither name nor number (the Google Play
            // build without Notification access) says "Unknown caller"; which
            // phone is ringing is shown under it, never in the caller's place.
            var caller = FirstNonEmpty(_call.ContactName, _call.Number, "Unknown caller");
            TxtCallerName.Text = caller;
            var initials = Initials(_call.ContactName);
            TxtInitials.Text = initials ?? PhoneGlyph;
            TxtInitials.FontFamily = initials != null ? _initialsFont : IconFont;

            var ringing = _call.State == "ringing";
            // Under the caller: the number (when the name is shown above it)
            // and which phone it's on.
            var showNumber = !string.IsNullOrEmpty(_call.ContactName) && !string.IsNullOrEmpty(_call.Number);
            TxtSource.Text = showNumber ? $"{_call.Number} · {phone}" : $"on {phone}";
            if (ringing)
            {
                _answeredAt = null;
                _clock.Stop();
                TxtState.Text = "INCOMING CALL";
                TxtState.Foreground = AnswerGreen;
            }
            else
            {
                _answeredAt ??= DateTime.UtcNow;
                TxtState.Foreground = OnCallBlue;
                ShowCallTime();
                _clock.Start();
            }

            BtnAccept.Visibility = ringing ? Visibility.Visible : Visibility.Collapsed;
            BtnDecline.IsEnabled = true;
            BtnAccept.IsEnabled = true;
            Microsoft.UI.Xaml.Controls.ToolTipService.SetToolTip(BtnDecline, ringing ? "Decline" : "End call");
            Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(BtnDecline, ringing ? "Decline the call" : "End the call");

            if (ringing) { _ringTimer.Stop(); _ringTimer.Start(); }
            else _ringTimer.Stop();
        }

        private static readonly Microsoft.UI.Xaml.Media.SolidColorBrush AnswerGreen =
            new(Windows.UI.Color.FromArgb(255, 0x30, 0xD1, 0x58));
        private static readonly Microsoft.UI.Xaml.Media.SolidColorBrush OnCallBlue =
            new(Windows.UI.Color.FromArgb(255, 0x0A, 0x84, 0xFF));

        private void ShowCallTime()
        {
            if (_answeredAt is not { } at) return;
            var t = DateTime.UtcNow - at;
            TxtState.Text = t.TotalHours >= 1
                ? $"ON CALL · {(int)t.TotalHours}:{t.Minutes:00}:{t.Seconds:00}"
                : $"ON CALL · {t.Minutes:00}:{t.Seconds:00}";
        }

        private static string FirstNonEmpty(params string[] values) =>
            values.First(v => !string.IsNullOrWhiteSpace(v));

        private const string PhoneGlyph = "\uE717";
        private static readonly Microsoft.UI.Xaml.Media.FontFamily IconFont = new("Segoe Fluent Icons, Segoe MDL2 Assets");

        // Contact initials, or null when there is no name to take them from.
        private static string? Initials(string name)
        {
            var letters = name.Split(' ', StringSplitOptions.RemoveEmptyEntries)
                .Where(w => char.IsLetter(w[0]))
                .Take(2)
                .Select(w => char.ToUpperInvariant(w[0]));
            var initials = new string(letters.ToArray());
            return initials.Length > 0 ? initials : null;
        }

        private void BtnDecline_Click(object sender, RoutedEventArgs e) => SendAction("decline");

        private void BtnAccept_Click(object sender, RoutedEventArgs e) => SendAction("accept");

        // The phone answers with a new call state: "offhook" after an answer,
        // "idle" after a decline. The banner follows that rather than guessing.
        private async void SendAction(string action)
        {
            BtnDecline.IsEnabled = false;
            BtnAccept.IsEnabled = false;
            var handle = App.EngineHandle;
            var deviceId = _call.DeviceId;
            int result = -1;
            if (handle != IntPtr.Zero && !string.IsNullOrEmpty(deviceId))
            {
                result = await Task.Run(() => NativeCore.linkall_send_call_action(handle, action, deviceId));
            }
            if (result != 0)
            {
                TxtSource.Text = "Couldn't reach your phone";
                BtnDecline.IsEnabled = true;
                BtnAccept.IsEnabled = true;
                return;
            }
            // A phone that can't act (the Google Play build without Notification
            // access) never changes its call state, which would leave these
            // buttons dead. Say so after a few seconds and let them be pressed again.
            var stateBefore = _call.State;
            await Task.Delay(TimeSpan.FromSeconds(4));
            if (_current == this && _call.State == stateBefore)
            {
                TxtSource.Text = "Your phone couldn't do that. Use the phone.";
                BtnDecline.IsEnabled = true;
                BtnAccept.IsEnabled = true;
            }
        }
    }
}
