using System;
using System.Linq;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.UI.Xaml.Media.Imaging;
using QRCoder;
using Windows.Storage.Streams;

namespace Deskdrop.WinUI
{
    // The pairing sheet. Mirrors QRPairingWindow's token/QR logic (which is
    // still used by the tray popup and onboarding, where no XamlRoot exists
    // to host a dialog) but presents it as an in-app task with live status
    // instead of a standalone window.
    public sealed partial class PairDeviceDialog : ContentDialog
    {
        public DeskdropStore mgr => DeskdropStore.Shared;

        // Segoe Fluent glyphs built from code points, so this file stays
        // pure ASCII and survives any tooling that re-encodes it.
        private const int GlyphWarning = 0xE7BA;
        private const int GlyphCheckMark = 0xE73E;
        private const int GlyphError = 0xE783;

        private static string Glyph(int codePoint) => char.ConvertFromUtf32(codePoint);

        // x:Bind helpers for the six-tile security code. Separators such as
        // spaces or dashes are ignored; a missing digit renders empty.
        public static string PinDigit(string? pin, int index)
        {
            if (string.IsNullOrEmpty(pin)) return "";
            var digits = new string(pin.Where(char.IsLetterOrDigit).ToArray());
            return index >= 0 && index < digits.Length ? digits[index].ToString() : "";
        }

        public static string MatchHint(string? deviceName)
        {
            var name = string.IsNullOrWhiteSpace(deviceName) ? "the other device" : deviceName;
            return $"Only accept if this matches the code on {name}.";
        }

        public PairDeviceDialog()
        {
            this.InitializeComponent();
            Title = Services.AppDialog.Header("\uE8D7", "Pair a new device", "Scan this code from Link All on your phone");

            // Track pairing progress while the sheet is open so the status
            // line reflects reality rather than a fixed "waiting" message.
            mgr.PropertyChanged += OnStoreChanged;
            mgr.PairingRequests.CollectionChanged += OnPairingRequestsChanged;

            this.Closed += (_, _) =>
            {
                try
                {
                    mgr.PropertyChanged -= OnStoreChanged;
                    mgr.PairingRequests.CollectionChanged -= OnPairingRequestsChanged;
                }
                catch (Exception ex) { App.HandleError(ex); }
            };

            this.Loaded += (_, _) =>
            {
                UpdateStatus();
                _ = PrepareAndGenerateQrAsync();
            };
        }

        private void OnStoreChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs e)
        {
            if (e.PropertyName is nameof(DeskdropStore.ConnectedCount)
                or nameof(DeskdropStore.KnownDeviceCount)
                or nameof(DeskdropStore.HasPairingRequests))
            {
                DispatcherQueue?.TryEnqueue(UpdateStatus);
            }
        }

        private void OnPairingRequestsChanged(object? sender, System.Collections.Specialized.NotifyCollectionChangedEventArgs e)
        {
            DispatcherQueue?.TryEnqueue(UpdateStatus);
        }

        // Three states, one line: waiting, a device is asking, or done.
        private void UpdateStatus()
        {
            try
            {
                if (mgr.PairingRequests.Count > 0)
                {
                    WaitingRing.IsActive = false;
                    WaitingRing.Visibility = Visibility.Collapsed;
                    StatusGlyph.Visibility = Visibility.Visible;
                    StatusGlyph.Glyph = Glyph(GlyphWarning); // needs a decision
                    StatusGlyph.Style = (Style)Resources["StatusGlyphWarningStyle"];
                    StatusTitle.Text = "Confirm the security code";
                    StatusDetail.Text = "A device scanned the code and is waiting for you.";
                    return;
                }

                if (mgr.ConnectedCount > 0)
                {
                    WaitingRing.IsActive = false;
                    WaitingRing.Visibility = Visibility.Collapsed;
                    StatusGlyph.Visibility = Visibility.Visible;
                    StatusGlyph.Glyph = Glyph(GlyphCheckMark);
                    StatusGlyph.Style = (Style)Resources["StatusGlyphSuccessStyle"];
                    var noun = mgr.ConnectedCount == 1 ? "device" : "devices";
                    StatusTitle.Text = $"{mgr.ConnectedCount} {noun} connected";
                    StatusDetail.Text = "Scan again to add another, or close this window.";
                    return;
                }

                WaitingRing.Visibility = Visibility.Visible;
                WaitingRing.IsActive = true;
                StatusGlyph.Visibility = Visibility.Collapsed;
                StatusTitle.Text = "Waiting for a device to scan";
                StatusDetail.Text = "This code stays valid while the window is open.";
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private void OnAcceptPairingClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is PeerViewModel peer)
            {
                mgr.RespondToPairing(peer.device_id, true);
            }
        }

        private void OnDeclinePairingClicked(object sender, RoutedEventArgs e)
        {
            if ((sender as FrameworkElement)?.DataContext is PeerViewModel peer)
            {
                mgr.RespondToPairing(peer.device_id, false);
            }
        }

        private void OnRetryClicked(object sender, RoutedEventArgs e)
        {
            _ = PrepareAndGenerateQrAsync();
        }

        // The daemon's IPC pipe can take a moment to come up after launch
        // (App.OnLaunched starts the native engine on a background task), and
        // each Status()/GenerateQrToken() call only waits about a second - so
        // a single attempt right after opening is flaky by design. Retry with
        // backoff before surfacing a real error rather than a blank QR box.
        private async Task PrepareAndGenerateQrAsync()
        {
            QrLoadingRing.Visibility = Visibility.Visible;
            QrLoadingRing.IsActive = true;
            QrCodeImage.Visibility = Visibility.Collapsed;
            QrErrorPanel.Visibility = Visibility.Collapsed;
            RetryButton.Visibility = Visibility.Collapsed;

            string? fingerprint = null;
            string? token = null;

            // DaemonClient.Status()/GenerateQrToken() use a 1s per-call
            // timeout, tuned for frequent background polling. Pairing is a
            // deliberate, one-off action right after the user opened this
            // sheet, often within a second or two of the app itself
            // launching - the native engine can report "started" in the
            // trace log before its named pipe is actually accepting
            // connections. Calling Send() directly with a longer timeout
            // gives it real room to come up instead of racing it.
            const int attempts = 6;
            const int perCallTimeoutMs = 2500;

            for (var attempt = 1; attempt <= attempts && (fingerprint == null || token == null); attempt++)
            {
                await Task.Run(() =>
                {
                    try
                    {
                        // Every daemon response is wrapped as
                        // {"status":"ok","data":{...}} (an internally-tagged
                        // Rust enum) - reading local_fingerprint/token
                        // straight off the root element throws every time
                        // (root only has "status"/"data" keys), which this
                        // loop's outer try/catch swallowed silently. That
                        // made every pairing attempt fail identically
                        // regardless of retries or timeout, and looked
                        // exactly like the daemon being unreachable even
                        // while it was answering fine.
                        var status = DaemonClient.Send(DaemonClient.Req("status"), perCallTimeoutMs);
                        if (status != null && status.RootElement.TryGetProperty("data", out var statusData))
                        {
                            fingerprint ??= statusData.TryGetProperty("local_fingerprint", out var fp) ? fp.GetString() : null;
                        }

                        var tokenDoc = DaemonClient.Send(DaemonClient.Req("generate_qr_token"), perCallTimeoutMs);
                        if (tokenDoc != null && tokenDoc.RootElement.TryGetProperty("data", out var tokenData))
                        {
                            token ??= tokenData.TryGetProperty("token", out var tok) ? tok.GetString() : null;
                        }
                    }
                    catch (Exception ex)
                    {
                        System.Diagnostics.Debug.WriteLine($"Error preparing QR data (attempt {attempt}): {ex.Message}");
                    }
                });

                if ((fingerprint == null || token == null) && attempt < attempts)
                {
                    await Task.Delay(attempt * 400);
                }
            }

            if (fingerprint == null || token == null)
            {
                ShowQrError("Link All's local service isn't responding yet.");
                return;
            }

            try
            {
                var name = System.Net.WebUtility.UrlEncode(Environment.MachineName);
                var ip = System.Net.Dns.GetHostEntry(System.Net.Dns.GetHostName())
                    .AddressList
                    .FirstOrDefault(x => x.AddressFamily == System.Net.Sockets.AddressFamily.InterNetwork)
                    ?.ToString();

                var uri = $"deskdrop://pair?id={fingerprint}&token={token}&name={name}";
                if (!string.IsNullOrEmpty(ip)) uri += $"&ip={ip}&port=47823";

                await GenerateQrCodeAsync(uri);

                QrLoadingRing.IsActive = false;
                QrLoadingRing.Visibility = Visibility.Collapsed;
                QrCodeImage.Visibility = Visibility.Visible;
            }
            catch (Exception ex)
            {
                App.HandleError(ex);
                ShowQrError("Couldn't generate the QR code.");
            }
        }

        private void ShowQrError(string detail)
        {
            QrLoadingRing.IsActive = false;
            QrLoadingRing.Visibility = Visibility.Collapsed;
            QrErrorPanel.Visibility = Visibility.Visible;
            RetryButton.Visibility = Visibility.Visible;

            WaitingRing.IsActive = false;
            WaitingRing.Visibility = Visibility.Collapsed;
            StatusGlyph.Visibility = Visibility.Visible;
            StatusGlyph.Glyph = Glyph(GlyphError);
            StatusGlyph.Style = (Style)Resources["StatusGlyphErrorStyle"];
            StatusTitle.Text = "Pairing unavailable";
            StatusDetail.Text = detail;
        }

        private async Task GenerateQrCodeAsync(string payload)
        {
            using var generator = new QRCodeGenerator();
            using var data = generator.CreateQrCode(payload, QRCodeGenerator.ECCLevel.Q);
            using var qrCode = new PngByteQRCode(data);

            var bytes = qrCode.GetGraphic(20);

            using var stream = new InMemoryRandomAccessStream();
            using var writer = new DataWriter(stream.GetOutputStreamAt(0));
            writer.WriteBytes(bytes);
            await writer.StoreAsync();

            var bitmap = new BitmapImage();
            await bitmap.SetSourceAsync(stream);
            QrCodeImage.Source = bitmap;
        }
    }
}
