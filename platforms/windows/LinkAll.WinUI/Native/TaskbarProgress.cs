using System;
using System.Runtime.InteropServices;
using System.Runtime.InteropServices.Marshalling;

namespace LinkAll.WinUI
{
    /// <summary>
    /// Taskbar button progress (ITaskbarList3). Uses source-generated COM
    /// interop: classic [ComImport] RCWs are not supported under Native AOT
    /// (IL3052) and would throw the first time a transfer reports progress.
    /// </summary>
    public static partial class TaskbarProgress
    {
        public enum TaskbarStates
        {
            NoProgress = 0,
            Indeterminate = 0x1,
            Normal = 0x2,
            Error = 0x4,
            Paused = 0x8
        }

        // Vtable order: ITaskbarList, then ITaskbarList2, then ITaskbarList3.
        [GeneratedComInterface]
        [Guid("ea1afb91-9e28-4b86-90e9-9e9f8a5eefaf")]
        internal partial interface ITaskbarList3
        {
            [PreserveSig] int HrInit();
            [PreserveSig] int AddTab(nint hwnd);
            [PreserveSig] int DeleteTab(nint hwnd);
            [PreserveSig] int ActivateTab(nint hwnd);
            [PreserveSig] int SetActiveAlt(nint hwnd);
            [PreserveSig] int MarkFullscreenWindow(nint hwnd, [MarshalAs(UnmanagedType.Bool)] bool fFullscreen);
            [PreserveSig] int SetProgressValue(nint hwnd, ulong ullCompleted, ulong ullTotal);
            [PreserveSig] int SetProgressState(nint hwnd, TaskbarStates state);
        }

        private static readonly Guid CLSID_TaskbarList = new("56FDF344-FD6D-11d0-958A-006097C9A090");
        private static readonly Guid IID_ITaskbarList3 = new("ea1afb91-9e28-4b86-90e9-9e9f8a5eefaf");
        private const uint CLSCTX_INPROC_SERVER = 0x1;

        [LibraryImport("ole32.dll")]
        private static partial int CoCreateInstance(in Guid rclsid, nint pUnkOuter, uint dwClsContext, in Guid riid, out nint ppv);

        private static ITaskbarList3? _taskbarInstance;

        private static ITaskbarList3 Instance()
        {
            if (_taskbarInstance != null) return _taskbarInstance;

            Marshal.ThrowExceptionForHR(CoCreateInstance(CLSID_TaskbarList, 0, CLSCTX_INPROC_SERVER, IID_ITaskbarList3, out var ptr));
            try
            {
                var instance = (ITaskbarList3)new StrategyBasedComWrappers()
                    .GetOrCreateObjectForComInstance(ptr, CreateObjectFlags.None);
                Marshal.ThrowExceptionForHR(instance.HrInit());
                _taskbarInstance = instance;
                return instance;
            }
            finally
            {
                Marshal.Release(ptr);
            }
        }

        public static void SetState(IntPtr hwnd, TaskbarStates state)
        {
            try
            {
                Instance().SetProgressState(hwnd, state);
            }
            catch (Exception ex) { App.HandleError(ex); }
        }

        public static void SetValue(IntPtr hwnd, double progress, double total)
        {
            try
            {
                Instance().SetProgressValue(hwnd, (ulong)progress, (ulong)total);
            }
            catch (Exception ex) { App.HandleError(ex); }
        }
    }
}
