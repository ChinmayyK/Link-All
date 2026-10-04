using System;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using System.Collections.ObjectModel;
using System.Collections.Generic;
using Microsoft.UI.Dispatching;
using Windows.ApplicationModel.DataTransfer;
using LinkAll.WinUI.Views;
using System.Text.Json;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using Microsoft.UI.Xaml;
using Windows.Graphics.Imaging;
using Windows.Storage.Streams;

namespace LinkAll.WinUI.Services
{
    public class ClipboardManager : IDisposable
    {
        private readonly DispatcherQueue _dispatcher;
        private string _lastText = string.Empty;
        // SHA-256 of the last image sent or received, as PNG bytes, so the
        // same picture is never sent twice.
        private string _lastImageHash = string.Empty;
        // Setting a received image fires ContentChanged. Windows re-encodes
        // the picture, so the bytes differ from what arrived; any bitmap seen
        // this soon after is our own write, not a new copy.
        private long _imageAppliedAtMs = long.MinValue / 2;
        private const long ImageEchoWindowMs = 2000;
        private readonly DispatcherTimer _pollTimer;

        public ObservableCollection<HistoryItem> History { get; set; } = new ObservableCollection<HistoryItem>();

        public ClipboardManager()
        {
            _dispatcher = DispatcherQueue.GetForCurrentThread();
            Windows.ApplicationModel.DataTransfer.Clipboard.ContentChanged += Clipboard_ContentChanged;

            // 30ms (33Hz) was needlessly aggressive for a queue that's empty the
            // vast majority of ticks — a continuous UI-thread wakeup forever, for
            // the app's whole lifetime. Android's equivalent native-event-drain
            // loop already settled on 100ms as plenty responsive (feels instant
            // for clipboard/file-transfer delivery) while idling on a background
            // thread; match that cadence here too.
            _pollTimer = new DispatcherTimer();
            _pollTimer.Interval = TimeSpan.FromMilliseconds(100);
            _pollTimer.Tick += OnPollTick;
            _pollTimer.Start();
        }



        private async void Clipboard_ContentChanged(object? sender, object e)
        {
            await CheckClipboardAsync();
        }

        private void OnPollTick(object? sender, object e)
        {
            DrainEvents();
        }

        // The native core's send entry points block on its async runtime until
        // the message is queued to every peer, which can stall for seconds on
        // a slow or backed-up connection. Never make the UI thread wait on that.
        private static void RunNativeOffUiThread(Action action)
        {
            System.Threading.Tasks.Task.Run(() =>
            {
                try { action(); }
                catch (Exception ex) { App.HandleError(ex); }
            });
        }

        private void DrainEvents()
        {
            if (App.EngineHandle == IntPtr.Zero) return;
            bool processedAny = false;
            bool onlyProgress = true;
            while (true)
            {
                var ev = NativeCore.linkall_poll_event(App.EngineHandle);
                if (ev == IntPtr.Zero) break;
                processedAny = true;
                try
                {
                    int kind = NativeCore.linkall_event_type(ev);
                    if (kind != NativeCore.PB_EVENT_FILE_TRANSFER_PROGRESS) onlyProgress = false;
                    switch (kind)
                    {
                        case NativeCore.PB_EVENT_CLIPBOARD_TEXT:
                        {
                            var text = NativeCore.PtrToUtf8String(NativeCore.linkall_event_text(ev));
                            var from = NativeCore.PtrToUtf8String(NativeCore.linkall_event_device_name(ev)) ?? "Unknown";
                            if (text != null)
                            {
                                _lastText = text;
                                (_dispatcher ?? App.MainDispatcherQueue)?.TryEnqueue(() => {
                                    try {
                                        var package = new DataPackage();
                                        package.SetText(text);
                                        Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(package);
                                    } catch (Exception ex) { App.HandleError(ex); }
                                    AddHistoryItem(text, from, "📝", text);
                                });
                            }
                            break;
                        }
                        case NativeCore.PB_EVENT_CLIPBOARD_IMAGE:
                        {
                            var ptr = NativeCore.linkall_event_image_data(ev);
                            var len = (int)NativeCore.linkall_event_image_len(ev);
                            var from = NativeCore.PtrToUtf8String(NativeCore.linkall_event_device_name(ev)) ?? "Unknown";
                            if (ptr != IntPtr.Zero && len > 0)
                            {
                                // The event owns the bytes; copy them before it is freed.
                                var bytes = new byte[len];
                                Marshal.Copy(ptr, bytes, 0, len);
                                (_dispatcher ?? App.MainDispatcherQueue)?.TryEnqueue(async () => {
                                    try { await SetClipboardImageAsync(bytes); }
                                    catch (Exception ex) { App.HandleError(ex); }
                                    AddHistoryItem("Image", from, "🖼️", "", CacheImage(bytes));
                                });
                            }
                            break;
                        }
                        case NativeCore.PB_EVENT_FILE_TRANSFER_INCOMING:
                        case NativeCore.PB_EVENT_CLIPBOARD_FILE:
                        {
                            var fileName = NativeCore.PtrToUtf8String(NativeCore.linkall_event_transfer_file_name(ev)) ?? "File";
                            var from = NativeCore.PtrToUtf8String(NativeCore.linkall_event_device_name(ev)) ?? "Unknown";
                            var transferId = NativeCore.PtrToUtf8String(NativeCore.linkall_event_transfer_id(ev)) ?? "";
                            if (NativeCore.linkall_event_transfer_in_folder(ev) != 0)
                            {
                                // One question per folder: the answer covers all of it.
                                var folder = fileName.Split('/')[0];
                                if (!ShouldAskAboutFolder(from + "/" + folder)) break;
                                (_dispatcher ?? App.MainDispatcherQueue)?.TryEnqueue(() => {
                                    NotificationHelper.ShowToastWithActions(
                                        $"Incoming folder from {from}",
                                        folder,
                                        null,
                                        $"linkall://accept/{transferId}",
                                        $"linkall://reject/{transferId}"
                                    );
                                });
                                break;
                            }
                            (_dispatcher ?? App.MainDispatcherQueue)?.TryEnqueue(() => {
                                AddHistoryItem(fileName, from, "📎", fileName);
                                NotificationHelper.ShowToastWithActions(
                                    $"Incoming File from {from}",
                                    fileName,
                                    null,
                                    $"linkall://accept/{transferId}",
                                    $"linkall://reject/{transferId}"
                                );
                            });
                            break;
                        }
                        case NativeCore.PB_EVENT_CALL_STATE_CHANGED:
                        {
                            var call = new PhoneCall(
                                DeviceId: NativeCore.PtrToUtf8String(NativeCore.linkall_event_device_id(ev)) ?? "",
                                DeviceName: NativeCore.PtrToUtf8String(NativeCore.linkall_event_device_name(ev)) ?? "",
                                State: NativeCore.PtrToUtf8String(NativeCore.linkall_event_text(ev)) ?? "",
                                Number: NativeCore.PtrToUtf8String(NativeCore.linkall_event_call_number(ev)) ?? "",
                                ContactName: NativeCore.PtrToUtf8String(NativeCore.linkall_event_call_contact_name(ev)) ?? "");
                            (_dispatcher ?? App.MainDispatcherQueue)?.TryEnqueue(() => {
                                try { IncomingCallBannerWindow.OnCallState(call); } catch (Exception ex) { App.HandleError(ex); }
                            });
                            break;
                        }

                        // The engine already emits these events (this poll loop already
                        // drains them every 30ms) - they just had no handler wired up,
                        // so the app ran silently for anything but clipboard/file-offer/
                        // camera-call while minimized to the tray.
                        case NativeCore.PB_EVENT_FOLDER_TRANSFER_COMPLETE:
                        {
                            var folder = NativeCore.PtrToUtf8String(NativeCore.linkall_event_transfer_file_name(ev)) ?? "Folder";
                            var device = NativeCore.PtrToUtf8String(NativeCore.linkall_event_device_name(ev)) ?? "Unknown device";
                            var destDir = NativeCore.PtrToUtf8String(NativeCore.linkall_event_transfer_dest_path(ev));
                            var total = NativeCore.linkall_event_folder_file_count(ev);
                            var failed = NativeCore.linkall_event_folder_failed_count(ev);
                            var noun = total == 1 ? "file" : "files";
                            var files = failed == 0 ? $"{total} {noun}" : $"{total - failed} of {total} {noun}";
                            (_dispatcher ?? App.MainDispatcherQueue)?.TryEnqueue(() => {
                                if (!string.IsNullOrEmpty(destDir))
                                {
                                    NotificationHelper.ShowToast("Folder Received", $"{folder} ({files}) from {device}");
                                    AddHistoryItem(folder, device, "📁", destDir);
                                }
                                else
                                {
                                    NotificationHelper.ShowToast("Folder Sent", $"{folder} ({files}) to {device}");
                                }
                            });
                            break;
                        }
                        case NativeCore.PB_EVENT_FILE_TRANSFER_COMPLETE:
                        {
                            // A folder's files report once, as the folder.
                            if (NativeCore.linkall_event_transfer_in_folder(ev) != 0) break;
                            var fileName = NativeCore.PtrToUtf8String(NativeCore.linkall_event_transfer_file_name(ev)) ?? "File";
                            var device = NativeCore.PtrToUtf8String(NativeCore.linkall_event_device_name(ev)) ?? "Unknown device";
                            var destPath = NativeCore.PtrToUtf8String(NativeCore.linkall_event_transfer_dest_path(ev));
                            (_dispatcher ?? App.MainDispatcherQueue)?.TryEnqueue(() => {
                                if (!string.IsNullOrEmpty(destPath))
                                    NotificationHelper.ShowToast("File Received", $"{fileName} from {device}");
                                else
                                    NotificationHelper.ShowToast("File Sent", $"{fileName} to {device}");
                            });
                            break;
                        }
                        case NativeCore.PB_EVENT_FILE_TRANSFER_FAILED:
                        {
                            if (NativeCore.linkall_event_transfer_in_folder(ev) != 0) break;
                            var fileName = NativeCore.PtrToUtf8String(NativeCore.linkall_event_transfer_file_name(ev)) ?? "File";
                            var device = NativeCore.PtrToUtf8String(NativeCore.linkall_event_device_name(ev)) ?? "Unknown device";
                            (_dispatcher ?? App.MainDispatcherQueue)?.TryEnqueue(() => {
                                NotificationHelper.ShowToast("Transfer Failed", $"{fileName} with {device} could not complete");
                            });
                            break;
                        }
                        case NativeCore.PB_EVENT_PAIRING_REQUESTED:
                        {
                            var device = NativeCore.PtrToUtf8String(NativeCore.linkall_event_device_name(ev)) ?? "A device";
                            var deviceId = NativeCore.PtrToUtf8String(NativeCore.linkall_event_device_id(ev)) ?? "";
                            // The event carries the code; the peer list only has
                            // it after the next refresh, too late for this toast.
                            var eventPin = NativeCore.PtrToUtf8String(NativeCore.linkall_event_fingerprint(ev));
                            (_dispatcher ?? App.MainDispatcherQueue)?.TryEnqueue(() => {
                                if (string.IsNullOrEmpty(deviceId))
                                {
                                    NotificationHelper.ShowToast("Pairing request", $"{device} wants to pair with this PC");
                                    return;
                                }
                                var pin = !string.IsNullOrWhiteSpace(eventPin) && eventPin != "------"
                                    ? eventPin
                                    : LinkAllStore.Shared.Peers?.FirstOrDefault(p => p.device_id == deviceId)?.pairingPin;
                                var codeLine = string.IsNullOrWhiteSpace(pin)
                                    ? "Open Link All to compare the security code."
                                    : $"Security code {pin} - accept only if it matches.";
                                NotificationHelper.ShowToastWithActions(
                                    $"{device} wants to pair",
                                    codeLine,
                                    null,
                                    $"linkall://pair-accept/{deviceId}",
                                    $"linkall://pair-reject/{deviceId}");
                            });
                            break;
                        }
                        case NativeCore.PB_EVENT_PEER_CONNECTED:
                        {
                            var device = NativeCore.PtrToUtf8String(NativeCore.linkall_event_device_name(ev)) ?? "A device";
                            (_dispatcher ?? App.MainDispatcherQueue)?.TryEnqueue(() => {
                                NotificationHelper.ShowToast("Device Connected", $"{device} is now connected");
                            });
                            break;
                        }
                        case NativeCore.PB_EVENT_CAMERA_STREAM_STOP:
                        {
                            var device = NativeCore.PtrToUtf8String(NativeCore.linkall_event_device_id(ev));
                            (_dispatcher ?? App.MainDispatcherQueue)?.TryEnqueue(() => {
                                if (!string.IsNullOrEmpty(device)) CameraPreviewWindow.NotifyRemoteStreamStopped(device);
                            });
                            break;
                        }
                        // A phone's notification, mirrored. Nothing showed these
                        // before: the event arrived as a bare "activity updated".
                        case NativeCore.PB_EVENT_NOTIFICATION_RECEIVED:
                        {
                            var title = NativeCore.PtrToUtf8String(NativeCore.linkall_event_notification_title(ev)) ?? "";
                            var body = NativeCore.PtrToUtf8String(NativeCore.linkall_event_text(ev)) ?? "";
                            var device = NativeCore.PtrToUtf8String(NativeCore.linkall_event_device_name(ev)) ?? "your phone";
                            if (title.Length > 0 || body.Length > 0)
                            {
                                (_dispatcher ?? App.MainDispatcherQueue)?.TryEnqueue(() => {
                                    NotificationHelper.ShowToast(
                                        title.Length > 0 ? title : $"Notification from {device}",
                                        body.Length > 0 ? $"{body}\nFrom {device}" : $"From {device}");
                                });
                            }
                            break;
                        }
                        case NativeCore.PB_EVENT_WARNING:
                        {
                            var message = NativeCore.PtrToUtf8String(NativeCore.linkall_event_text(ev)) ?? "A device reported an issue";
                            var device = NativeCore.PtrToUtf8String(NativeCore.linkall_event_device_name(ev));
                            if (ShouldToastWarning(message))
                            {
                                (_dispatcher ?? App.MainDispatcherQueue)?.TryEnqueue(() => {
                                    NotificationHelper.ShowToast(string.IsNullOrEmpty(device) ? "Link All Warning" : $"Warning from {device}", message);
                                });
                            }
                            break;
                        }
                        // Cross-device link handoff: a trusted, connected peer asked us
                        // to open a URL. Engine already restricted this to http/https
                        // before emitting the event, so just open it and report back
                        // whether the OS-level open actually worked.
                        case NativeCore.PB_EVENT_OPEN_URL_ON_DEVICE_REQUESTED:
                        {
                            var url = NativeCore.PtrToUtf8String(NativeCore.linkall_event_text(ev));
                            var requesterId = NativeCore.PtrToUtf8String(NativeCore.linkall_event_device_id(ev));
                            var fromName = NativeCore.PtrToUtf8String(NativeCore.linkall_event_device_name(ev)) ?? "A device";
                            if (!string.IsNullOrEmpty(url) && !string.IsNullOrEmpty(requesterId))
                            {
                                bool opened;
                                string? openError = null;
                                try
                                {
                                    System.Diagnostics.Process.Start(new System.Diagnostics.ProcessStartInfo(url)
                                    {
                                        UseShellExecute = true
                                    });
                                    opened = true;
                                }
                                catch (Exception ex)
                                {
                                    opened = false;
                                    openError = ex.Message;
                                }
                                DaemonActions.RunFireAndForget("Open Link Ack", () => DaemonClient.AckOpenUrlOnDevice(requesterId, opened, openError));
                                if (opened)
                                {
                                    (_dispatcher ?? App.MainDispatcherQueue)?.TryEnqueue(() => {
                                        NotificationHelper.ShowToast("Link opened", $"From {fromName}");
                                    });
                                }
                            }
                            break;
                        }
                        case NativeCore.PB_EVENT_OPEN_URL_ON_DEVICE_ACK:
                        {
                            var success = NativeCore.linkall_event_open_url_ack_success(ev) != 0;
                            (_dispatcher ?? App.MainDispatcherQueue)?.TryEnqueue(() => {
                                NotificationHelper.ShowToast(
                                    success ? "Link opened" : "Couldn't open link",
                                    success ? "Opened on the other device" : "The other device reported an error");
                            });
                            break;
                        }
                    }
                }
                catch (Exception ex) { App.HandleError(ex); }
                finally
                {
                    NativeCore.linkall_free_event(ev);
                }
            }
            if (processedAny)
            {
                // Progress events arrive every tick while a transfer runs. A
                // full refresh is several IPC round trips that take the
                // engine's transfer lock, so refreshing on each one slowed
                // the transfer itself. Progress refreshes at most twice a
                // second; any other event still refreshes at once.
                var now = Environment.TickCount64;
                if (!onlyProgress || now - _lastProgressRefreshMs >= ProgressRefreshIntervalMs)
                {
                    _lastProgressRefreshMs = now;
                    LinkAllStore.Shared.UpdateStateFromDaemon();
                }
            }
        }

        private const long ProgressRefreshIntervalMs = 500;
        private long _lastProgressRefreshMs;

        // While the network is down the engine re-reports the same connect
        // failure every time discovery retries a peer - the trace log shows
        // 26k identical warnings in one hour. One toast per distinct message
        // per window is all a person can use; the rest is noise and memory.
        private const long WarningToastWindowMs = 5 * 60 * 1000;
        private const int MaxHistoryItems = 100;
        private readonly Dictionary<string, long> _lastWarningToastMs = new();

        // Each folder's files are offered a few at a time; ask once.
        private readonly Dictionary<string, long> _folderAskedMs = new();

        private bool ShouldAskAboutFolder(string key)
        {
            var now = Environment.TickCount64;
            lock (_folderAskedMs)
            {
                if (_folderAskedMs.TryGetValue(key, out var last) && now - last < WarningToastWindowMs) return false;
                if (_folderAskedMs.Count > 64) _folderAskedMs.Clear();
                _folderAskedMs[key] = now;
                return true;
            }
        }

        private bool ShouldToastWarning(string message)
        {
            var now = Environment.TickCount64;
            lock (_lastWarningToastMs)
            {
                if (_lastWarningToastMs.TryGetValue(message, out var last) && now - last < WarningToastWindowMs) return false;
                if (_lastWarningToastMs.Count > 64) _lastWarningToastMs.Clear();
                _lastWarningToastMs[message] = now;
                return true;
            }
        }

        private void AddHistoryItem(string summary, string source, string icon, string fullText, string? path = null)
        {
            var item = new HistoryItem
            {
                path = path ?? "",
                Summary = summary.Length > 80 ? summary[..77] + "…" : summary,
                FullText = fullText,
                Source = source,
                TypeIcon = icon,
                Time = DateTime.Now,
                RelativeTime = "Just now",
                display_text = summary,
                is_text = icon == "📝"
            };
            History.Insert(0, item);
            if (History.Count > MaxHistoryItems) History.RemoveAt(History.Count - 1);
            try
            {
                // Same cap for the store's copy, which used to grow for the
                // life of the process (every clipboard text kept in full).
                // Pinned items are kept regardless; the oldest unpinned goes.
                var shared = LinkAllStore.Shared.History;
                shared.Insert(0, item);
                for (var i = shared.Count - 1; i >= 0 && shared.Count > MaxHistoryItems; i--)
                {
                    if (!shared[i].IsPinned) shared.RemoveAt(i);
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private async Task CheckClipboardAsync()
        {
            try
            {
                // "Share clipboard automatically" off: this watcher is the
                // automatic path, and it used to push every copy (and send
                // every copied file) regardless. Explicit sends elsewhere
                // still work.
                if (!LinkAllStore.Shared.SyncEnabled) return;

                var packageView = global::Windows.ApplicationModel.DataTransfer.Clipboard.GetContent();
                if (packageView == null) return;
                
                if (packageView.Contains(StandardDataFormats.Text))
                {
                    var text = await packageView.GetTextAsync();
                    if (!string.IsNullOrEmpty(text) && text != _lastText)
                    {
                        if (LinkAllStore.Shared.OtpShieldEnabled && IsSensitiveContent(text))
                        {
                            _lastText = text;
                            return; // Filter out sensitive OTP/passwords/tokens
                        }
                        _lastText = text;
                        _lastImageHash = string.Empty;
                        if (App.EngineHandle != IntPtr.Zero)
                        {
                            var handle = App.EngineHandle;
                            RunNativeOffUiThread(() => NativeCore.linkall_push_text(handle, text));
                        }
                        else
                        {
                            DaemonActions.RunFireAndForget("Push Text", () => DaemonClient.PushText(text));
                        }
                        AddHistoryItem(text, "local", "📝", text);
                    }
                }
                else if (packageView.Contains(StandardDataFormats.Bitmap))
                {
                    // Copied images and Win+Shift+S screenshots.
                    var png = await ReadClipboardPngAsync(packageView);
                    if (png == null) return;
                    var hash = Convert.ToHexString(SHA256.HashData(png));
                    if (Environment.TickCount64 - _imageAppliedAtMs < ImageEchoWindowMs)
                    {
                        _lastImageHash = hash;
                        return;
                    }
                    if (hash == _lastImageHash) return;
                    _lastImageHash = hash;
                    _lastText = string.Empty;
                    if (App.EngineHandle != IntPtr.Zero)
                    {
                        var handle = App.EngineHandle;
                        RunNativeOffUiThread(() => NativeCore.linkall_push_image(handle, "image/png", png, (UIntPtr)png.Length));
                    }
                    else
                    {
                        DaemonActions.RunFireAndForget("Push Image", () => DaemonClient.PushImage(png));
                    }
                    AddHistoryItem("Image", "local", "🖼️", "", CacheImage(png));
                }
                // Files copied in Explorer (StorageItems) are deliberately not sent:
                // Ctrl+C between local folders must never broadcast a file to peers.
                // Files only leave the PC through an explicit send.
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        public void HandleIncomingData(System.Text.Json.JsonElement json)
        {
            if (json.TryGetProperty("type", out var typeProp) && typeProp.GetString() == "file")
            {
                string name = json.GetProperty("name").GetString() ?? "Unknown";
                string from = json.GetProperty("from").GetString() ?? "Unknown";
                _dispatcher?.TryEnqueue(() => {
                    AddHistoryItem(name, from, "📎", name);
                    NotificationHelper.ShowToastWithActions(
                        $"Incoming File from {from}",
                        name,
                        null,
                        "linkall://accept/0",
                        "linkall://reject/0"
                    );
                });
            }
        }

        public void PushFile(string path, string? targetDevice = null)
        {
            if (!string.IsNullOrEmpty(path) && Directory.Exists(path))
            {
                DaemonActions.RunFireAndForget("Send Folder", () => DaemonClient.SendFolder(path, targetDevice));
                return;
            }
            if (!string.IsNullOrEmpty(path) && File.Exists(path))
            {
                string name = Path.GetFileName(path);
                try {
                    if (App.EngineHandle != IntPtr.Zero)
                    {
                        var handle = App.EngineHandle;
                        RunNativeOffUiThread(() => NativeCore.linkall_send_file_path(handle, targetDevice, path, name, "application/octet-stream"));
                    }
                    else
                    {
                        DaemonActions.RunFireAndForget("Send File", () => DaemonClient.SendFilePath(path, name, "application/octet-stream", targetDevice));
                    }
                    _dispatcher.TryEnqueue(() => AddHistoryItem(name, "local", "📎", path));
                } catch (Exception ex) { App.HandleError(ex); }
            }
        }

        public void PushLocalClipboard()
        {
        }

        // Puts received image bytes (PNG or JPEG) on the clipboard. Windows
        // also offers them to older apps as a device-independent bitmap.
        // Images in history keep their bytes here, named by content hash, so
        // clicking one copies it again. The oldest go past MaxCachedImages.
        private const int MaxCachedImages = 100;
        private static readonly string ImageCacheDir = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "LinkAll", "clipboard_cache");

        private static string? CacheImage(byte[] bytes)
        {
            try
            {
                Directory.CreateDirectory(ImageCacheDir);
                var isJpeg = bytes.Length > 2 && bytes[0] == 0xFF && bytes[1] == 0xD8;
                var file = Path.Combine(ImageCacheDir, Convert.ToHexString(SHA256.HashData(bytes)) + (isJpeg ? ".jpg" : ".png"));
                if (!File.Exists(file)) File.WriteAllBytes(file, bytes);
                File.SetLastWriteTimeUtc(file, DateTime.UtcNow);
                foreach (var old in new DirectoryInfo(ImageCacheDir).GetFiles()
                             .OrderByDescending(f => f.LastWriteTimeUtc).Skip(MaxCachedImages))
                {
                    try { old.Delete(); } catch { }
                }
                return file;
            }
            catch (Exception ex) { App.HandleError(ex); return null; }
        }

        public static bool IsCachedImage(string? path) =>
            !string.IsNullOrEmpty(path)
            && path.StartsWith(ImageCacheDir, StringComparison.OrdinalIgnoreCase)
            && File.Exists(path);

        // A history image clicked: put it back on the clipboard.
        public async Task CopyImageAsync(string path)
        {
            var bytes = await File.ReadAllBytesAsync(path);
            await SetClipboardImageAsync(bytes);
            NotificationHelper.ShowToast("Image copied", "Paste it anywhere");
        }

        private async Task SetClipboardImageAsync(byte[] bytes)
        {
            // Not disposed: the clipboard reads the stream when an app pastes.
            var stream = new InMemoryRandomAccessStream();
            using (var writer = new DataWriter(stream.GetOutputStreamAt(0)))
            {
                writer.WriteBytes(bytes);
                await writer.StoreAsync();
                await writer.FlushAsync();
                writer.DetachStream();
            }
            var package = new DataPackage();
            package.SetBitmap(RandomAccessStreamReference.CreateFromStream(stream));
            _imageAppliedAtMs = Environment.TickCount64;
            global::Windows.ApplicationModel.DataTransfer.Clipboard.SetContent(package);
        }

        // Sends this PC's clipboard (text, or an image) to a device the user
        // picks. Shared by the home screen and the Clipboard page.
        public static async Task SendLocalClipboardAsync(XamlRoot? xamlRoot)
        {
            var store = LinkAllStore.Shared;
            var view = global::Windows.ApplicationModel.DataTransfer.Clipboard.GetContent();
            if (view.Contains(StandardDataFormats.Text))
            {
                var text = await view.GetTextAsync();
                if (string.IsNullOrEmpty(text)) return;
                var target = await DevicePicker.PickAsync(xamlRoot, store.ConnectedPeers);
                if (target != null) store.SendPushText(text, target.device_id);
            }
            else if (view.Contains(StandardDataFormats.Bitmap))
            {
                var png = await ReadClipboardPngAsync(view);
                if (png == null) return;
                var target = await DevicePicker.PickAsync(xamlRoot, store.ConnectedPeers);
                if (target != null) store.SendPushImage(png, target.device_id);
            }
            else
            {
                NotificationHelper.ShowToast("Nothing to send", "Copy some text or an image first.");
            }
        }

        // Reads the clipboard bitmap and encodes it as PNG, the format every
        // Link All platform puts on its own clipboard.
        public static async Task<byte[]?> ReadClipboardPngAsync(DataPackageView view)
        {
            var reference = await view.GetBitmapAsync();
            if (reference == null) return null;
            using var source = await reference.OpenReadAsync();
            var decoder = await BitmapDecoder.CreateAsync(source);
            using var bitmap = await decoder.GetSoftwareBitmapAsync(BitmapPixelFormat.Bgra8, BitmapAlphaMode.Premultiplied);
            using var output = new InMemoryRandomAccessStream();
            var encoder = await BitmapEncoder.CreateAsync(BitmapEncoder.PngEncoderId, output);
            encoder.SetSoftwareBitmap(bitmap);
            await encoder.FlushAsync();
            var bytes = new byte[output.Size];
            using var reader = new DataReader(output.GetInputStreamAt(0));
            await reader.LoadAsync((uint)output.Size);
            reader.ReadBytes(bytes);
            return bytes;
        }

        private bool IsSensitiveContent(string text)
        {
            if (string.IsNullOrWhiteSpace(text)) return false;
            var t = text.Trim();
            if (t.Length >= 4 && t.Length <= 8 && t.All(char.IsDigit)) return true; // 4-8 digit OTP code
            if (t.StartsWith("Bearer ", StringComparison.OrdinalIgnoreCase)) return true;
            if (t.StartsWith("sk-", StringComparison.OrdinalIgnoreCase)) return true;
            if (t.Contains("password", StringComparison.OrdinalIgnoreCase)) return true;
            return false;
        }

        public void Dispose()
        {
            Windows.ApplicationModel.DataTransfer.Clipboard.ContentChanged -= Clipboard_ContentChanged;
            _pollTimer.Stop();
        }
    }
}

