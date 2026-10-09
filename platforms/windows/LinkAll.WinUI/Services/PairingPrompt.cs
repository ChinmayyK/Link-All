using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Text;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media;

namespace LinkAll.WinUI.Services
{
    // A pairing request used to surface only as a card on the Devices page
    // and a plain toast with no actions - so anyone on another page, or with
    // the window in the tray, never saw it, and the other device just sat on
    // "waiting". Mac and Android put the request in front of the user with
    // the security code and an explicit Accept / Decline; this is the same
    // moment for Windows. The Devices-page card stays as the fallback when
    // the dialog can't be shown (another dialog is already open).
    public static class PairingPrompt
    {
        private static readonly HashSet<string> _prompted = new();
        private static bool _isOpen;

        // Called on the UI thread each time the store re-syncs its
        // PairingRequests projection.
        public static void Sync(IReadOnlyCollection<PeerViewModel> requests)
        {
            // Forget requests that were answered or withdrawn, so the same
            // device asking again later is prompted again.
            _prompted.IntersectWith(requests.Select(p => p.device_id));
            if (_isOpen) return;

            var next = requests.FirstOrDefault(p => !_prompted.Contains(p.device_id));
            if (next == null) return;

            var root = DashboardWindow.Current?.Content?.XamlRoot;
            if (root == null) return; // tray-only: the actionable toast covers it

            _prompted.Add(next.device_id);
            _ = ShowAsync(next, root);
        }

        private static async Task ShowAsync(PeerViewModel peer, XamlRoot root)
        {
            _isOpen = true;
            try
            {
                var dialog = new ContentDialog
                {
                    Title = AppDialog.Header("\uE8D7", $"{peer.DisplayName} wants to pair", "Accept only if the code matches"),
                    Content = BuildContent(peer),
                    PrimaryButtonText = "Accept",
                    SecondaryButtonText = "Decline",
                    CloseButtonText = "Later",
                    DefaultButton = ContentDialogButton.Primary,
                    XamlRoot = root,
                };

                // The request can end while this is up (expired, withdrawn,
                // connection lost); a stale Accept would be refused anyway.
                void OnPeerChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs e) =>
                    dialog.DispatcherQueue.TryEnqueue(() => { if (!peer.pairingRequested) dialog.Hide(); });
                peer.PropertyChanged += OnPeerChanged;
                ContentDialogResult result;
                try { result = await dialog.ShowAsync(); }
                finally { peer.PropertyChanged -= OnPeerChanged; }
                if (!peer.pairingRequested) return;
                if (result == ContentDialogResult.Primary)
                    LinkAllStore.Shared.RespondToPairing(peer.device_id, true);
                else if (result == ContentDialogResult.Secondary)
                    LinkAllStore.Shared.RespondToPairing(peer.device_id, false);
            }
            catch (Exception ex)
            {
                // Most often "only one ContentDialog can be open at a time".
                // Let it be prompted again on the next sync.
                _prompted.Remove(peer.device_id);
                TraceLog.Write($"PairingPrompt: could not show dialog - {ex.Message}");
            }
            finally
            {
                _isOpen = false;
            }
        }

        // Our own request. Pair used to only change one line of small text
        // in the device row, so the code to compare - which the other device
        // shows full screen - was easy to miss or not there at all. This
        // sheet stays up for the whole request: the code, a countdown, how it
        // ended, and Cancel / Try again. If the other device asks us at the
        // same time it becomes the Accept prompt, since one Accept pairs both.
        public static void ShowOutgoing(PeerViewModel peer, XamlRoot? root)
        {
            if (root == null || _isOpen) return; // the device row still shows the code
            _ = ShowOutgoingAsync(peer, root);
        }

        private static async Task ShowOutgoingAsync(PeerViewModel peer, XamlRoot root)
        {
            _isOpen = true;
            var status = new TextBlock { TextWrapping = TextWrapping.Wrap, Opacity = 0.8 };
            var dialog = new ContentDialog
            {
                Title = AppDialog.Header("\uE8D7", $"Pair with {peer.DisplayName}", "Waiting for the other device"),
                Content = BuildCodePanel(peer, $"Accept the request on {peer.DisplayName} if it shows this code.", status),
                CloseButtonText = "Cancel request",
                DefaultButton = ContentDialogButton.Primary,
                XamlRoot = root,
            };

            DateTime? deadline = null;
            ulong? lastExpires = null;
            // The sheet opens before the poll that shows the request; until
            // then an old outcome on the peer must not read as this one's.
            var seenPending = false;
            DispatcherQueueTimer? closeTimer = null;

            void Render()
            {
                seenPending |= peer.outgoingPairingWaiting || peer.pairingRequested;
                if (peer.pairingExpiresInSecs != lastExpires)
                {
                    lastExpires = peer.pairingExpiresInSecs;
                    deadline = lastExpires is { } secs ? DateTime.UtcNow.AddSeconds(secs) : null;
                }

                if (peer.is_trusted)
                {
                    status.Text = $"Paired with {peer.DisplayName}.";
                    dialog.PrimaryButtonText = "";
                    dialog.CloseButtonText = "Done";
                    if (closeTimer == null)
                    {
                        closeTimer = dialog.DispatcherQueue.CreateTimer();
                        closeTimer.Interval = TimeSpan.FromSeconds(1.2);
                        closeTimer.IsRepeating = false;
                        closeTimer.Tick += (_, _) => dialog.Hide();
                        closeTimer.Start();
                    }
                }
                else if (peer.pairingRequested)
                {
                    status.Text = $"{peer.DisplayName} asked to pair too. Accept if the codes match.";
                    dialog.PrimaryButtonText = "Accept";
                    dialog.CloseButtonText = "Decline";
                }
                else if (peer.outgoingPairingWaiting || !seenPending)
                {
                    var left = deadline is { } d ? Math.Max(0, (int)Math.Ceiling((d - DateTime.UtcNow).TotalSeconds)) : (int?)null;
                    status.Text = string.IsNullOrWhiteSpace(peer.pairingPin)
                        ? $"Connecting to {peer.DisplayName}..."
                        : left is { } l ? $"Waiting for {peer.DisplayName} to accept - {l}s left" : $"Waiting for {peer.DisplayName} to accept";
                    dialog.PrimaryButtonText = "";
                    dialog.CloseButtonText = "Cancel request";
                }
                else
                {
                    status.Text = peer.pairingOutcome switch
                    {
                        "declined" => $"{peer.DisplayName} declined.",
                        "expired" => $"No answer from {peer.DisplayName}. Check Link All is open there.",
                        "cancelled" => $"{peer.DisplayName} withdrew its request.",
                        "update_needed" => $"{peer.DisplayName} runs an older Link All that turns pairing requests down on its own. Update it, then try again.",
                        _ => "Request closed.",
                    };
                    dialog.PrimaryButtonText = "Try again";
                    dialog.CloseButtonText = "Close";
                }
            }

            void OnPeerChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs e) =>
                dialog.DispatcherQueue.TryEnqueue(Render);

            var tick = dialog.DispatcherQueue.CreateTimer();
            tick.Interval = TimeSpan.FromSeconds(1);
            tick.Tick += (_, _) => Render();

            dialog.PrimaryButtonClick += (_, args) =>
            {
                if (peer.pairingRequested)
                {
                    LinkAllStore.Shared.RespondToPairing(peer.device_id, true);
                    args.Cancel = true; // stay up to show "Paired"
                }
                else
                {
                    seenPending = false;
                    LinkAllStore.Shared.ConnectAndPair(peer.device_id);
                    args.Cancel = true;
                }
            };

            peer.PropertyChanged += OnPeerChanged;
            try
            {
                Render();
                tick.Start();
                await dialog.ShowAsync();
                // Closed while still pending: Cancel / Decline / Esc all mean no.
                if (peer.pairingRequested && !peer.is_trusted)
                    LinkAllStore.Shared.RespondToPairing(peer.device_id, false);
                else if ((peer.outgoingPairingWaiting || !seenPending) && !peer.is_trusted)
                    LinkAllStore.Shared.CancelPairing(peer.device_id);
            }
            catch (Exception ex)
            {
                // Most often "only one ContentDialog can be open at a time";
                // the device row keeps showing the code.
                TraceLog.Write($"PairingPrompt: could not show outgoing sheet - {ex.Message}");
            }
            finally
            {
                tick.Stop();
                closeTimer?.Stop();
                peer.PropertyChanged -= OnPeerChanged;
                _prompted.Add(peer.device_id); // answered here; don't re-prompt
                _isOpen = false;
            }
        }

        private static UIElement BuildContent(PeerViewModel peer) =>
            BuildCodePanel(
                peer,
                $"Check that this code matches the one shown on {peer.DisplayName}. Only accept if it does.",
                new TextBlock
                {
                    Text = "Once paired, the two devices reconnect automatically and share clipboard and files.",
                    TextWrapping = TextWrapping.Wrap,
                    Opacity = 0.7,
                    FontSize = 12,
                });

        private static UIElement BuildCodePanel(PeerViewModel peer, string instruction, TextBlock footer)
        {
            var pin = new TextBlock
            {
                FontFamily = new FontFamily("Cascadia Mono, Consolas"),
                FontSize = 30,
                FontWeight = FontWeights.SemiBold,
                CharacterSpacing = 160,
                HorizontalAlignment = HorizontalAlignment.Center,
            };
            // Bound rather than copied: the code can land a poll after the
            // request itself, and a blank code must fill in, not stay blank.
            pin.SetBinding(TextBlock.TextProperty, new Microsoft.UI.Xaml.Data.Binding
            {
                Source = peer,
                Path = new PropertyPath(nameof(PeerViewModel.pairingPin)),
                FallbackValue = "--- ---",
                TargetNullValue = "--- ---",
            });

            return new StackPanel
            {
                Spacing = 14,
                MinWidth = 320,
                Children =
                {
                    new TextBlock
                    {
                        Text = instruction,
                        TextWrapping = TextWrapping.Wrap,
                    },
                    new Border
                    {
                        Padding = new Thickness(16, 12, 16, 12),
                        CornerRadius = new CornerRadius(10),
                        Background = Application.Current.Resources.TryGetValue("AppSurfaceSubtleBrush", out var bg) && bg is Brush b
                            ? b
                            : new SolidColorBrush(Microsoft.UI.Colors.Transparent),
                        Child = pin,
                    },
                    footer,
                },
            };
        }
    }
}
