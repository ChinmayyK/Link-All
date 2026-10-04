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
            ExtendsContentIntoTitleBar = true;
            SetTitleBar(AppTitleBar);
            LinkAll.WinUI.Services.ThemeService.Register(this);

            if (Microsoft.UI.Composition.SystemBackdrops.MicaController.IsSupported())
            {
                this.SystemBackdrop = new Microsoft.UI.Xaml.Media.MicaBackdrop();
            }
            else if (Microsoft.UI.Composition.SystemBackdrops.DesktopAcrylicController.IsSupported())
            {
                this.SystemBackdrop = new Microsoft.UI.Xaml.Media.DesktopAcrylicBackdrop();
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
            LinkAll.WinUI.Services.WindowIconHelper.ResizeAndPlaceTopRightDips(_appWindow, hwnd, 360, 100, 16);

            _ringTimer.Tick += (_, _) => Close();
            Closed += (_, _) =>
            {
                _ringTimer.Stop();
                if (_current == this) _current = null;
            };

            Update(call);
        }

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

            var caller = FirstNonEmpty(_call.ContactName, _call.Number, "Unknown caller");
            var phone = FirstNonEmpty(_call.DeviceName, "your phone");
            TxtCallerName.Text = caller;
            var initials = Initials(_call.ContactName);
            TxtInitials.Text = initials ?? PhoneGlyph;
            TxtInitials.FontFamily = initials != null ? _initialsFont : IconFont;

            var ringing = _call.State == "ringing";
            TxtDetail.Text = ringing
                ? (string.IsNullOrEmpty(_call.ContactName) || string.IsNullOrEmpty(_call.Number)
                    ? $"Incoming call on {phone}"
                    : $"{_call.Number} · {phone}")
                : $"On call · {phone}";

            BtnAccept.Visibility = ringing ? Visibility.Visible : Visibility.Collapsed;
            BtnDecline.IsEnabled = true;
            BtnAccept.IsEnabled = true;
            Microsoft.UI.Xaml.Controls.ToolTipService.SetToolTip(BtnDecline, ringing ? "Decline" : "End call");
            Microsoft.UI.Xaml.Automation.AutomationProperties.SetName(BtnDecline, ringing ? "Decline the call" : "End the call");

            if (ringing) { _ringTimer.Stop(); _ringTimer.Start(); }
            else _ringTimer.Stop();
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
                TxtDetail.Text = "Couldn't reach your phone";
                BtnDecline.IsEnabled = true;
                BtnAccept.IsEnabled = true;
            }
        }
    }
}
