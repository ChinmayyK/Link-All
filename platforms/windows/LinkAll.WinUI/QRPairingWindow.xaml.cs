using System;
using System.IO;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media;
using Microsoft.UI.Xaml.Media.Imaging;
using Microsoft.UI.Windowing;
using Microsoft.UI.Composition.SystemBackdrops;
using QRCoder;
using Windows.Storage.Streams;

namespace LinkAll.WinUI
{
    public sealed partial class QRPairingWindow : Window
    {
        public QRPairingWindow()
        {
            this.InitializeComponent();

            if (MicaController.IsSupported())
            {
                this.SystemBackdrop = new MicaBackdrop();
            }
            else if (DesktopAcrylicController.IsSupported())
            {
                this.SystemBackdrop = new DesktopAcrylicBackdrop();
            }

            ExtendsContentIntoTitleBar = true;
            SetTitleBar(AppTitleBar);
            LinkAll.WinUI.Services.ThemeService.Register(this);

            TxtDeviceName.Text = System.Environment.MachineName;

            // Resize the window
            var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(this);
            var windowId = Microsoft.UI.Win32Interop.GetWindowIdFromWindow(hwnd);
            var appWindow = Microsoft.UI.Windowing.AppWindow.GetFromWindowId(windowId);
            LinkAll.WinUI.Services.WindowIconHelper.Apply(appWindow);
            LinkAll.WinUI.Services.WindowIconHelper.ResizeDips(appWindow, hwnd, 320, 480);

            _ = PrepareAndGenerateQrAsync();
        }

        private void OnRetryClicked(object sender, RoutedEventArgs e)
        {
            _ = PrepareAndGenerateQrAsync();
        }

        // The daemon's IPC pipe can take a moment to come up after launch
        // (see App.xaml.cs OnLaunched - the native engine starts on a
        // background Task.Run), and each Status()/GenerateQrToken() call
        // only waits ~1s - so a single attempt right after opening this
        // window is flaky by design. Retry a few times with backoff before
        // surfacing a real error state instead of leaving a blank QR box.
        private async Task PrepareAndGenerateQrAsync()
        {
            QrLoadingRing.Visibility = Visibility.Visible;
            QrLoadingRing.IsActive = true;
            QrCodeImage.Visibility = Visibility.Collapsed;
            QrErrorPanel.Visibility = Visibility.Collapsed;
            RetryButton.Visibility = Visibility.Collapsed;
            TxtCaption.Text = "Scan with Link All on your other device to connect.";

            string? fp = null;
            string? token = null;

            for (var attempt = 1; attempt <= 5 && (fp == null || token == null); attempt++)
            {
                await Task.Run(() =>
                {
                    try
                    {
                        // Every daemon response is wrapped as
                        // {"status":"ok","data":{...}} (an internally-tagged
                        // Rust enum) - reading local_fingerprint/token
                        // straight off the root element threw every time
                        // (root only has "status"/"data" keys), silently
                        // caught below, so this failed identically on every
                        // attempt regardless of retries - it looked exactly
                        // like the daemon being unreachable even when it
                        // was answering fine.
                        var status = DaemonClient.Status();
                        if (status != null && status.RootElement.TryGetProperty("data", out var statusData))
                        {
                            fp ??= statusData.TryGetProperty("local_fingerprint", out var fpEl) ? fpEl.GetString() : null;
                        }

                        var tokenDoc = DaemonClient.GenerateQrToken();
                        if (tokenDoc != null && tokenDoc.RootElement.TryGetProperty("data", out var tokenData))
                        {
                            token ??= tokenData.TryGetProperty("token", out var tokEl) ? tokEl.GetString() : null;
                        }
                    }
                    catch (Exception ex)
                    {
                        System.Diagnostics.Debug.WriteLine($"Error preparing QR data (attempt {attempt}): {ex.Message}");
                    }
                });

                if ((fp == null || token == null) && attempt < 5)
                {
                    await Task.Delay(attempt * 400);
                }
            }

            if (fp == null || token == null)
            {
                QrLoadingRing.IsActive = false;
                QrLoadingRing.Visibility = Visibility.Collapsed;
                QrErrorPanel.Visibility = Visibility.Visible;
                RetryButton.Visibility = Visibility.Visible;
                TxtCaption.Text = "Link All's local service isn't responding yet.";
                return;
            }

            try
            {
                var name = System.Net.WebUtility.UrlEncode(System.Environment.MachineName);
                var ip = System.Net.Dns.GetHostEntry(System.Net.Dns.GetHostName()).AddressList.FirstOrDefault(x => x.AddressFamily == System.Net.Sockets.AddressFamily.InterNetwork)?.ToString();

                string uri = $"linkall://pair?id={fp}&token={token}&name={name}";
                if (!string.IsNullOrEmpty(ip)) uri += $"&ip={ip}&port=47823";
                await GenerateQRCodeAsync(uri);

                QrLoadingRing.IsActive = false;
                QrLoadingRing.Visibility = Visibility.Collapsed;
                QrCodeImage.Visibility = Visibility.Visible;
            }
            catch (Exception ex)
            {
                App.HandleError(ex);
                QrLoadingRing.IsActive = false;
                QrLoadingRing.Visibility = Visibility.Collapsed;
                QrErrorPanel.Visibility = Visibility.Visible;
                RetryButton.Visibility = Visibility.Visible;
                TxtCaption.Text = "Couldn't generate the QR code.";
            }
        }

        private async Task GenerateQRCodeAsync(string payload)
        {
            using var qrGenerator = new QRCodeGenerator();
            using var qrCodeData = qrGenerator.CreateQrCode(payload, QRCodeGenerator.ECCLevel.Q);
            using var qrCode = new PngByteQRCode(qrCodeData);

            byte[] qrCodeBytes = qrCode.GetGraphic(20);

            using var stream = new InMemoryRandomAccessStream();
            using var writer = new DataWriter(stream.GetOutputStreamAt(0));
            writer.WriteBytes(qrCodeBytes);
            await writer.StoreAsync();

            var bitmapImage = new BitmapImage();
            await bitmapImage.SetSourceAsync(stream);

            QrCodeImage.Source = bitmapImage;
        }
    }
}


