using System;
using System.Runtime.InteropServices.WindowsRuntime;
using System.Text.Json.Nodes;
using System.Threading;
using System.Threading.Tasks;
using Windows.Media.Control;
using Windows.Storage.Streams;

namespace LinkAll.WinUI.Services
{
    /// <summary>
    /// Reads what this PC is playing (Spotify, a browser, any app that uses the system
    /// media controls) and tells the engine, which tells the paired devices. The cover
    /// is sent once per track.
    /// </summary>
    public sealed class NowPlayingSource : IDisposable
    {
        private readonly CancellationTokenSource _stop = new();
        private string _lastKey = "";

        public void Start() => _ = Task.Run(LoopAsync);

        public void Dispose() => _stop.Cancel();

        private async Task LoopAsync()
        {
            GlobalSystemMediaTransportControlsSessionManager? manager = null;
            while (!_stop.IsCancellationRequested)
            {
                try
                {
                    manager ??= await GlobalSystemMediaTransportControlsSessionManager.RequestAsync();
                    await TellOnceAsync(manager);
                }
                catch (Exception ex)
                {
                    TraceLog.Write("NowPlayingSource: " + ex.Message);
                    manager = null;
                }
                try { await Task.Delay(TimeSpan.FromSeconds(3), _stop.Token); } catch (TaskCanceledException) { }
            }
        }

        private async Task TellOnceAsync(GlobalSystemMediaTransportControlsSessionManager manager)
        {
            var session = manager.GetCurrentSession();
            if (session == null)
            {
                Send("", "", "", "", false, 0, 0, null);
                return;
            }
            var props = await session.TryGetMediaPropertiesAsync();
            var info = session.GetPlaybackInfo();
            var timeline = session.GetTimelineProperties();
            var playing = info.PlaybackStatus == GlobalSystemMediaTransportControlsSessionPlaybackStatus.Playing;
            var key = $"{props.Title}|{props.Artist}|{props.AlbumTitle}";
            string? art = null;
            if (key != _lastKey)
            {
                _lastKey = key;
                art = await CoverAsync(props.Thumbnail);
            }
            var app = session.SourceAppUserModelId ?? "";
            // "Spotify.exe" or "Microsoft.ZuneMusic_8wekyb3d8bbwe!Microsoft.ZuneMusic" -> a short name.
            app = app.Split('!')[^1].Split('.')[0];
            Send(props.Title ?? "", props.Artist ?? "", props.AlbumTitle ?? "", app, playing,
                 (long)timeline.Position.TotalMilliseconds, (long)(timeline.EndTime - timeline.StartTime).TotalMilliseconds, art);
        }

        /// <summary>The cover scaled to at most 300 pixels and encoded as a JPEG, as base64.</summary>
        private static async Task<string?> CoverAsync(IRandomAccessStreamReference? thumbnail)
        {
            if (thumbnail == null) return null;
            try
            {
                using var source = await thumbnail.OpenReadAsync();
                var decoder = await Windows.Graphics.Imaging.BitmapDecoder.CreateAsync(source);
                var scale = Math.Min(1.0, 300.0 / Math.Max(decoder.PixelWidth, decoder.PixelHeight));
                var transform = new Windows.Graphics.Imaging.BitmapTransform
                {
                    ScaledWidth = Math.Max(1, (uint)(decoder.PixelWidth * scale)),
                    ScaledHeight = Math.Max(1, (uint)(decoder.PixelHeight * scale)),
                };
                var pixels = await decoder.GetPixelDataAsync(
                    Windows.Graphics.Imaging.BitmapPixelFormat.Bgra8,
                    Windows.Graphics.Imaging.BitmapAlphaMode.Ignore, transform,
                    Windows.Graphics.Imaging.ExifOrientationMode.IgnoreExifOrientation,
                    Windows.Graphics.Imaging.ColorManagementMode.DoNotColorManage);
                using var output = new InMemoryRandomAccessStream();
                var encoder = await Windows.Graphics.Imaging.BitmapEncoder.CreateAsync(Windows.Graphics.Imaging.BitmapEncoder.JpegEncoderId, output);
                encoder.SetPixelData(Windows.Graphics.Imaging.BitmapPixelFormat.Bgra8, Windows.Graphics.Imaging.BitmapAlphaMode.Ignore,
                    transform.ScaledWidth, transform.ScaledHeight, 96, 96, pixels.DetachPixelData());
                await encoder.FlushAsync();
                var bytes = new byte[output.Size];
                output.Seek(0);
                await output.ReadAsync(bytes.AsBuffer(), (uint)output.Size, InputStreamOptions.None);
                return bytes.Length <= 120 * 1024 ? Convert.ToBase64String(bytes) : null;
            }
            catch { return null; }
        }

        private static void Send(string title, string artist, string album, string app, bool playing, long positionMs, long durationMs, string? art)
        {
            var request = DaemonClient.Req("set_now_playing",
                ("title", title), ("artist", artist), ("album", album), ("app", app),
                ("playing", playing), ("position_ms", Math.Max(0, positionMs)), ("duration_ms", Math.Max(0, durationMs)));
            if (art != null) request["art_base64"] = art;
            DaemonClient.Send(request)?.Dispose();
        }
    }
}
