using System;
using System.Linq;
using Microsoft.Windows.AppNotifications;
using Microsoft.Windows.AppNotifications.Builder;

namespace LinkAll.WinUI
{
    public static class NotificationHelper
    {
        public const string AppUserModelID = "LinkAll.App.1";

        private static bool _registered;

        // Windows.UI.Notifications.ToastNotificationManager (the API this
        // used to use) never actually worked for this app: it requires
        // Windows to already recognize the AUMID via a Start-Menu shortcut
        // whose shortcut file carries a System.AppUserModel.ID property -
        // an unpackaged app with just a plain Desktop shortcut (no such
        // property) never gets that registered, so Show() silently did
        // nothing every single time, for every notification this app ever
        // tried to send. Confirmed: "LinkAll.App.1" never once appeared in
        // HKCU\...\Notifications\Settings despite months of ShowToast calls.
        // Microsoft.Windows.AppNotifications is WinAppSDK's purpose-built
        // replacement for exactly this unpackaged-app scenario - it only
        // needs AppNotificationManager.Default.Register(), no shortcut
        // shenanigans.
        //
        // Attempted once per process. A failure here is environmental (the
        // trace shows "module could not be found"), so retrying can't fix
        // it - and retrying on every toast subscribed another
        // NotificationInvoked handler and threw a COM exception each time,
        // tens of thousands of times during a warning storm. Show() still
        // works without registration; only button activation is lost.
        public static void EnsureRegistered()
        {
            if (_registered) return;
            _registered = true;
            try
            {
                AppNotificationManager.Default.NotificationInvoked += OnNotificationInvoked;
                if (Services.StartupRegistration.IsPackaged)
                {
                    // The Store package's notifications use its manifest logo.
                    AppNotificationManager.Default.Register();
                }
                else
                {
                    // Unpackaged, Windows took the exe's icon, the light tile, which
                    // looked wrong on a dark notification. The mark without its tile
                    // reads on light and dark alike.
                    var icon = System.IO.Path.Combine(AppContext.BaseDirectory, "Assets",
                        "Square44x44Logo.targetsize-256_altform-unplated.png");
                    if (System.IO.File.Exists(icon))
                        AppNotificationManager.Default.Register("Link All", new Uri(icon));
                    else
                        AppNotificationManager.Default.Register();
                }
                Trace("AppNotificationManager registered successfully");
            }
            catch (Exception ex)
            {
                Trace($"AppNotificationManager.Register FAILED: {ex}");
            }
        }

        public static void ShowToast(string title, string body, string? iconPath = null, string? launchArg = null)
        {
            try
            {
                EnsureRegistered();
                var builder = new AppNotificationBuilder()
                    .AddText(title)
                    .AddText(body);
                if (!string.IsNullOrEmpty(iconPath)) builder.SetAppLogoOverride(new Uri(iconPath));

                AppNotificationManager.Default.Show(builder.BuildNotification());
                Trace($"ShowToast succeeded: \"{title}\" / \"{body}\"");
            }
            catch (Exception ex)
            {
                Trace($"ShowToast FAILED: \"{title}\" - {ex}");
            }
        }

        /// <summary>
        /// The code in a phone notification Link All sent for a verification code: the
        /// title starts "Verification code" and the text is just 4 to 8 digits.
        /// </summary>
        public static string? VerificationCodeOf(string title, string text)
        {
            var code = text.Trim();
            return title.StartsWith("Verification code", StringComparison.Ordinal) && IsVerificationCode(code) ? code : null;
        }

        public static bool IsVerificationCode(string code) =>
            code.Length is >= 4 and <= 8 && code.All(c => c is >= '0' and <= '9');

        /// <summary>A popup with the code and "Copy code" and "Paste" buttons.</summary>
        public static void ShowCodeToast(string code, string from, string device)
        {
            try
            {
                EnsureRegistered();
                var builder = new AppNotificationBuilder()
                    .AddText($"Verification code: {code}")
                    .AddText(string.IsNullOrWhiteSpace(from) ? $"From {device}" : $"{from} · {device}")
                    .SetScenario(AppNotificationScenario.Reminder)
                    .AddButton(new AppNotificationButton("Copy code").AddArgument("action", $"linkall://copy-code/{code}"))
                    .AddButton(new AppNotificationButton("Paste").AddArgument("action", $"linkall://paste-code/{code}"));
                AppNotificationManager.Default.Show(builder.BuildNotification());
            }
            catch (Exception ex)
            {
                Trace($"ShowCodeToast FAILED: {ex}");
            }
        }

        [System.Runtime.InteropServices.DllImport("user32.dll")]
        private static extern void keybd_event(byte vk, byte scan, uint flags, UIntPtr extra);

        /// <summary>Puts the code on the clipboard; with <paramref name="paste"/>, also presses Ctrl+V in the focused app.</summary>
        public static void CopyCode(string code, bool paste)
        {
            var package = new Windows.ApplicationModel.DataTransfer.DataPackage();
            package.SetText(code);
            Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(package);
            if (!paste) return;
            // Let the clipboard take the code and the popup give focus back first.
            _ = System.Threading.Tasks.Task.Run(async () =>
            {
                await System.Threading.Tasks.Task.Delay(300);
                const byte VK_CONTROL = 0x11, VK_V = 0x56;
                const uint KEYUP = 0x2;
                keybd_event(VK_CONTROL, 0, 0, UIntPtr.Zero);
                keybd_event(VK_V, 0, 0, UIntPtr.Zero);
                keybd_event(VK_V, 0, KEYUP, UIntPtr.Zero);
                keybd_event(VK_CONTROL, 0, KEYUP, UIntPtr.Zero);
            });
        }

        public static void ShowToastWithActions(string title, string body, string? iconPath, string acceptUrl, string rejectUrl)
        {
            try
            {
                EnsureRegistered();
                var builder = new AppNotificationBuilder()
                    .AddText(title)
                    .AddText(body)
                    .AddButton(new AppNotificationButton("Accept").AddArgument("action", acceptUrl))
                    .AddButton(new AppNotificationButton("Reject").AddArgument("action", rejectUrl));
                if (!string.IsNullOrEmpty(iconPath)) builder.SetAppLogoOverride(new Uri(iconPath));

                AppNotificationManager.Default.Show(builder.BuildNotification());
                Trace($"ShowToastWithActions succeeded: \"{title}\" / \"{body}\"");
            }
            catch (Exception ex)
            {
                Trace($"ShowToastWithActions FAILED: \"{title}\" - {ex}");
            }
        }

        // Fires when a notification button is clicked while the app is
        // already running (the common case, since Link All lives in the
        // tray). Cold-launch clicks arrive instead via App.xaml.cs's
        // ProcessActivationArgs (ExtendedActivationKind.AppNotification) -
        // both paths funnel into App.HandleLinkAllUri for one shared
        // accept/reject dispatch.
        private static void OnNotificationInvoked(AppNotificationManager sender, AppNotificationActivatedEventArgs args)
        {
            if (args.Arguments.TryGetValue("action", out var action) && !string.IsNullOrEmpty(action))
            {
                App.MainDispatcherQueue?.TryEnqueue(() => App.HandleLinkAllUri(action));
            }
        }

        private static void Trace(string message)
        {
            try
            {
                TraceLog.Write(message);
                TraceLog.Flush();
            }
            catch { }
        }
    }
}
