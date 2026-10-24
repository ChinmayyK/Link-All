using System;
using System.Collections.Concurrent;
using System.Runtime.CompilerServices;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;

namespace LinkAll.WinUI.Services
{
    /// <summary>
    /// Shares this PC's keyboard and mouse with a neighbouring computer. When the
    /// pointer is pushed against a screen edge that has a computer linked to it,
    /// the engine asks that computer to take over; until the pointer comes back
    /// (or Ctrl+Alt+Esc is pressed) this PC's mouse and keyboard are swallowed
    /// here and sent to it instead.
    ///
    /// The hooks run in the input path of the whole PC, so the callbacks do no
    /// more than compare numbers and queue work. All talking to the engine
    /// happens on the sender thread, in batches of about 8 ms.
    /// </summary>
    public sealed unsafe class EdgeControlService : IDisposable
    {
        public const string EnabledKey = "edge_control_enabled";
        public static string LinkKey(int edge) => "edge_link_" + edge;

        /// <summary>0 left, 1 right, 2 top, 3 bottom: the same numbers the engine uses.</summary>
        public static readonly string[] EdgeNames = { "left", "right", "above", "below" };

        public static EdgeControlService? Current { get; private set; }

        private enum Mode { Idle = 0, Starting = 1, Remote = 2 }

        // Edge push: this many moves at the edge, and at least this long.
        private const int EdgeHitsNeeded = 4;
        private const long EdgeDwellMs = 120;
        // After a hand-off ends the pointer can't start another straight away.
        private const long CooldownMs = 800;
        // How long to wait for the event saying why the engine ended a hand-off.
        private const long OverWaitMs = 1000;
        // While the other computer is asked to take over, clicks and keys are
        // held for it until then, for at most this long; after that they are
        // this PC's again.
        private const long HoldWhileStartingMs = 500;

        private int _mode;
        private readonly string?[] _links = new string?[4];
        private readonly int[] _edgeHits = new int[4];
        private readonly long[] _edgeSince = new long[4];
        private long _cooldownUntil;
        private int _lastEdge;
        private int _centerX, _centerY;

        private int _dx, _dy, _wheelX, _wheelY;
        private int _wheelResidualX, _wheelResidualY;
        private readonly ConcurrentQueue<Op> _ops = new();
        private readonly AutoResetEvent _wake = new(false);
        private int _failures;
        private long _overSince;

        private Thread? _hookThread;
        private Thread? _senderThread;
        private uint _hookThreadId;
        private volatile bool _stop;
        private bool _ctrl, _alt;
        private long _holdUntil;
        // Keys that went down on this PC before a hand-off, by virtual key:
        // their releases stay here too, or Windows would think them held.
        private readonly bool[] _localDown = new bool[256];

        private readonly record struct Op(OpKind Kind, int A, bool Down);
        private enum OpKind { Button, Key }

        public EdgeControlService()
        {
            Current = this;
            Reload();
        }

        public bool Enabled => LocalSettingsStore.GetBool(EnabledKey, false);

        /// <summary>Reads the settings again; starts or stops the hooks to match.</summary>
        public void Reload()
        {
            bool any = false;
            for (int e = 0; e < 4; e++)
            {
                var id = LocalSettingsStore.Get(LinkKey(e));
                _links[e] = string.IsNullOrEmpty(id) ? null : id;
                any |= _links[e] != null;
                _edgeHits[e] = 0;
            }
            if (Enabled && any) StartHooks();
            else
            {
                // A start still waiting for its answer is called off; Begin
                // sees the mode changed and ends it in the engine.
                Interlocked.CompareExchange(ref _mode, (int)Mode.Idle, (int)Mode.Starting);
                EndRemote("Sharing was turned off.", tellEngine: true);
                StopHooks();
            }
        }

        // ── Hook thread ──────────────────────────────────────────────────────

        private void StartHooks()
        {
            if (_hookThread != null) return;
            _stop = false;
            var ready = new ManualResetEventSlim(false);
            // Every key and mouse move on the PC waits for the hook thread, and
            // a hand-off's input for the sender: neither may queue behind the UI.
            _hookThread = new Thread(() => HookMain(ready)) { IsBackground = true, Name = "LinkAll edge hooks", Priority = ThreadPriority.Highest };
            _hookThread.Start();
            ready.Wait(2000);
            _senderThread = new Thread(SenderMain) { IsBackground = true, Name = "LinkAll edge sender", Priority = ThreadPriority.Highest };
            _senderThread.Start();
        }

        private void StopHooks()
        {
            if (_hookThread == null) return;
            _stop = true;
            _wake.Set();
            if (_hookThreadId != 0) PostThreadMessageW(_hookThreadId, WM_QUIT, UIntPtr.Zero, IntPtr.Zero);
            _hookThread.Join(1500);
            _senderThread?.Join(500);
            _hookThread = null;
            _senderThread = null;
            _hookThreadId = 0;
        }

        private void HookMain(ManualResetEventSlim ready)
        {
            IntPtr mouseHook = IntPtr.Zero, keyHook = IntPtr.Zero;
            try
            {
                _hookThreadId = GetCurrentThreadId();
                var module = GetModuleHandleW(null);
                mouseHook = SetWindowsHookExW(WH_MOUSE_LL, (IntPtr)(delegate* unmanaged<int, IntPtr, IntPtr, IntPtr>)&MouseProc, module, 0);
                keyHook = SetWindowsHookExW(WH_KEYBOARD_LL, (IntPtr)(delegate* unmanaged<int, IntPtr, IntPtr, IntPtr>)&KeyProc, module, 0);
                if (mouseHook == IntPtr.Zero || keyHook == IntPtr.Zero)
                    TraceLog.Write("Edge control: could not install the input hooks");
            }
            catch (Exception ex) { App.HandleError(ex); }
            finally { ready.Set(); }

            while (!_stop && GetMessageW(out _, IntPtr.Zero, 0, 0) > 0) { }

            if (mouseHook != IntPtr.Zero) UnhookWindowsHookEx(mouseHook);
            if (keyHook != IntPtr.Zero) UnhookWindowsHookEx(keyHook);
        }

        [UnmanagedCallersOnly]
        private static IntPtr MouseProc(int nCode, IntPtr wParam, IntPtr lParam)
        {
            try
            {
                var self = Current;
                if (nCode >= 0 && self != null)
                {
                    var ms = *(MSLLHOOKSTRUCT*)lParam;
                    if ((ms.flags & LLMHF_INJECTED) == 0 && self.OnMouse((int)wParam, ms))
                        return (IntPtr)1;
                }
            }
            catch { }
            return CallNextHookEx(IntPtr.Zero, nCode, wParam, lParam);
        }

        [UnmanagedCallersOnly]
        private static IntPtr KeyProc(int nCode, IntPtr wParam, IntPtr lParam)
        {
            try
            {
                var self = Current;
                if (nCode >= 0 && self != null)
                {
                    var kb = *(KBDLLHOOKSTRUCT*)lParam;
                    if ((kb.flags & LLKHF_INJECTED) == 0 && self.OnKey(kb))
                        return (IntPtr)1;
                }
            }
            catch { }
            return CallNextHookEx(IntPtr.Zero, nCode, wParam, lParam);
        }

        // ── What the hooks decide (kept cheap: they run on every input) ──────

        /// <returns>True to swallow the event.</returns>
        private bool OnMouse(int message, in MSLLHOOKSTRUCT ms)
        {
            var mode = (Mode)Volatile.Read(ref _mode);
            if (mode == Mode.Idle)
            {
                if (message == WM_MOUSEMOVE) CheckEdges(ms.x, ms.y);
                return false;
            }
            if (mode == Mode.Starting)
            {
                if (Environment.TickCount64 >= Volatile.Read(ref _holdUntil)) return false;
                // The pointer waits at the edge; clicks and the wheel are kept.
                if (message == WM_MOUSEMOVE) return true;
            }

            switch (message)
            {
                case WM_MOUSEMOVE:
                    // The pointer is parked at the centre, so how far this event
                    // would have moved it is its distance from there.
                    int dx = ms.x - _centerX, dy = ms.y - _centerY;
                    if (dx != 0) Interlocked.Add(ref _dx, dx);
                    if (dy != 0) Interlocked.Add(ref _dy, dy);
                    _wake.Set();
                    break;
                case WM_LBUTTONDOWN: QueueButton(0, true); break;
                case WM_LBUTTONUP: QueueButton(0, false); break;
                case WM_RBUTTONDOWN: QueueButton(1, true); break;
                case WM_RBUTTONUP: QueueButton(1, false); break;
                case WM_MBUTTONDOWN: QueueButton(2, true); break;
                case WM_MBUTTONUP: QueueButton(2, false); break;
                case WM_XBUTTONDOWN: QueueButton(XButton(ms), true); break;
                case WM_XBUTTONUP: QueueButton(XButton(ms), false); break;
                case WM_MOUSEWHEEL: Interlocked.Add(ref _wheelY, (short)(ms.mouseData >> 16)); _wake.Set(); break;
                case WM_MOUSEHWHEEL: Interlocked.Add(ref _wheelX, (short)(ms.mouseData >> 16)); _wake.Set(); break;
            }
            return true;
        }

        // The side buttons are numbered 3 (back) and 4 (forward) on the wire.
        private static int XButton(in MSLLHOOKSTRUCT ms) => (ms.mouseData >> 16) == 1 ? 3 : 4;

        private void QueueButton(int button, bool down)
        {
            _ops.Enqueue(new Op(OpKind.Button, button, down));
            _wake.Set();
        }

        private bool OnKey(in KBDLLHOOKSTRUCT kb)
        {
            bool down = (kb.flags & LLKHF_UP) == 0;
            switch (kb.vkCode)
            {
                case 0x11: case 0xA2: case 0xA3: _ctrl = down; break;
                case 0x12: case 0xA4: case 0xA5: _alt = down; break;
            }
            var mode = (Mode)Volatile.Read(ref _mode);
            if (mode == Mode.Idle || (mode == Mode.Starting && Environment.TickCount64 >= Volatile.Read(ref _holdUntil)))
            {
                _localDown[kb.vkCode & 0xFF] = down;
                return false;
            }
            if (!down && _localDown[kb.vkCode & 0xFF])
            {
                _localDown[kb.vkCode & 0xFF] = false;
                return false;
            }

            // The way back that never depends on the other computer or the network.
            if (kb.vkCode == 0x1B && down && _ctrl && _alt)
            {
                Task.Run(() => EndRemote("You pressed Ctrl+Alt+Esc.", tellEngine: true));
                return true;
            }
            int scan = (int)kb.scanCode | ((kb.flags & LLKHF_EXTENDED) != 0 ? 0xE000 : 0);
            _ops.Enqueue(new Op(OpKind.Key, scan, down));
            _wake.Set();
            return true;
        }

        private void CheckEdges(int x, int y)
        {
            long now = Environment.TickCount64;
            if (now < _cooldownUntil) return;
            GetVirtualScreen(out int left, out int top, out int right, out int bottom);
            for (int e = 0; e < 4; e++)
            {
                if (_links[e] == null) continue;
                bool at = e switch
                {
                    0 => x <= left,
                    1 => x >= right - 1,
                    2 => y <= top,
                    _ => y >= bottom - 1,
                };
                if (!at) { _edgeHits[e] = 0; continue; }
                if (_edgeHits[e]++ == 0) _edgeSince[e] = now;
                // Dragging a window or a scroll bar to the edge is not a push.
                if (_edgeHits[e] >= EdgeHitsNeeded && now - _edgeSince[e] >= EdgeDwellMs && !ButtonHeld())
                {
                    _edgeHits[e] = 0;
                    int pos = e <= 1 ? Fraction(y, top, bottom) : Fraction(x, left, right);
                    Begin(e, pos);
                    return;
                }
            }
        }

        private static bool ButtonHeld() =>
            (GetAsyncKeyState(0x01) & 0x8000) != 0 || (GetAsyncKeyState(0x02) & 0x8000) != 0;

        private static int Fraction(int v, int min, int max) =>
            max - min <= 1 ? 0 : (int)(Math.Clamp(v - min, 0, max - min - 1) * 65535L / (max - min - 1));

        // ── Starting and ending a hand-off ───────────────────────────────────

        private void Begin(int edge, int pos)
        {
            while (_ops.TryDequeue(out _)) { }
            Interlocked.Exchange(ref _wheelX, 0);
            Interlocked.Exchange(ref _wheelY, 0);
            Volatile.Write(ref _holdUntil, Environment.TickCount64 + HoldWhileStartingMs);
            if (Interlocked.CompareExchange(ref _mode, (int)Mode.Starting, (int)Mode.Idle) != (int)Mode.Idle) return;
            var id = _links[edge];
            var handle = App.EngineHandle;
            Task.Run(() =>
            {
                try
                {
                    if (id == null || handle == IntPtr.Zero ||
                        NativeCore.linkall_edge_begin(handle, id, (byte)edge, (ushort)pos) != 0)
                    {
                        TraceLog.Write($"Edge control: the computer on the {EdgeNames[edge]} did not take the keyboard and mouse");
                        _cooldownUntil = Environment.TickCount64 + 3000;
                        Volatile.Write(ref _mode, (int)Mode.Idle);
                        return;
                    }
                    // Sharing may have been turned off, or this edge relinked,
                    // while the other computer answered: Reload then moved the
                    // mode off Starting (or the link changed). Going Remote
                    // would leave no hooks running, and the next StartHooks
                    // would swallow all local input.
                    if (_links[edge] != id ||
                        Volatile.Read(ref _mode) != (int)Mode.Starting)
                    {
                        Interlocked.CompareExchange(ref _mode, (int)Mode.Idle, (int)Mode.Starting);
                        NativeCore.linkall_edge_end(handle);
                        return;
                    }
                    _lastEdge = edge;
                    _centerX = GetSystemMetrics(SM_CXSCREEN) / 2;
                    _centerY = GetSystemMetrics(SM_CYSCREEN) / 2;
                    Interlocked.Exchange(ref _dx, 0);
                    Interlocked.Exchange(ref _dy, 0);
                    _wheelResidualX = _wheelResidualY = 0;
                    _failures = 0;
                    _overSince = 0;
                    // What was held while starting goes first, unless the
                    // answer took so long that this PC had its input back.
                    if (Environment.TickCount64 >= Volatile.Read(ref _holdUntil))
                    {
                        while (_ops.TryDequeue(out _)) { }
                        Interlocked.Exchange(ref _wheelX, 0);
                        Interlocked.Exchange(ref _wheelY, 0);
                    }
                    SetCursorPos(_centerX, _centerY);
                    if (Interlocked.CompareExchange(ref _mode, (int)Mode.Remote, (int)Mode.Starting) != (int)Mode.Starting)
                    {
                        NativeCore.linkall_edge_end(handle);
                        return;
                    }
                    TraceLog.Write($"Edge control: keyboard and mouse handed to the computer on the {EdgeNames[edge]}");
                }
                catch (Exception ex)
                {
                    App.HandleError(ex);
                    Volatile.Write(ref _mode, (int)Mode.Idle);
                }
            });
        }

        /// <summary>The engine reports the pointer came back over the edge it left by.</summary>
        public void OnReturned(int pos)
        {
            if (Interlocked.CompareExchange(ref _mode, (int)Mode.Idle, (int)Mode.Remote) != (int)Mode.Remote) return;
            PlaceAtEdge(pos);
            Settle();
            TraceLog.Write("Edge control: pointer came back");
        }

        /// <summary>The engine reports the hand-off ended without the pointer coming back.</summary>
        public void OnEnded(string reason)
        {
            if (Interlocked.CompareExchange(ref _mode, (int)Mode.Idle, (int)Mode.Remote) != (int)Mode.Remote) return;
            PlaceAtEdge(32768);
            Settle();
            NotificationHelper.ShowToast("Keyboard and mouse are back on this PC", reason);
        }

        /// <summary>Takes the keyboard and mouse back, for the page's button.</summary>
        public void EndSharing() => Task.Run(() => EndRemote("You took it back.", tellEngine: true));

        /// <summary>Takes the keyboard and mouse back from this side.</summary>
        private void EndRemote(string reason, bool tellEngine)
        {
            if (Interlocked.CompareExchange(ref _mode, (int)Mode.Idle, (int)Mode.Remote) != (int)Mode.Remote) return;
            var handle = App.EngineHandle;
            if (tellEngine && handle != IntPtr.Zero)
            {
                try { NativeCore.linkall_edge_end(handle); } catch (Exception ex) { App.HandleError(ex); }
            }
            PlaceAtEdge(32768);
            Settle();
            TraceLog.Write("Edge control: ended - " + reason);
            NotificationHelper.ShowToast("Keyboard and mouse are back on this PC", reason);
        }

        private void Settle()
        {
            _cooldownUntil = Environment.TickCount64 + CooldownMs;
            while (_ops.TryDequeue(out _)) { }
        }

        // Back just inside the edge it left by, `pos` of the way along it.
        private void PlaceAtEdge(int pos)
        {
            GetVirtualScreen(out int left, out int top, out int right, out int bottom);
            int along(int min, int max) => min + (int)(pos * (long)(max - min - 1) / 65535);
            const int inset = 6;
            int x, y;
            switch (_lastEdge)
            {
                case 0: x = left + inset; y = along(top, bottom); break;
                case 1: x = right - 1 - inset; y = along(top, bottom); break;
                case 2: x = along(left, right); y = top + inset; break;
                default: x = along(left, right); y = bottom - 1 - inset; break;
            }
            SetCursorPos(x, y);
        }

        // ── Sender thread ────────────────────────────────────────────────────

        private void SenderMain()
        {
            long lastCenter = 0;
            while (!_stop)
            {
                _wake.WaitOne(8);
                if ((Mode)Volatile.Read(ref _mode) != Mode.Remote) continue;
                var handle = App.EngineHandle;
                if (handle == IntPtr.Zero) continue;
                try
                {
                    while (_ops.TryDequeue(out var op))
                    {
                        // Movement first, so a click lands where the pointer was.
                        FlushMotion(handle);
                        int rc = op.Kind == OpKind.Button
                            ? NativeCore.linkall_edge_button(handle, (byte)op.A, op.Down ? 1 : 0)
                            : NativeCore.linkall_edge_key(handle, (ushort)op.A, op.Down ? 1 : 0);
                        Track(rc);
                    }
                    FlushMotion(handle);

                    long now = Environment.TickCount64;
                    if (now - lastCenter > 100)
                    {
                        lastCenter = now;
                        SetCursorPos(_centerX, _centerY);
                    }
                    if (_overSince != 0)
                    {
                        // The engine has already ended it; the pointer coming
                        // back, or the other computer ending it, arrives as an
                        // event that names the real reason. Only if that never
                        // comes does this side end it, so input isn't held.
                        if (now - _overSince > OverWaitMs)
                        {
                            _overSince = 0;
                            EndRemote("The other computer is no longer taking the keyboard and mouse.", tellEngine: false);
                        }
                    }
                    else if (_failures >= 5)
                    {
                        _failures = 0;
                        // Still sharing as far as the engine knows, but nothing
                        // gets through: end it there too, or it stays held.
                        EndRemote("The other computer stopped answering.", tellEngine: true);
                    }
                }
                catch (Exception ex) { App.HandleError(ex); }
            }
        }

        // 0 sent, 1 the hand-off is already over in the engine (an event says
        // why), -1 not sent.
        private void Track(int rc)
        {
            if (rc == 1)
            {
                if (_overSince == 0) _overSince = Environment.TickCount64;
                return;
            }
            _failures = rc == 0 ? 0 : _failures + 1;
        }

        private void FlushMotion(IntPtr handle)
        {
            int dx = Interlocked.Exchange(ref _dx, 0), dy = Interlocked.Exchange(ref _dy, 0);
            while (dx != 0 || dy != 0)
            {
                int sx = Math.Clamp(dx, -32767, 32767), sy = Math.Clamp(dy, -32767, 32767);
                Track(NativeCore.linkall_edge_move(handle, (short)sx, (short)sy));
                dx -= sx;
                dy -= sy;
            }
            // One wheel notch is 120; the other end counts in units of 6 of those.
            int wx = _wheelResidualX + Interlocked.Exchange(ref _wheelX, 0);
            int wy = _wheelResidualY + Interlocked.Exchange(ref _wheelY, 0);
            _wheelResidualX = wx % 6;
            _wheelResidualY = wy % 6;
            if (wx / 6 != 0 || wy / 6 != 0)
                Track(NativeCore.linkall_edge_scroll(handle, (short)Math.Clamp(wx / 6, -32767, 32767), (short)Math.Clamp(wy / 6, -32767, 32767)));
        }

        public void Dispose()
        {
            EndRemote("Link All is closing.", tellEngine: true);
            StopHooks();
            if (ReferenceEquals(Current, this)) Current = null;
        }

        // ── Win32 ────────────────────────────────────────────────────────────

        private const int WH_MOUSE_LL = 14, WH_KEYBOARD_LL = 13;
        private const int WM_MOUSEMOVE = 0x200, WM_LBUTTONDOWN = 0x201, WM_LBUTTONUP = 0x202,
            WM_RBUTTONDOWN = 0x204, WM_RBUTTONUP = 0x205, WM_MBUTTONDOWN = 0x207, WM_MBUTTONUP = 0x208,
            WM_MOUSEWHEEL = 0x20A, WM_XBUTTONDOWN = 0x20B, WM_XBUTTONUP = 0x20C, WM_MOUSEHWHEEL = 0x20E;
        private const uint WM_QUIT = 0x12;
        private const uint LLMHF_INJECTED = 0x1;
        private const uint LLKHF_EXTENDED = 0x1, LLKHF_INJECTED = 0x10, LLKHF_UP = 0x80;
        private const int SM_CXSCREEN = 0, SM_CYSCREEN = 1, SM_XVIRTUALSCREEN = 76, SM_YVIRTUALSCREEN = 77,
            SM_CXVIRTUALSCREEN = 78, SM_CYVIRTUALSCREEN = 79;

        [StructLayout(LayoutKind.Sequential)]
        private struct MSLLHOOKSTRUCT
        {
            public int x, y;
            public int mouseData;
            public uint flags;
            public uint time;
            public UIntPtr extra;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct KBDLLHOOKSTRUCT
        {
            public uint vkCode, scanCode, flags, time;
            public UIntPtr extra;
        }

        [StructLayout(LayoutKind.Sequential)]
        private struct MSG
        {
            public IntPtr hwnd;
            public uint message;
            public UIntPtr wParam;
            public IntPtr lParam;
            public uint time;
            public int x, y;
        }

        private static void GetVirtualScreen(out int left, out int top, out int right, out int bottom)
        {
            left = GetSystemMetrics(SM_XVIRTUALSCREEN);
            top = GetSystemMetrics(SM_YVIRTUALSCREEN);
            right = left + GetSystemMetrics(SM_CXVIRTUALSCREEN);
            bottom = top + GetSystemMetrics(SM_CYVIRTUALSCREEN);
        }

        [DllImport("user32.dll")] private static extern IntPtr SetWindowsHookExW(int idHook, IntPtr lpfn, IntPtr hMod, uint dwThreadId);
        [DllImport("user32.dll")] [return: MarshalAs(UnmanagedType.Bool)] private static extern bool UnhookWindowsHookEx(IntPtr hhk);
        [DllImport("user32.dll")] private static extern IntPtr CallNextHookEx(IntPtr hhk, int nCode, IntPtr wParam, IntPtr lParam);
        [DllImport("user32.dll")] private static extern int GetMessageW(out MSG msg, IntPtr hWnd, uint min, uint max);
        [DllImport("user32.dll")] [return: MarshalAs(UnmanagedType.Bool)] private static extern bool PostThreadMessageW(uint idThread, uint msg, UIntPtr wParam, IntPtr lParam);
        [DllImport("user32.dll")] private static extern int GetSystemMetrics(int index);
        [DllImport("user32.dll")] [return: MarshalAs(UnmanagedType.Bool)] private static extern bool SetCursorPos(int x, int y);
        [DllImport("user32.dll")] private static extern short GetAsyncKeyState(int vKey);
        [DllImport("kernel32.dll")] private static extern uint GetCurrentThreadId();
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode)] private static extern IntPtr GetModuleHandleW(string? name);
    }
}
