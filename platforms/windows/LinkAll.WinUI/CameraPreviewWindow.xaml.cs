using System;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Media.Imaging;
using Windows.Storage.Streams;

namespace LinkAll.WinUI
{
    public sealed partial class CameraPreviewWindow : Window
    {
        // Raised by ClipboardManager's event loop on PB_EVENT_CAMERA_STREAM_STOP
        // so an open preview reflects the peer ending the stream instead of
        // just silently going stale.
        public static event Action<string>? RemoteStreamStopped;
        public static void NotifyRemoteStreamStopped(string deviceId) => RemoteStreamStopped?.Invoke(deviceId);

        private readonly string _deviceId;
        private readonly Microsoft.UI.Dispatching.DispatcherQueue _dispatcherQueue;
        private CancellationTokenSource? _cts;
        private bool _receivedFirstFrame;
        private int _consecutiveMisses;

        public CameraPreviewWindow(string deviceId)
        {
            _deviceId = deviceId;
            this.InitializeComponent();
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
            var appWindow = Microsoft.UI.Windowing.AppWindow.GetFromWindowId(windowId);
            LinkAll.WinUI.Services.WindowIconHelper.Apply(appWindow);

            // Set window size (DIPs, not physical pixels - see WindowIconHelper.ResizeDips)
            LinkAll.WinUI.Services.WindowIconHelper.ResizeDips(appWindow, hwnd, 640, 480);

            _dispatcherQueue = Microsoft.UI.Dispatching.DispatcherQueue.GetForCurrentThread();

            RemoteStreamStopped += OnRemoteStreamStopped;
            this.Closed += (s, e) =>
            {
                RemoteStreamStopped -= OnRemoteStreamStopped;
                StopPolling();
                // Off the UI thread: the native call blocks on the core runtime.
                var handle = App.EngineHandle;
                if (handle != IntPtr.Zero)
                {
                    _ = Task.Run(() =>
                    {
                        try { NativeCore.linkall_stop_camera_stream(handle); }
                        catch (Exception ex) { App.HandleError(ex); }
                    });
                }
            };

            // This window used to just poll for a frame that might already
            // exist - nothing ever actually asked the peer to start
            // streaming, so it showed "Waiting for stream..." forever
            // unless the phone happened to start unprompted.
            var engineHandle = App.EngineHandle;
            var requestedDevice = _deviceId;
            if (engineHandle != IntPtr.Zero)
            {
                _ = Task.Run(() =>
                {
                    try { NativeCore.linkall_request_camera_stream(engineHandle, requestedDevice); }
                    catch (Exception ex) { App.HandleError(ex); }
                });
            }

            StartPolling();
        }

        private void StartPolling()
        {
            _cts = new CancellationTokenSource();
            var token = _cts.Token;

            _ = Task.Run(async () =>
            {
                while (!token.IsCancellationRequested)
                {
                    try
                    {
                        // Off the UI thread - LatestCameraFrame is a synchronous
                        // named-pipe round trip, same as every other DaemonClient
                        // call, but we're already inside the background loop.
                        var resp = DaemonClient.LatestCameraFrame(_deviceId);
                        byte[]? frameBytes = null;

                        if (resp != null
                            && resp.RootElement.TryGetProperty("data", out var dataElem)
                            && dataElem.TryGetProperty("frame_base64", out var frameElem))
                        {
                            var base64 = frameElem.GetString();
                            if (!string.IsNullOrEmpty(base64))
                            {
                                try { frameBytes = Convert.FromBase64String(base64); }
                                catch { frameBytes = null; }
                            }
                        }

                        if (frameBytes != null)
                        {
                            _consecutiveMisses = 0;
                            var bytes = frameBytes;
                            _dispatcherQueue.TryEnqueue(async () => await RenderFrameAsync(bytes));
                        }
                        else
                        {
                            _consecutiveMisses++;
                            if (_consecutiveMisses >= 5)
                            {
                                _dispatcherQueue.TryEnqueue(() =>
                                {
                                    StatusText.Text = _receivedFirstFrame ? "Connection lost..." : "Waiting for stream...";
                                    StatusPanel.Visibility = Visibility.Visible;
                                });
                            }
                        }
                    }
                    catch (Exception ex)
                    {
                        App.HandleError(ex);
                    }

                    try
                    {
                        await Task.Delay(700, token);
                    }
                    catch (TaskCanceledException)
                    {
                        break;
                    }
                }
            }, token);
        }

        private async Task RenderFrameAsync(byte[] jpegBytes)
        {
            try
            {
                using var stream = new InMemoryRandomAccessStream();
                using var writer = new DataWriter(stream.GetOutputStreamAt(0));
                writer.WriteBytes(jpegBytes);
                await writer.StoreAsync();

                var bitmapImage = new BitmapImage();
                await bitmapImage.SetSourceAsync(stream);

                CameraImage.Source = bitmapImage;
                _receivedFirstFrame = true;
                StatusPanel.Visibility = Visibility.Collapsed;
            }
            catch (Exception ex)
            {
                App.HandleError(ex);
            }
        }

        private void OnRemoteStreamStopped(string deviceId)
        {
            if (!string.Equals(deviceId, _deviceId, StringComparison.OrdinalIgnoreCase)) return;
            _dispatcherQueue.TryEnqueue(() =>
            {
                StopPolling();
                StatusText.Text = "Stream ended";
                StatusPanel.Visibility = Visibility.Visible;
            });
        }

        private void StopPolling()
        {
            try { _cts?.Cancel(); } catch { }
            _cts?.Dispose();
            _cts = null;
        }

        private void BtnClose_Click(object sender, RoutedEventArgs e)
        {
            this.Close();
        }
    }
}
