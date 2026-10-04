using System;
using System.Collections.Generic;
using System.Linq;

namespace LinkAll.WinUI.Services
{
    /// <summary>One line in the dashboard's search: what it is, and what choosing it does.</summary>
    public sealed record SearchHit(string Glyph, string Title, string Subtitle, Action Run);

    /// <summary>
    /// The title bar's search. It looks across what a person would hunt for:
    /// the pages, their devices, recent clipboard items and transfers, and
    /// each result does the obvious thing: open the page, browse the device,
    /// copy the item again.
    /// </summary>
    public static class DashboardSearch
    {
        private const int MaxHits = 10;

        public static List<SearchHit> Find(string? query, LinkAllStore store, Action<string> navigate)
        {
            var words = (query ?? "").Split(' ', StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries);
            bool Matches(params string?[] fields)
            {
                var hay = string.Join(' ', fields.Where(f => !string.IsNullOrEmpty(f)));
                return words.All(w => hay.Contains(w, StringComparison.OrdinalIgnoreCase));
            }

            var hits = new List<SearchHit>();

            foreach (var (tag, glyph, name, hint) in new[]
            {
                ("Devices", "", "Devices", "Your paired devices and what's nearby"),
                ("Clipboard", "", "Clipboard", "Everything you copied on any device"),
                ("DevicePeer", "", "Remote files", "Browse a device's files"),
                ("Transfers", "", "Transfers", "Files and folders moving now, and finished ones"),
                ("Activity", "", "Activity", "What crossed between your devices"),
                ("Settings", "", "Settings", "Startup, transfers, security"),
            })
            {
                if (Matches(name, hint)) hits.Add(new SearchHit(glyph, name, hint, () => navigate(tag)));
            }

            foreach (var peer in store.Peers.Where(p => p.IsKnown || p.IsNearby))
            {
                var state = peer.IsConnected ? "Connected" : "Not connected";
                if (!Matches(peer.DisplayName, peer.PlatformLabel, state)) continue;
                var p = peer;
                hits.Add(new SearchHit(
                    p.DeviceGlyph, p.DisplayName,
                    string.Join(" · ", new[] { p.PlatformLabel, state }.Where(s => !string.IsNullOrEmpty(s))),
                    () =>
                    {
                        if (p.IsConnected) { store.SelectedPeer = p; navigate("DevicePeer"); }
                        else navigate("Devices");
                    }));
            }

            foreach (var item in store.History.Where(h => Matches(h.Summary, h.FullText, h.Source)).Take(5))
            {
                var h = item;
                var isImage = ClipboardManager.IsCachedImage(h.path);
                hits.Add(new SearchHit(
                    isImage ? "" : "",
                    string.IsNullOrWhiteSpace(h.Summary) ? "Clipboard item" : h.Summary,
                    $"{(h.Source == "local" ? "This PC" : h.Source)} · {h.RelativeTime} · click to copy again",
                    () => CopyAgain(h)));
            }

            foreach (var t in store.ActiveTransfers.Where(t => Matches(t.FileName, t.PeerLabel)).Take(3))
            {
                hits.Add(new SearchHit(t.TransferIcon, t.FileName, $"{t.PeerLabel} · {t.StateLabel}", () => navigate("Transfers")));
            }

            return hits.Take(MaxHits).ToList();
        }

        private static void CopyAgain(HistoryItem item)
        {
            try
            {
                if (ClipboardManager.IsCachedImage(item.path))
                {
                    var path = item.path;
                    _ = App.Clipboard?.CopyImageAsync(path);
                    return;
                }
                var text = !string.IsNullOrEmpty(item.FullText) ? item.FullText : item.Summary;
                if (string.IsNullOrEmpty(text)) return;
                var package = new global::Windows.ApplicationModel.DataTransfer.DataPackage();
                package.SetText(text);
                global::Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(package);
                NotificationHelper.ShowToast("Copied", "Paste it anywhere");
            }
            catch (Exception ex) { App.HandleError(ex); }
        }
    }
}
