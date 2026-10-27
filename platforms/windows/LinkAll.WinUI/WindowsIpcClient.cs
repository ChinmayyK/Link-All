using Microsoft.UI.Dispatching;
// WindowsIpcServer.cs
// Full named-pipe IPC server for Windows.
// Replaces the stub in ipc.rs for the C# tray application.
//
// The Rust daemon writes JSON to \\.\pipe\linkall;
// the C# app (and linkall-cli on Windows) reads/writes the same pipe.

using System;
using System.IO;
using System.IO.Pipes;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Threading;
using System.Threading.Tasks;

namespace LinkAll.WinUI
{
    /// <summary>
    /// Named-pipe client that talks to the running Link All daemon.
    /// Thread-safe: each request opens a fresh pipe connection.
    /// </summary>
    internal sealed class DaemonClient : IDisposable
    {
        // Must match get_pipe_name() in linkall-core/src/ipc_windows.rs.
        private static readonly string PipeName = "linkall_" +
            (Environment.GetEnvironmentVariable("LOCALAPPDATA") ?? "default").Replace('\\', '_').Replace(':', '_');
        private const int    TimeoutMs   = 1000;


        public static JsonDocument? SendFilePath(string path, string name, string mime, string? targetDevice = null, string? batchId = null, bool isDirectory = false, int itemCount = 1)
        {
            var req = Req("send_file_path", ("path", path), ("name", name), ("mime", mime), ("target_device", targetDevice), ("batch_id", batchId), ("is_directory", isDirectory), ("item_count", itemCount));
            return Send(req);
        }

        // A folder goes whole: the engine walks it and sends a few files at
        // a time, all sharing one batch id.
        public static JsonDocument? SendFolder(string path, string? targetDevice) =>
            Send(Req("send_folder", ("path", path), ("target_device", targetDevice)));

        public static JsonDocument? CancelFolder(string batchId) =>
            Send(Req("cancel_folder", ("batch_id", batchId)));

        // A file or a folder.
        public static JsonDocument? PushFile(string targetDevice, string path)
        {
            if (System.IO.Directory.Exists(path))
                return SendFolder(path, string.IsNullOrEmpty(targetDevice) ? null : targetDevice);
            var fileName = System.IO.Path.GetFileName(path);
            return SendFilePath(path, fileName, "application/octet-stream", string.IsNullOrEmpty(targetDevice) ? null : targetDevice);
        }

        // ── Security ────────────────────────────────────────────────────────────

        /// <summary>
        /// Returns true if the daemon is currently reachable.
        /// </summary>
        public static bool IsDaemonRunning()
        {
            try
            {
                using var pipe = OpenPipe(TimeoutMs / 4);
                return pipe != null;
            }
            catch { return false; }
        }



        /// <summary>
        /// Send a JSON command and return the parsed response.
        /// Returns null if the daemon is not running.
        /// </summary>
        public static JsonDocument? Send(JsonObject request) => Send(request, TimeoutMs);

        public static JsonDocument? Send(JsonObject request, int timeoutMs)
        {
            try
            {
                using var pipe = OpenPipe(timeoutMs);
                if (pipe == null) return null;

                // Write request (newline-delimited JSON).
                var json = request.ToJsonString() + "\n";
                var bytes = Encoding.UTF8.GetBytes(json);
                pipe.Write(bytes, 0, bytes.Length);
                pipe.Flush();

                // Read response line.
                var line = ReadLineWithTimeout(pipe, timeoutMs);
                if (line != null)
                {
                    try { return JsonDocument.Parse(line); } catch { return null; }
                }
                return null;
            }
            catch { return null; }
        }

        // Async version for use in async contexts (tray event handlers).
        public static async Task<JsonDocument?> SendAsync(JsonObject request,
            CancellationToken ct = default)
        {
            return await Task.Run(() => Send(request), ct);
        }

        public static async Task<JsonDocument?> SendAsync(JsonObject request, int timeoutMs, CancellationToken ct = default)
        {
            return await Task.Run(() => Send(request, timeoutMs), ct);
        }

        // ── Convenience commands ──────────────────────────────────────────────

        public static JsonDocument? Ping()       => Send(Req("ping"));
        public static JsonDocument? Status()     => Send(Req("status"));
        public static JsonDocument? Peers()      => Send(Req("peers"));

        // The daemon takes the settings patch as a JSON string field.
        public static JsonDocument? PatchSettings(JsonObject patch) =>
            Send(Req("patch_settings", ("patch", patch.ToJsonString())));

        // Requests are built as JsonObject rather than serialised anonymous objects: reflection
        // over anonymous types does not survive trimming or Native AOT, JsonObject does.
        public static JsonObject Req(string cmd, params (string Key, JsonNode? Value)[] fields)
        {
            var o = Fields(fields);
            o.Insert(0, "cmd", cmd);
            return o;
        }

        public static JsonObject Fields(params (string Key, JsonNode? Value)[] fields)
        {
            var o = new JsonObject();
            foreach (var (key, value) in fields) o[key] = value;
            return o;
        }


        // ── Private transport ─────────────────────────────────────────────────

        private static NamedPipeClientStream? OpenPipe(int timeoutMs)
        {
            var pipe = new NamedPipeClientStream(".", PipeName, PipeDirection.InOut, PipeOptions.Asynchronous);
            try
            {
                pipe.Connect(Math.Max(1, timeoutMs));
                return pipe;
            }
            catch
            {
                pipe.Dispose();
                return null;
            }
        }

        // One read deadline for the whole reply. A blocking Read can't be
        // interrupted by a clock check between bytes, so the read is async
        // (the pipe is opened Asynchronous) and the deadline cancels it and
        // disposes the pipe, which unblocks a stalled engine reply for good.
        // ConfigureAwait(false) throughout: callers may be on the UI thread
        // and block on this, and a captured context would deadlock them.
        private static string? ReadLineWithTimeout(NamedPipeClientStream stream, int timeoutMs)
        {
            using var cts = new CancellationTokenSource(Math.Max(1, timeoutMs));
            using var onTimeout = cts.Token.Register(() => { try { stream.Dispose(); } catch { } });
            try { return ReadLineAsync(stream, cts.Token).GetAwaiter().GetResult(); }
            catch { return null; }
        }

        private static async Task<string?> ReadLineAsync(Stream stream, CancellationToken ct)
        {
            var line = new MemoryStream();
            var buf  = new byte[4096];
            while (true)
            {
                int n = await stream.ReadAsync(buf.AsMemory(0, buf.Length), ct).ConfigureAwait(false);
                if (n == 0) break;
                int nl = Array.IndexOf(buf, (byte)'\n', 0, n);
                if (nl >= 0) { line.Write(buf, 0, nl); break; }
                line.Write(buf, 0, n);
            }
            return line.Length > 0 ? Encoding.UTF8.GetString(line.GetBuffer(), 0, (int)line.Length) : null;
        }

        public static JsonDocument? PushText(string text) =>
            Send(Req("push_text", ("text", text)));

        public static JsonDocument? PushTextTo(string text, string targetDevice) =>
            Send(Req("push_text_to", ("text", text), ("target", targetDevice)));

        public static JsonDocument? PushImage(byte[] png) =>
            Send(Req("push_image", ("mime", "image/png"), ("data_base64", Convert.ToBase64String(png))));

        public static JsonDocument? PushClipboard(string? targetDeviceId = null) =>
            Send(Req("push_clipboard", ("target_device_id", targetDeviceId)));

        public static JsonDocument? SetSyncEnabled(bool enabled) =>
            Send(Req("set_sync_enabled", ("enabled", enabled)));

        public static JsonDocument? PushBatteryStatus(int level, bool charging) =>
            Send(Req("push_battery_status", ("level", level), ("charging", charging)));

        public static JsonDocument? PushStorageStatus(ulong imagesBytes, ulong videosBytes, ulong appsBytes, ulong freeBytes, ulong totalBytes) =>
            Send(Req("push_storage_status", ("images_bytes", imagesBytes), ("videos_bytes", videosBytes), ("apps_bytes", appsBytes), ("free_bytes", freeBytes), ("total_bytes", totalBytes)));

        public static JsonDocument? HistoryClear() => Send(Req("history_clear"));

        public static JsonDocument? History(int last = 20) =>
            Send(Req("history", ("last", last)));

        public static JsonDocument? RevokeTrustedDevice(string deviceId) =>
            Send(Req("revoke_trusted_device", ("device_id", deviceId)));

        // ── Transfer Controls ─────────────────────────────────────────────────
        public static JsonDocument? SendPairingRequest(string deviceId) => Send(Req("send_pairing_request", ("device_id", deviceId)));
        public static JsonDocument? GenerateQrToken() => Send(Req("generate_qr_token"));
        public static JsonDocument? RespondToPairing(string deviceId, bool accepted) => Send(Req("respond_to_pairing", ("device_id", deviceId), ("accepted", accepted)));
        public static JsonDocument? CancelPairingRequest(string deviceId) => Send(Req("cancel_pairing_request", ("device_id", deviceId)));
        public static JsonDocument? AcceptFileTransfer(string transferId) => Send(Req("accept_file_transfer", ("transfer_id", transferId)));
        public static JsonDocument? RejectFileTransfer(string transferId, string reason) => Send(Req("reject_file_transfer", ("transfer_id", transferId), ("reason", reason)));
        public static JsonDocument? PauseFileTransfer(string transferId) => Send(Req("pause_file_transfer", ("transfer_id", transferId)));
        public static JsonDocument? ResumeFileTransfer(string transferId) => Send(Req("resume_file_transfer", ("transfer_id", transferId)));
        public static JsonDocument? CancelFileTransfer(string transferId) => Send(Req("cancel_file_transfer", ("transfer_id", transferId)));
        public static JsonDocument? StartSpeedTest(string deviceId, int durationSecs = 10) => Send(Req("start_speed_test", ("device_id", deviceId), ("duration_secs", durationSecs)));

        // ── Device Management ─────────────────────────────────────────────────
        public static JsonDocument? DisconnectPeer(string deviceId) => Send(Req("disconnect_peer", ("device_id", deviceId)));
        public static JsonDocument? DisconnectAllPeers()
        {
            try
            {
                var peers = LinkAllStore.Shared.Peers;
                System.Collections.Generic.List<string> ids = new();
                if (App.MainDispatcherQueue?.HasThreadAccess == true)
                {
                    foreach (var p in peers) ids.Add(p.device_id);
                }
                else
                {
                    // If on background thread, send explicit IPC command or safely marshal
                    Send(Req("disconnect_all_peers"));
                    return null;
                }
                foreach (var id in ids) DisconnectPeer(id);
            }
            catch (Exception ex) { App.HandleError(ex); }
            return null;
        }
        public static JsonDocument? RescanPeers() => Send(Req("rescan_peers"));
        public static JsonDocument? RenameTrustedDevice(string deviceId, string displayName) => Send(Req("rename_trusted_device", ("device_id", deviceId), ("display_name", displayName)));
        public static JsonDocument? PauseSyncPeer(string deviceId) => Send(Req("pause_sync_peer", ("device_id", deviceId)));
        public static JsonDocument? ResumeSyncPeer(string deviceId) => Send(Req("resume_sync_peer", ("device_id", deviceId)));
        public static JsonDocument? ForgetDevice(string deviceId) => Send(Req("forget_device", ("device_id", deviceId)));
        public static JsonDocument? SetAutoConnect(string deviceId, bool enabled) => Send(Req("set_auto_connect", ("device_id", deviceId), ("enabled", enabled)));

        // ── Activity & Settings ───────────────────────────────────────────────
        public static JsonDocument? ActivityRecent(int limit) => Send(Req("activity_recent", ("limit", limit)));
        public static JsonDocument? PendingRemoteClipboards() => Send(Req("pending_remote_clipboards"));
        public static JsonDocument? ApplyClipboard(string contentHash) => Send(Req("apply_clipboard", ("content_hash", contentHash)));
        public static JsonDocument? GetSettings() => Send(Req("get_settings"));

        /// <summary>Where the keyboard and mouse hand-off stands. <c>peek</c> looks without clearing what the hand-off itself acts on.</summary>
        public static JsonDocument? EdgeStatus(bool peek) => Send(Req("edge_status", ("peek", peek)));
        public static JsonDocument? GetMetrics() => Send(Req("get_metrics"));

        // ── Remote File Explorer ──────────────────────────────────────────────
        // The daemon waits up to 10s for the remote peer's reply
        // (query_remote_files_sync's timeout_secs, ipc.rs) before giving up -
        // an unfiltered/large listing genuinely takes a few seconds over a
        // P2P link. The local pipe read needs a matching margin, or this
        // gives up first and discards a reply that was still on its way,
        // showing a spurious empty folder. Mirrors RemoteThumbnailRequestAsync's
        // ThumbnailTimeoutMs below.
        private const int RemoteFilesQueryTimeoutMs = 12000;
        public static async Task<JsonDocument?> RemoteFilesQueryAsync(string deviceId, bool summaryOnly = false, string? category = null, string? source = null, string? searchQuery = null, uint offset = 0, uint limit = 100)
        {
            var req = new JsonObject
            {
                ["cmd"] = "remote_files_query",
                ["target_device"] = deviceId,
                ["summary_only"] = summaryOnly,
                ["offset"] = offset,
                ["limit"] = limit
            };
            if (!string.IsNullOrEmpty(category)) req["category"] = category;
            if (!string.IsNullOrEmpty(source)) req["source"] = source;
            if (!string.IsNullOrEmpty(searchQuery)) req["search_query"] = searchQuery;
            return await SendAsync(req, RemoteFilesQueryTimeoutMs);
        }
            
        public static JsonDocument? RemoteFilePullRequest(string deviceId, ulong fileId) =>
            Send(Req("remote_file_pull_request", ("target_device", deviceId), ("file_id", fileId)));

        // Asks the remote peer to generate/send a thumbnail for one file
        // (JPEG bytes, base64-encoded in the response) - the local daemon
        // does not cache or generate these itself. Mirrors macOS's
        // requestRemoteThumbnail(targetDevice:fileId:sizePx:). The engine
        // waits up to 10s for the peer's reply (engine/mod.rs), so this
        // needs a longer local pipe read timeout than the default 1s.
        private const int ThumbnailTimeoutMs = 12000;
        public static async Task<JsonDocument?> RemoteThumbnailRequestAsync(string deviceId, ulong fileId, uint sizePx = 256) =>
            await SendAsync(Req("remote_thumbnail_request", ("target_device", deviceId), ("file_id", fileId), ("size_px", sizePx)), ThumbnailTimeoutMs);

        public static JsonDocument? RemoteFileActionRequest(string deviceId, ulong fileId, string action, string? newName = null)
        {
            var req = new JsonObject
            {
                ["cmd"] = "remote_file_action_request",
                ["target_device"] = deviceId,
                ["file_id"] = fileId,
                ["action"] = action
            };
            if (!string.IsNullOrEmpty(newName)) req["new_name"] = newName;
            return Send(req);
        }

        // Cross-device link handoff: ask a trusted, connected peer to open a
        // URL immediately (fire-and-forget; the peer reports back over its
        // own AckOpenUrlOnDevice call once it knows whether the OS-level
        // open actually succeeded).
        public static JsonDocument? OpenUrlOnDevice(string deviceId, string url) =>
            Send(Req("open_url_on_device", ("target_device", deviceId), ("url", url)));

        // Make a paired phone ring (ring = true) or stop. The daemon answers once
        // the phone has: an error status carries the phone's reason for not ringing.
        // The daemon waits up to 5 s for the phone's answer, and a sleeping
        // phone often takes more than the default second: the phone rang, but
        // the app saw no reply, said it failed, and offered no Stop button.
        public static JsonDocument? FindDevice(string deviceId, bool ring) =>
            Send(Req("find_device", ("target", deviceId), ("ring", ring)), 7000);

        // Ask a paired phone to take a photo ("photo") or scan a document ("document") and send it back.
        public static JsonDocument? CaptureRequest(string deviceId, string mode) =>
            Send(Req("capture_request", ("target", deviceId), ("mode", mode)));

        // Offer a paired phone a call; its owner taps a notification to open the dialer.
        public static JsonDocument? DialNumber(string deviceId, string number) =>
            Send(Req("dial_number", ("target", deviceId), ("number", number)));

        // The devices a purpose ("send", "remote", "find" ...) can go to, ranked.
        public static JsonDocument? DeviceChoices(string purpose) =>
            Send(Req("device_choices", ("purpose", purpose)));

        // Guest Drop: a link and PIN for someone who isn't paired, and stop it.
        public static JsonDocument? GuestDropStart() => Send(Req("guest_drop_start"));
        public static JsonDocument? GuestDropStop() => Send(Req("guest_drop_stop"));

        // Report back whether we actually managed to open a URL a peer asked
        // us to open. Called after the local Process.Start attempt.
        public static JsonDocument? AckOpenUrlOnDevice(string requesterDeviceId, bool success, string? error = null)
        {
            var req = new JsonObject
            {
                ["cmd"] = "ack_open_url_on_device",
                ["requester_device"] = requesterDeviceId,
                ["success"] = success
            };
            if (!string.IsNullOrEmpty(error)) req["error"] = error;
            return Send(req);
        }

        public static JsonDocument? Shutdown() => Send(Req("shutdown"));
        
        public void Dispose() { }
    }

    /// <summary>
    /// Polls the daemon every N seconds and fires events on state changes.
    /// Used by the tray app to update the tooltip and menu items.
    /// </summary>
    internal sealed class DaemonPoller : IDisposable
    {
        private const int FastMs = 1000;
        private const int SlowMs = 5000;

        private CancellationTokenSource? _cts;
        private Task? _pollTask;

        private bool _wasDaemonRunning;
        private int  _lastPeerCount        = -1;
        private bool _lastSyncState        = true;
        private int  _lastPendingClipboard = -1;

        public event Action<bool>? DaemonAvailabilityChanged;
        public event Action<int>?  PeerCountChanged;
        public event Action<bool>? SyncStateChanged;
        public event Action<int>?  PendingClipboardCountChanged;

        public DaemonPoller()
        {
            _cts = new CancellationTokenSource();
            _pollTask = Task.Run(() => PollLoopAsync(_cts.Token));
        }

        private async Task PollLoopAsync(CancellationToken token)
        {
            while (!token.IsCancellationRequested)
            {
                int delayMs = SlowMs;
                try
                {
                    bool running = DaemonClient.IsDaemonRunning();
                    if (running != _wasDaemonRunning)
                    {
                        _wasDaemonRunning = running;
                        try { DaemonAvailabilityChanged?.Invoke(running); } catch (Exception ex) { App.HandleError(ex); }
                    }

                    if (running)
                    {
                        var resp = await DaemonClient.SendAsync(DaemonClient.Req("status"), token);
                        if (resp != null)
                        {
                            try
                            {
                                var root = resp.RootElement;
                                if (root.TryGetProperty("data", out var data))
                                {
                                    int peerCount = data.TryGetProperty("peer_count", out var pc) ? pc.GetInt32() : 0;
                                    bool syncEnabled = !data.TryGetProperty("sync_enabled", out var se) || se.GetBoolean();
                                    int pending = data.TryGetProperty("pending_clipboard_count", out var pcc) ? pcc.GetInt32() : 0;

                                    if (peerCount != _lastPeerCount)
                                    { _lastPeerCount = peerCount; try { PeerCountChanged?.Invoke(peerCount); } catch (Exception ex) { App.HandleError(ex); } }
                                    if (syncEnabled != _lastSyncState)
                                    { _lastSyncState = syncEnabled; try { SyncStateChanged?.Invoke(syncEnabled); } catch (Exception ex) { App.HandleError(ex); } }
                                    if (pending != _lastPendingClipboard)
                                    { _lastPendingClipboard = pending; try { PendingClipboardCountChanged?.Invoke(pending); } catch (Exception ex) { App.HandleError(ex); } }
                                }
                            }
                            catch (Exception ex) { App.HandleError(ex); }
                            
                            delayMs = _lastPeerCount > 0 ? FastMs : SlowMs;
                        }
                    }
                }
                catch (Exception ex)
                {
                    App.HandleError(ex);
                }

                try
                {
                    await Task.Delay(delayMs, token);
                }
                catch (TaskCanceledException)
                {
                    break;
                }
            }
        }

        public void Dispose()
        {
            _cts?.Cancel();
            _cts?.Dispose();
        }
    }
}









