using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;

namespace LinkAll.WinUI
{
    public class GlobalHotKeyManager : IDisposable
    {
        public static GlobalHotKeyManager Shared { get; } = new GlobalHotKeyManager();

        public event Action? HotKeyPressed;

        private readonly Dictionary<int, Action> _callbacks = new();
        private int _currentId = 0;
        private IntPtr _hwnd = IntPtr.Zero;
        private SubclassProc? _subclassDelegate;

        public GlobalHotKeyManager()
        {
            _hwnd = CreateWindowExW(0, "Static", "LinkAllHotKeyWindow", 0, 0, 0, 0, 0, new IntPtr(-3) /* HWND_MESSAGE */, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero);
            if (_hwnd != IntPtr.Zero)
            {
                _subclassDelegate = new SubclassProc(WindowSubclassProc);
                SetWindowSubclass(_hwnd, _subclassDelegate, 1, IntPtr.Zero);
            }
        }

        public void RegisterHotKey(bool ctrl, bool shift, bool alt, bool win, string key)
        {
            if (_hwnd == IntPtr.Zero) return;
            int id = ++_currentId;
            uint fsModifiers = 0;
            if (alt) fsModifiers |= 0x0001;
            if (ctrl) fsModifiers |= 0x0002;
            if (shift) fsModifiers |= 0x0004;
            if (win) fsModifiers |= 0x0008;

            uint vk = 0;
            if (!string.IsNullOrEmpty(key) && key.Length == 1)
            {
                vk = (uint)char.ToUpperInvariant(key[0]);
            }
            else if (key?.Equals("V", StringComparison.OrdinalIgnoreCase) == true) vk = 0x56;
            else if (key?.Equals("K", StringComparison.OrdinalIgnoreCase) == true) vk = 0x4B;

            if (vk != 0)
            {
                if (RegisterHotKey(_hwnd, id, fsModifiers, vk))
                {
                    _callbacks[id] = () => HotKeyPressed?.Invoke();
                }
                else
                {
                    App.MainDispatcherQueue?.TryEnqueue(() => {
                        // Services.NotificationHelper.ShowToast("Hotkey Error", "Failed to register global hotkey. Another app might be using it.");
                    });
                }
            }
        }

        public int Register(bool ctrl, bool shift, bool alt, bool win, uint vk, Action callback)
        {
            if (_hwnd == IntPtr.Zero) return -1;
            int id = ++_currentId;
            uint fsModifiers = 0;
            if (alt) fsModifiers |= 0x0001;
            if (ctrl) fsModifiers |= 0x0002;
            if (shift) fsModifiers |= 0x0004;
            if (win) fsModifiers |= 0x0008;

            if (RegisterHotKey(_hwnd, id, fsModifiers, vk))
            {
                _callbacks[id] = callback;
                return id;
            }
            App.MainDispatcherQueue?.TryEnqueue(() => {
                // Services.NotificationHelper.ShowToast("Hotkey Error", "Failed to register global hotkey. Another app might be using it.");
            });
            return -1;
        }

        public void UnregisterHotKey()
        {
            if (_hwnd == IntPtr.Zero) return;
            foreach (var id in _callbacks.Keys)
            {
                UnregisterHotKey(_hwnd, id);
            }
            _callbacks.Clear();
        }

        private IntPtr WindowSubclassProc(IntPtr hWnd, uint uMsg, IntPtr wParam, IntPtr lParam, IntPtr uIdSubclass, IntPtr dwRefData)
        {
            if (uMsg == 0x0312) // WM_HOTKEY
            {
                try
                {
                    int id = wParam.ToInt32();
                    if (_callbacks.TryGetValue(id, out var action))
                    {
                        action?.Invoke();
                    }
                }
                catch (Exception ex) { App.HandleError(ex); }
            }
            return DefSubclassProc(hWnd, uMsg, wParam, lParam);
        }

        public void Dispose()
        {
            UnregisterHotKey();
            if (_hwnd != IntPtr.Zero)
            {
                DestroyWindow(_hwnd);
                _hwnd = IntPtr.Zero;
            }
        }

        [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
        private static extern IntPtr CreateWindowExW(
            uint dwExStyle, string lpClassName, string lpWindowName, uint dwStyle,
            int x, int y, int nWidth, int nHeight, IntPtr hWndParent,
            IntPtr hMenu, IntPtr hInstance, IntPtr lpParam);

        [DllImport("user32.dll", SetLastError = true)]
        private static extern bool RegisterHotKey(IntPtr hWnd, int id, uint fsModifiers, uint vk);

        [DllImport("user32.dll", SetLastError = true)]
        private static extern bool UnregisterHotKey(IntPtr hWnd, int id);

        [DllImport("user32.dll", SetLastError = true)]
        private static extern bool DestroyWindow(IntPtr hWnd);

        private delegate IntPtr SubclassProc(IntPtr hWnd, uint uMsg, IntPtr wParam, IntPtr lParam, IntPtr uIdSubclass, IntPtr dwRefData);

        [DllImport("comctl32.dll", SetLastError = true)]
        private static extern bool SetWindowSubclass(IntPtr hWnd, SubclassProc pfnSubclass, IntPtr uIdSubclass, IntPtr dwRefData);

        [DllImport("comctl32.dll", SetLastError = true)]
        private static extern IntPtr DefSubclassProc(IntPtr hWnd, uint uMsg, IntPtr wParam, IntPtr lParam);
    }
}
