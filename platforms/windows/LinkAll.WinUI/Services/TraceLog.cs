using System;
using System.Collections.Concurrent;
using System.IO;
using System.Text;
using System.Threading;

namespace LinkAll.WinUI
{
    /// <summary>
    /// Central diagnostic trace log (%LOCALAPPDATA%\LinkAll\winui_trace.txt).
    /// Every call site used to open/write/close the file synchronously and
    /// individually - harmless for one-off startup/navigation logging, but
    /// ClipboardManager's 30ms poll timer did this per drained native event,
    /// unbounded, on the UI thread. Write() just enqueues; a background
    /// timer batches queued lines into one file write. Call Flush() right
    /// before anything that logs a crash/unhandled exception, or a
    /// deliberate process exit, so that diagnostic isn't lost to a delayed
    /// batch.
    /// </summary>
    public static class TraceLog
    {
        private static readonly string LogDir = Path.Combine(
            Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "LinkAll");
        private static readonly string LogPath = Path.Combine(LogDir, "winui_trace.txt");
        private static readonly ConcurrentQueue<string> _queue = new();
        private static readonly Timer _flushTimer;
        private static int _flushing;

        static TraceLog()
        {
            try { Directory.CreateDirectory(LogDir); } catch { }
            _flushTimer = new Timer(_ => Flush(), null, 500, 500);
        }

        public static void Write(string message) => _queue.Enqueue($"[{DateTime.Now:u}] {message}\n");

        public static void Flush()
        {
            if (_queue.IsEmpty) return;
            if (Interlocked.Exchange(ref _flushing, 1) == 1) return;
            try
            {
                var sb = new StringBuilder();
                while (_queue.TryDequeue(out var line)) sb.Append(line);
                if (sb.Length > 0) File.AppendAllText(LogPath, sb.ToString());
            }
            catch { }
            finally { Interlocked.Exchange(ref _flushing, 0); }
        }
    }
}
