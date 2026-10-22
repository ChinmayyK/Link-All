using System;
using System.Runtime.InteropServices;
using Microsoft.UI.Xaml;
using Microsoft.UI.Dispatching;

namespace LinkAll.WinUI.Services
{
    public class GlobalDragMonitor : IDisposable
    {
        [DllImport("user32.dll")]
        [return: MarshalAs(UnmanagedType.Bool)]
        private static extern bool GetCursorPos(out POINT lpPoint);

        [DllImport("user32.dll")]
        private static extern short GetAsyncKeyState(int vKey);

        [DllImport("user32.dll")]
        private static extern int GetSystemMetrics(int nIndex);

        [DllImport("user32.dll")]
        private static extern IntPtr WindowFromPoint(POINT point);

        [DllImport("user32.dll", CharSet = CharSet.Unicode)]
        private static extern int GetClassNameW(IntPtr hWnd, [Out] ushort[] buffer, int maxCount);

        private const int SM_CXSCREEN = 0;
        private const int SM_CYSCREEN = 1;
        private const int SM_CXDRAG = 68;

        [StructLayout(LayoutKind.Sequential)]
        public struct POINT
        {
            public int X;
            public int Y;
        }

        private readonly DispatcherTimer _timer;
        // The UI thread's queue, taken here because the main window does not
        // exist when Link All starts in the background.
        private readonly DispatcherQueue _queue = DispatcherQueue.GetForCurrentThread();
        private bool _wasLeftButtonDown = false;
        private UI.EdgeDropWindow? _dropZoneWindow;
        private readonly ClipboardManager _clipboardManager;
        private readonly int _edgeThreshold = 20;
        // Edge ticks (100 ms each) the cursor must stay put before the zone shows, so
        // sweeping past the edge doesn't open it.
        private const int EdgeDwellTicks = 4;
        private POINT _press;
        private bool _pressedOnFile;
        private bool _moved;
        private int _edgeTicks;

        public GlobalDragMonitor(ClipboardManager clipboardManager)
        {
            _clipboardManager = clipboardManager;
            _timer = new DispatcherTimer { Interval = TimeSpan.FromMilliseconds(100) };
            _timer.Tick += OnTick;
            _timer.Start();
        }

        private void OnTick(object? sender, object e)
        {
            try
            {
                // VK_LBUTTON is 0x01
                bool isLeftButtonDown = (GetAsyncKeyState(0x01) & 0x8000) != 0;

                if (isLeftButtonDown && !_wasLeftButtonDown && GetCursorPos(out POINT down))
                {
                    // A new press: remember where, and whether it landed on a file icon.
                    _press = down;
                    _pressedOnFile = IsFileItem(down);
                    _moved = false;
                    _edgeTicks = 0;
                }
                else if (isLeftButtonDown && _wasLeftButtonDown)
                {
                    // Only a drag that started on a file in Explorer or on the desktop
                    // counts: window moves, text selections and in-app drags don't.
                    if (_pressedOnFile && GetCursorPos(out POINT p))
                    {
                        int slop = GetSystemMetrics(SM_CXDRAG);
                        if (Math.Abs(p.X - _press.X) > slop || Math.Abs(p.Y - _press.Y) > slop) _moved = true;

                        double screenWidth = GetSystemMetrics(SM_CXSCREEN);
                        if (_moved && p.X >= screenWidth - _edgeThreshold)
                        {
                            if (++_edgeTicks >= EdgeDwellTicks && _dropZoneWindow == null)
                            {
                                ShowDropZone();
                            }
                        }
                        else
                        {
                            _edgeTicks = 0;
                        }
                    }
                }
                else if (!isLeftButtonDown && _wasLeftButtonDown)
                {
                    // User released the drag. Delay close slightly to allow drop event to process
                    System.Threading.Tasks.Task.Delay(500).ContinueWith(_ =>
                    {
                        _queue.TryEnqueue(HideDropZone);
                    });
                }

                _wasLeftButtonDown = isLeftButtonDown;
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        // The list views that hold file icons in Explorer windows and on the desktop.
        // The window under the press is the icon area itself, not a title bar or frame.
        private static bool IsFileItem(POINT p)
        {
            var hwnd = WindowFromPoint(p);
            if (hwnd == IntPtr.Zero) return false;
            var buf = new ushort[64];
            int n = GetClassNameW(hwnd, buf, buf.Length);
            if (n <= 0) return false;
            var chars = new char[n];
            for (int i = 0; i < n; i++) chars[i] = (char)buf[i];
            var cls = new string(chars);
            return cls is "DirectUIHWND" or "SysListView32" or "SHELLDLL_DefView";
        }

        private void ShowDropZone()
        {
            try
            {
                if (_dropZoneWindow != null) return;
                
                _dropZoneWindow = new UI.EdgeDropWindow();
                
                var hwnd = WinRT.Interop.WindowNative.GetWindowHandle(_dropZoneWindow);
                var windowId = Microsoft.UI.Win32Interop.GetWindowIdFromWindow(hwnd);
                var appWindow = Microsoft.UI.Windowing.AppWindow.GetFromWindowId(windowId);
                
                int screenW = GetSystemMetrics(SM_CXSCREEN);
                int screenH = GetSystemMetrics(SM_CYSCREEN);
                
                int w = 20;
                int h = 200;
                
                appWindow.MoveAndResize(new Windows.Graphics.RectInt32(screenW - w, (screenH - h) / 2, w, h));
                
                // Shown without taking focus: the user is mid-drag in another
                // app, and activating this window could end that drag.
                appWindow.Show(false);
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        private void HideDropZone()
        {
            try
            {
                if (_dropZoneWindow != null)
                {
                    _dropZoneWindow.Close();
                    _dropZoneWindow = null;
                }
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        public void Dispose()
        {
            _timer.Stop();
            HideDropZone();
        }
    }
}



