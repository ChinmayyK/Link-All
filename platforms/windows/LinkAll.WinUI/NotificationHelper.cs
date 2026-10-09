using System;
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
                AppNotificationManager.Default.Register();
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
