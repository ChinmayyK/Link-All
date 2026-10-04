use std::cell::RefCell;
use std::ffi::c_void;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};

use windows_sys::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_PIPE_CONNECTED, GENERIC_WRITE, HWND, INVALID_HANDLE_VALUE,
    LPARAM, LRESULT, POINT, WPARAM,
};
use windows_sys::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, OPEN_EXISTING, PIPE_ACCESS_INBOUND,
};
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, WaitNamedPipeW, PIPE_READMODE_BYTE,
    PIPE_TYPE_BYTE, PIPE_WAIT,
};
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};
use windows_sys::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows_sys::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DispatchMessageW,
    EnumWindows, FindWindowW, GetCursorPos, GetMessageW, GetSystemMetrics, GetWindow,
    GetWindowThreadProcessId, IsWindowVisible, LoadIconW, LoadImageW, PostMessageW,
    PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SetForegroundWindow,
    SetMenuDefaultItem, ShowWindow, TrackPopupMenu, TranslateMessage, GW_OWNER, HICON,
    IDI_APPLICATION, IMAGE_ICON, LR_LOADFROMFILE, MF_SEPARATOR, MF_STRING, MSG, SM_CXSMICON,
    SM_CYSMICON, SW_RESTORE, SW_SHOWNORMAL, TPM_BOTTOMALIGN, TPM_NONOTIFY, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, WM_APP, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP,
    WM_SETTINGCHANGE, WNDCLASSEXW, WS_OVERLAPPED,
};

const TRAY_PIPE: &str = r"\\.\pipe\Deskdrop_Tray_Pipe";
const MAIN_PIPE: &str = r"\\.\pipe\Deskdrop_Main_Commands";
const MAIN_EXE: &str = "Deskdrop.exe";

/// Shell_NotifyIcon callback for mouse events on the icon.
const WM_TRAY: u32 = WM_APP + 1;
/// A line read from the tray pipe; lParam owns a `Box<String>`.
const WM_PIPE_LINE: u32 = WM_APP + 2;

const ID_OPEN: usize = 1;
const ID_QUICK_ACCESS: usize = 2;
const ID_SETTINGS: usize = 3;
const ID_RESCAN: usize = 4;
const ID_QUIT: usize = 5;

struct Tray {
    nid: NOTIFYICONDATAW,
    /// Broadcast when explorer.exe (re)creates the taskbar, which wipes every
    /// tray icon: the documented signal to add ours again.
    taskbar_created: u32,
}

thread_local! {
    // Only touched on the UI thread (window procedure and `run`).
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

pub fn run() {
    log("=== deskdrop-tray starting ===");
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let instance = GetModuleHandleW(null());
        let class = wide("DeskdropTrayWindow");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: instance,
            lpszClassName: class.as_ptr(),
            ..std::mem::zeroed()
        };
        RegisterClassExW(&wc);

        // A hidden top-level window, not a message-only one: message-only
        // windows don't receive the TaskbarCreated broadcast.
        let title = wide("Link All Tray");
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            null_mut(),
            null_mut(),
            instance,
            null(),
        );
        if hwnd.is_null() {
            log(&format!("CreateWindowExW failed: {}", GetLastError()));
            return;
        }

        let mut nid: NOTIFYICONDATAW = std::mem::zeroed();
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = 1;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_TRAY;
        nid.hIcon = load_icon();
        copy_wide(&mut nid.szTip, "Link All");
        Shell_NotifyIconW(NIM_ADD, &nid);

        let taskbar_created = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
        TRAY.with(|t| {
            *t.borrow_mut() = Some(Tray {
                nid,
                taskbar_created,
            })
        });
        balloon("Link All is running in your system tray.");

        let hwnd_addr = hwnd as usize;
        std::thread::spawn(move || pipe_server(hwnd_addr));

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        TRAY.with(|t| {
            if let Some(tray) = t.borrow().as_ref() {
                Shell_NotifyIconW(NIM_DELETE, &tray.nid);
            }
        });
    }
    log("=== deskdrop-tray exiting ===");
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            match lparam as u32 {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => open_main_window(),
                WM_RBUTTONUP => show_menu(hwnd),
                _ => {}
            }
            0
        }
        WM_PIPE_LINE => {
            let line = *Box::from_raw(lparam as *mut String);
            handle_pipe_line(&line);
            0
        }
        // The user switched Windows between light and dark: swap the icon.
        WM_SETTINGCHANGE => {
            if lparam != 0 && wide_ptr_eq(lparam as *const u16, "ImmersiveColorSet") {
                TRAY.with(|t| {
                    if let Some(tray) = t.borrow_mut().as_mut() {
                        tray.nid.uFlags = NIF_ICON;
                        tray.nid.hIcon = load_icon();
                        Shell_NotifyIconW(NIM_MODIFY, &tray.nid);
                    }
                });
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        _ => {
            let taskbar_created = TRAY.with(|t| t.borrow().as_ref().map(|t| t.taskbar_created));
            if taskbar_created == Some(msg) && msg != 0 {
                log("TaskbarCreated received (explorer restarted) - re-adding tray icon");
                TRAY.with(|t| {
                    if let Some(tray) = t.borrow_mut().as_mut() {
                        tray.nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
                        tray.nid.hIcon = load_icon();
                        Shell_NotifyIconW(NIM_ADD, &tray.nid);
                    }
                });
                return 0;
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
    }
}

fn handle_pipe_line(line: &str) {
    if line == "QUIT" {
        unsafe { PostQuitMessage(0) };
    } else if let Some(tip) = line.strip_prefix("TIP:") {
        TRAY.with(|t| {
            if let Some(tray) = t.borrow_mut().as_mut() {
                tray.nid.uFlags = NIF_TIP;
                copy_wide(&mut tray.nid.szTip, tip);
                unsafe { Shell_NotifyIconW(NIM_MODIFY, &tray.nid) };
            }
        });
    } else if let Some(text) = line.strip_prefix("NOTIFY:") {
        balloon(text);
    }
}

fn balloon(text: &str) {
    TRAY.with(|t| {
        if let Some(tray) = t.borrow_mut().as_mut() {
            tray.nid.uFlags = NIF_INFO;
            tray.nid.dwInfoFlags = NIIF_INFO;
            copy_wide(&mut tray.nid.szInfoTitle, "Link All");
            copy_wide(&mut tray.nid.szInfo, text);
            unsafe { Shell_NotifyIconW(NIM_MODIFY, &tray.nid) };
        }
    });
}

unsafe fn show_menu(hwnd: HWND) {
    let menu = CreatePopupMenu();
    if menu.is_null() {
        return;
    }
    let items: [(usize, &str); 4] = [
        (ID_OPEN, "Open Link All"),
        (ID_QUICK_ACCESS, "Quick Access"),
        (ID_SETTINGS, "Settings..."),
        (ID_RESCAN, "Rescan Network"),
    ];
    for (id, label) in items {
        AppendMenuW(menu, MF_STRING, id, wide(label).as_ptr());
    }
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(menu, MF_STRING, ID_QUIT, wide("Quit Link All").as_ptr());
    SetMenuDefaultItem(menu, ID_OPEN as u32, 0);

    // Without the foreground call the menu doesn't close when clicking
    // elsewhere; the WM_NULL afterwards is the documented companion fix.
    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd);
    let cmd = TrackPopupMenu(
        menu,
        TPM_RIGHTBUTTON | TPM_BOTTOMALIGN | TPM_RETURNCMD | TPM_NONOTIFY,
        pt.x,
        pt.y,
        0,
        hwnd,
        null(),
    ) as usize;
    PostMessageW(hwnd, WM_NULL, 0, 0);
    DestroyMenu(menu);

    match cmd {
        ID_OPEN => open_main_window(),
        ID_QUICK_ACCESS => send_to_main("QUICKACCESS"),
        ID_SETTINGS => send_to_main("SETTINGS"),
        ID_RESCAN => send_to_main("RESCAN"),
        ID_QUIT => {
            kill_processes(MAIN_EXE);
            PostQuitMessage(0);
        }
        _ => {}
    }
}

/// Serves `Deskdrop_Tray_Pipe` forever, one line per client connection, and
/// hands each line to the UI thread. TrayService's watchdog also connects and
/// closes without writing; that reads as an empty line and is ignored.
fn pipe_server(hwnd_addr: usize) {
    let name = wide(TRAY_PIPE);
    loop {
        let pipe = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                PIPE_ACCESS_INBOUND,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                1,
                0,
                4096,
                0,
                null(),
            )
        };
        if pipe == INVALID_HANDLE_VALUE {
            log(&format!("CreateNamedPipeW failed: {}", unsafe {
                GetLastError()
            }));
            std::thread::sleep(std::time::Duration::from_secs(1));
            continue;
        }
        loop {
            let connected = unsafe { ConnectNamedPipe(pipe, null_mut()) } != 0
                || unsafe { GetLastError() } == ERROR_PIPE_CONNECTED;
            if !connected {
                break;
            }
            let line = read_line(pipe);
            unsafe { DisconnectNamedPipe(pipe) };
            if !line.is_empty() {
                let boxed = Box::into_raw(Box::new(line));
                let posted =
                    unsafe { PostMessageW(hwnd_addr as HWND, WM_PIPE_LINE, 0, boxed as LPARAM) }
                        != 0;
                if !posted {
                    drop(unsafe { Box::from_raw(boxed) });
                }
            }
        }
        unsafe { CloseHandle(pipe) };
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

/// Reads until the client closes its end or a newline arrives.
fn read_line(pipe: *mut c_void) -> String {
    let mut data = Vec::new();
    let mut buf = [0u8; 1024];
    loop {
        let mut read = 0u32;
        let ok = unsafe {
            ReadFile(
                pipe,
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut read,
                null_mut(),
            )
        } != 0;
        if !ok || read == 0 {
            break;
        }
        data.extend_from_slice(&buf[..read as usize]);
        if data.contains(&b'\n') || data.len() > 16 * 1024 {
            break;
        }
    }
    // .NET's StreamWriter writes UTF-8 with a BOM by default.
    let text = String::from_utf8_lossy(&data);
    let text = text.trim_start_matches('\u{feff}');
    text.lines().next().unwrap_or("").trim_end().to_string()
}

/// Sends one command line to the main app; if it isn't listening (not
/// running yet), just open it, as the old helper did.
fn send_to_main(command: &str) {
    let name = wide(MAIN_PIPE);
    let sent = unsafe {
        WaitNamedPipeW(name.as_ptr(), 300);
        let h = CreateFileW(
            name.as_ptr(),
            GENERIC_WRITE,
            0,
            null(),
            OPEN_EXISTING,
            0,
            null_mut(),
        );
        if h == INVALID_HANDLE_VALUE {
            false
        } else {
            let bytes = format!("{command}\r\n");
            let mut written = 0u32;
            let ok = WriteFile(
                h,
                bytes.as_ptr(),
                bytes.len() as u32,
                &mut written,
                null_mut(),
            ) != 0;
            CloseHandle(h);
            ok
        }
    };
    if !sent {
        open_main_window();
    }
}

/// Brings the main window forward, or starts the app when it has none.
fn open_main_window() {
    unsafe {
        let mut search = WindowSearch {
            pids: process_ids(MAIN_EXE),
            found: null_mut(),
        };
        if !search.pids.is_empty() {
            EnumWindows(
                Some(find_main_window),
                &mut search as *mut WindowSearch as LPARAM,
            );
        }
        let mut found = search.found;
        if found.is_null() && !search.pids.is_empty() {
            found = FindWindowW(null(), wide("Link All").as_ptr());
            if found.is_null() {
                found = FindWindowW(null(), wide("DeskDrop Dashboard").as_ptr());
            }
        }
        if !found.is_null() {
            ShowWindow(found, SW_RESTORE);
            SetForegroundWindow(found);
            return;
        }
    }

    let Some(exe) = main_exe_path() else {
        log("OpenDeskdropWindow: Deskdrop.exe not found");
        return;
    };
    let path = wide(&exe.to_string_lossy());
    let verb = wide("open");
    unsafe {
        ShellExecuteW(
            null_mut(),
            verb.as_ptr(),
            path.as_ptr(),
            null(),
            null(),
            SW_SHOWNORMAL,
        );
    }
}

struct WindowSearch {
    pids: Vec<u32>,
    found: HWND,
}

/// EnumWindows callback: the first visible, unowned top-level window of a
/// Deskdrop.exe process (what .NET calls a process's MainWindowHandle).
unsafe extern "system" fn find_main_window(hwnd: HWND, lparam: LPARAM) -> windows_sys::core::BOOL {
    if IsWindowVisible(hwnd) == 0 || !GetWindow(hwnd, GW_OWNER).is_null() {
        return 1;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    let search = &mut *(lparam as *mut WindowSearch);
    if search.pids.contains(&pid) {
        search.found = hwnd;
        return 0;
    }
    1
}

fn main_exe_path() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    [dir.join("..").join(MAIN_EXE), dir.join(MAIN_EXE)]
        .into_iter()
        .find(|p| p.is_file())
        .and_then(|p| p.canonicalize().ok())
        .map(strip_verbatim)
}

/// `canonicalize` returns `\\?\C:\...`, which ShellExecute handles poorly.
fn strip_verbatim(p: PathBuf) -> PathBuf {
    match p.to_str().and_then(|s| s.strip_prefix(r"\\?\")) {
        Some(rest) => PathBuf::from(rest),
        None => p,
    }
}

fn process_ids(exe_name: &str) -> Vec<u32> {
    let mut ids = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return ids;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut ok = Process32FirstW(snap, &mut entry) != 0;
        while ok {
            let len = entry
                .szExeFile
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(entry.szExeFile.len());
            let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
            if name.eq_ignore_ascii_case(exe_name) {
                ids.push(entry.th32ProcessID);
            }
            ok = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
    }
    ids
}

fn kill_processes(exe_name: &str) {
    for pid in process_ids(exe_name) {
        unsafe {
            let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
            if !h.is_null() {
                TerminateProcess(h, 0);
                CloseHandle(h);
            }
        }
    }
}

/// Whether the taskbar (and so the tray) is light. Windows keeps this apart
/// from the apps' theme. Assumes light when the setting can't be read.
fn taskbar_is_light() -> bool {
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let name = wide("SystemUsesLightTheme");
    let mut data: u32 = 1;
    let mut size: u32 = std::mem::size_of::<u32>() as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            &mut data as *mut u32 as *mut _,
            &mut size,
        )
    };
    rc != 0 || data != 0
}

/// True when the NUL-terminated UTF-16 string at `p` equals `s`.
unsafe fn wide_ptr_eq(mut p: *const u16, s: &str) -> bool {
    for c in s.encode_utf16() {
        if *p != c {
            return false;
        }
        p = p.add(1);
    }
    *p == 0
}

/// The tray icon from the app's Assets (shipped next to Deskdrop.exe), in the
/// logo that suits the taskbar's theme, at the small-icon size for the
/// current DPI, else the stock application icon.
fn load_icon() -> HICON {
    let Ok(exe) = std::env::current_exe() else {
        return unsafe { LoadIconW(null_mut(), IDI_APPLICATION) };
    };
    let dir = exe.parent().unwrap_or(Path::new("."));
    let themed = if taskbar_is_light() {
        "TrayIcon.ico"
    } else {
        "TrayIconDark.ico"
    };
    let candidates = [
        dir.join("Assets").join(themed),
        dir.join("..").join("Assets").join(themed),
        dir.join("Assets").join("TrayIcon.ico"),
        dir.join("..").join("Assets").join("TrayIcon.ico"),
        dir.join("..").join("Assets").join("AppIcon.ico"),
    ];
    let (cx, cy) = unsafe { (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON)) };
    for path in candidates.iter().filter(|p| p.is_file()) {
        let w = wide(&path.to_string_lossy());
        let icon =
            unsafe { LoadImageW(null_mut(), w.as_ptr(), IMAGE_ICON, cx, cy, LR_LOADFROMFILE) };
        if !icon.is_null() {
            return icon as HICON;
        }
        log(&format!("Failed to load icon {}", path.display()));
    }
    unsafe { LoadIconW(null_mut(), IDI_APPLICATION) }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Copies `s` into a fixed, NUL-terminated UTF-16 field, truncating to fit.
fn copy_wide(dst: &mut [u16], s: &str) {
    let max = dst.len() - 1;
    let mut n = 0;
    for (slot, c) in dst.iter_mut().take(max).zip(s.encode_utf16()) {
        *slot = c;
        n += 1;
    }
    dst[n] = 0;
}

/// Appends to %LOCALAPPDATA%\Deskdrop\tray_debug.txt, like the old helper.
fn log(msg: &str) {
    let Some(base) = std::env::var_os("LOCALAPPDATA") else {
        return;
    };
    let dir = PathBuf::from(base).join("Deskdrop");
    let _ = std::fs::create_dir_all(&dir);
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("tray_debug.txt"))
    {
        let _ = writeln!(f, "[{secs}] {msg}");
    }
}
