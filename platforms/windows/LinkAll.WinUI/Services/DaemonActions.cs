using System;
using System.Text.Json;
using System.Threading.Tasks;

namespace LinkAll.WinUI
{
    /// <summary>
    /// Surfaces daemon command failures to the user instead of letting them
    /// fail silently. Most mutating DaemonClient calls (Rescan, Rename,
    /// Pause/Resume, drag-and-drop send, remote file actions, etc.) used to
    /// discard their response entirely - if the daemon was down or rejected
    /// the request, the button just appeared to do nothing.
    /// </summary>
    public static class DaemonActions
    {
        // Call after an already-awaited Send()/SendAsync() response is in hand.
        // Every daemon response is wrapped as {"status":"ok"/"error",...}
        // (linkall-core/src/ipc.rs IpcResponse) - a null response means the
        // daemon was unreachable, an "error" status means it rejected the
        // request.
        public static void ReportIfFailed(string actionLabel, JsonDocument? response)
        {
            if (response == null)
            {
                NotificationHelper.ShowToast($"{actionLabel} Failed", "Couldn't reach the Link All service.");
                return;
            }
            if (response.RootElement.TryGetProperty("status", out var status) && status.GetString() == "error")
            {
                var message = response.RootElement.TryGetProperty("message", out var m) ? m.GetString() : null;
                NotificationHelper.ShowToast($"{actionLabel} Failed", string.IsNullOrEmpty(message) ? "The request was rejected." : message);
            }
        }

        // For fire-and-forget sends (drag-and-drop): runs on a background
        // thread, catches exceptions that would otherwise only reach
        // TaskScheduler.UnobservedTaskException (silently, per App.xaml.cs),
        // and reports failure the same way as ReportIfFailed.
        public static void RunFireAndForget(string actionLabel, Func<JsonDocument?> action)
        {
            Task.Run(() =>
            {
                try { ReportIfFailed(actionLabel, action()); }
                catch (Exception ex)
                {
                    App.HandleError(ex);
                    NotificationHelper.ShowToast($"{actionLabel} Failed", "An unexpected error occurred.");
                }
            });
        }
    }
}
