using Microsoft.UI.Dispatching;
using System;
using System.Runtime.InteropServices;
using System.Text;

namespace LinkAll.WinUI
{
    internal static class NativeCore
    {
        private const string DLL = "linkall_core";

        // Event codes (must match Rust CR_EVENT_* constants)
        public const int PB_EVENT_NONE = 0;
        public const int PB_EVENT_CLIPBOARD_TEXT = 1;
        public const int PB_EVENT_CLIPBOARD_IMAGE = 2;
        public const int PB_EVENT_CLIPBOARD_FILE = 3;
        public const int PB_EVENT_PAIRING_REQUESTED = 4; // TOFU prompt
        public const int PB_EVENT_PEER_CONNECTED = 5;
        public const int PB_EVENT_PEER_DISCONNECTED = 6;
        public const int PB_EVENT_WARNING = 7;
        public const int PB_EVENT_CLIPBOARD_SYNCED = 8;
        public const int PB_EVENT_CLIPBOARD_AVAILABLE = 11; // timeline-first
        public const int PB_EVENT_FILE_TRANSFER_INCOMING = 12;
        public const int PB_EVENT_FILE_TRANSFER_PROGRESS = 13;
        public const int PB_EVENT_FILE_TRANSFER_COMPLETE = 14;
        public const int PB_EVENT_FILE_TRANSFER_FAILED = 15;
        public const int PB_EVENT_ACTIVITY_UPDATED = 16;
        public const int PB_EVENT_CALL_STATE_CHANGED = 17;
        public const int PB_EVENT_CALL_ACTION = 18;
        public const int PB_EVENT_BATTERY_STATE_CHANGED = 19;
        public const int PB_EVENT_FILE_TRANSFER_PAUSED = 20;
        public const int PB_EVENT_FILE_TRANSFER_RESUMED = 21;
        public const int PB_EVENT_SYSTEM_HEALTH_UPDATED = 26;
        public const int PB_EVENT_PEER_DISCOVERED = 27;
        public const int PB_EVENT_NETWORK_STATE_CHANGED = 28;
        public const int PB_EVENT_OUTGOING_PAIRING_WAITING = 29;
        public const int PB_EVENT_REMOTE_FILES_QUERY = 30;
        public const int PB_EVENT_REMOTE_THUMBNAIL_REQUEST = 31;
        public const int PB_EVENT_REMOTE_FILE_PULL_REQUEST = 32;
        public const int PB_EVENT_REMOTE_FILES_RESPONSE = 33;
        public const int PB_EVENT_REMOTE_THUMBNAIL_RESPONSE = 34;
        // These two were missing entirely (a real version-skew gap against
        // linkall-core/src/ffi.rs's PB_EVENT_* constants) - kind=35 fired
        // constantly and silently during any remote-files/speed-test
        // activity with no matching case in DrainEvents' switch, logged and
        // discarded every time.
        public const int PB_EVENT_SPEED_TEST_PROGRESS = 35;
        public const int PB_EVENT_SPEED_TEST_COMPLETE = 36;
        public const int PB_EVENT_REMOTE_FILE_ACTION_REQUEST = 37;
        public const int PB_EVENT_OPEN_URL_ON_DEVICE_REQUESTED = 38;
        public const int PB_EVENT_OPEN_URL_ON_DEVICE_ACK = 39;
        public const int PB_EVENT_NOTIFICATION_RECEIVED = 40;
        public const int PB_EVENT_FOLDER_TRANSFER_COMPLETE = 41;

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr linkall_start(
            [MarshalAs(UnmanagedType.LPUTF8Str)] string? deviceName, ushort port);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern void linkall_stop(IntPtr handle);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern int linkall_push_text(
            IntPtr handle, [MarshalAs(UnmanagedType.LPUTF8Str)] string text);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern int linkall_push_image(
            IntPtr handle, [MarshalAs(UnmanagedType.LPUTF8Str)] string mimeType,
            byte[] data, UIntPtr len);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern int linkall_push_image_to(
            IntPtr handle, [MarshalAs(UnmanagedType.LPUTF8Str)] string targetDeviceId,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string mimeType,
            byte[] data, UIntPtr len);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern int linkall_push_file(
            IntPtr handle, [MarshalAs(UnmanagedType.LPUTF8Str)] string name,
            byte[] data, UIntPtr len);


        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern int linkall_send_file_path(
            IntPtr handle,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string? targetDevice,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string path,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string fileName,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string mimeType);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern int linkall_send_call_action(
            IntPtr handle,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string action,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string targetDevice);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr linkall_poll_event(IntPtr handle);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern int linkall_event_type(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr linkall_event_text(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr linkall_event_device_name(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr linkall_event_fingerprint(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr linkall_event_device_id(IntPtr ev);

        /// Respond to a TOFU prompt. trust=1 to accept, trust=0 to reject.
        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern int linkall_trust_peer(
            IntPtr handle, [MarshalAs(UnmanagedType.LPUTF8Str)] string deviceName, int trust);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern void linkall_free_event(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern int linkall_accept_file_transfer(IntPtr handle, [MarshalAs(UnmanagedType.LPUTF8Str)] string transferIdHex);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr linkall_event_transfer_id(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr linkall_event_transfer_file_name(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr linkall_event_transfer_dest_path(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern int linkall_event_transfer_percent(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern long linkall_event_transfer_total_bytes(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern long linkall_event_transfer_bytes_received(IntPtr ev);

        // Remote Explorer FFI Functions
        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern int linkall_send_remote_files_query(
            IntPtr handle,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string targetDeviceId,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string requestId,
            int summaryOnly,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string? category,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string? source,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string? searchQuery,
            uint offset,
            uint limit);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern int linkall_send_remote_thumbnail_request(
            IntPtr handle,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string targetDeviceId,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string requestId,
            ulong fileId,
            uint sizePx);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern int linkall_send_remote_file_pull_request(
            IntPtr handle,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string targetDeviceId,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string requestId,
            ulong fileId);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern int linkall_send_remote_files_response(
            IntPtr handle,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string requestId,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string targetDeviceId,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string? summaryJson,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string? filesJson,
            uint totalMatching,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string? errorStr);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr linkall_event_remote_request_id(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr linkall_event_remote_summary_json(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr linkall_event_remote_files_json(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern uint linkall_event_remote_total_matching(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern ulong linkall_event_remote_file_id(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr linkall_event_remote_thumbnail_data(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern UIntPtr linkall_event_remote_thumbnail_len(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr linkall_event_remote_error(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern int linkall_event_open_url_ack_success(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr linkall_event_call_number(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr linkall_event_call_contact_name(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr linkall_event_notification_title(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern int linkall_event_folder_file_count(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern int linkall_event_folder_failed_count(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern int linkall_event_transfer_in_folder(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr linkall_event_image_data(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern UIntPtr linkall_event_image_len(IntPtr ev);

        [DllImport(DLL, CallingConvention = CallingConvention.Cdecl)]
        public static extern IntPtr linkall_event_image_mime(IntPtr ev);

        public const uint ES_CONTINUOUS = 0x80000000;
        public const uint ES_SYSTEM_REQUIRED = 0x00000001;

        [DllImport("kernel32.dll", CharSet = CharSet.Auto, SetLastError = true)]
        public static extern uint SetThreadExecutionState(uint esFlags);

        public static string? PtrToUtf8String(IntPtr ptr)
        {
            if (ptr == IntPtr.Zero) return null;
            try
            {
                int len = 0;
                while (Marshal.ReadByte(ptr, len) != 0) len++;
                if (len == 0) return string.Empty;
                var buf = new byte[len];
                Marshal.Copy(ptr, buf, 0, len);
                return Encoding.UTF8.GetString(buf);
            }
            catch { return null; }
        }
    }
}









