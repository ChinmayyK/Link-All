using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;

namespace LinkAll.WinUI.Services
{
    // Several "quick action" paths (push clipboard, quick send, title-bar
    // send) used to resolve their target via ConnectedPeers.FirstOrDefault()
    // - invisible with one connected device, but silently acts on an
    // arbitrary one once a second is connected, with no indication which.
    // This prompts instead whenever there's real ambiguity.
    public static class DevicePicker
    {
        public static async Task<PeerViewModel?> PickAsync(XamlRoot? xamlRoot, IEnumerable<PeerViewModel> connectedPeers)
        {
            var peers = connectedPeers.ToList();
            if (peers.Count == 0) return null;
            if (peers.Count == 1) return peers[0];
            if (xamlRoot == null) return peers[0];

            var choice = await ShowAsync(xamlRoot, peers, offerAll: false);
            return choice?.Peer;
        }

        // File sends can also go to every connected device at once - a null
        // target in send_file_path, same as Android and macOS. Returns null
        // when the user cancels; otherwise the chosen target, where a null
        // DeviceId means all connected devices.
        public static async Task<SendTarget?> PickSendTargetAsync(XamlRoot? xamlRoot, IEnumerable<PeerViewModel> connectedPeers)
        {
            var peers = connectedPeers.ToList();
            if (peers.Count == 0) return new SendTarget(null);
            if (peers.Count == 1 || xamlRoot == null) return new SendTarget(peers[0].device_id);

            var choice = await ShowAsync(xamlRoot, peers, offerAll: true);
            if (choice == null) return null;
            return new SendTarget(choice.Value.All ? null : choice.Value.Peer?.device_id);
        }

        // Each device is one large choice, like the send sheet on Android and
        // the Mac's modal: its logo glyph, name and platform; "All devices"
        // first when sending to every device is allowed.
        private static async Task<(PeerViewModel? Peer, bool All)?> ShowAsync(XamlRoot xamlRoot, List<PeerViewModel> peers, bool offerAll)
        {
            (PeerViewModel? Peer, bool All)? result = null;
            var dialog = AppDialog.Create(xamlRoot, "\uE8EA",
                offerAll ? "Send to which device?" : "Which device?",
                offerAll ? "All your devices, or just one" : null);
            dialog.CloseButtonText = "Cancel";

            var list = new StackPanel { Spacing = 10 };
            if (offerAll)
            {
                list.Children.Add(AppDialog.Option("\uE8F1", "All devices", $"{peers.Count} connected", () =>
                {
                    result = (null, true);
                    dialog.Hide();
                }));
            }
            foreach (var peer in peers)
            {
                var p = peer;
                list.Children.Add(AppDialog.Option(p.DeviceGlyph, p.DisplayName, p.PlatformLabel, () =>
                {
                    result = (p, false);
                    dialog.Hide();
                }));
            }
            dialog.Content = new ScrollViewer { Content = list, MaxHeight = 420 };
            await dialog.ShowAsync();
            return result;
        }
    }

    public sealed record SendTarget(string? DeviceId);
}
