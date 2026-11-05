//! Tray residency and single-instance activation for the interactive app.
//!
//! Closing the main window hides it instead of terminating the process so the
//! FlexiKey keyboard hook remains active. The tray icon restores the window.

#[cfg(windows)]
mod platform {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::{Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;

    use raw_window_handle::{HasWindowHandle as _, RawWindowHandle};
    use slint::ComponentHandle as _;
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, RECT,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, NOTIFYICONDATAW_0,
        Shell_NotifyIconW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreateIconFromResourceEx, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
        DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW, FindWindowW, GWL_EXSTYLE,
        GetCursorPos, GetMessageW, GetSystemMetrics, GetWindowLongPtrW, GetWindowRect, HICON,
        HWND_TOP, IDI_APPLICATION, LR_DEFAULTCOLOR, LoadIconW, MF_STRING, PostMessageW,
        PostQuitMessage, RegisterClassW, RegisterWindowMessageW, SM_CXSCREEN, SM_CYSCREEN, SW_HIDE,
        SW_RESTORE, SW_SHOW, SWP_NOZORDER, SWP_SHOWWINDOW, SetForegroundWindow, SetWindowLongPtrW,
        SetWindowPos, ShowWindow, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
        TranslateMessage, WM_APP, WM_CLOSE, WM_COMMAND, WM_CONTEXTMENU, WM_DESTROY, WM_LBUTTONUP,
        WM_RBUTTONUP, WNDCLASSW, WS_EX_TOOLWINDOW, WS_OVERLAPPED,
    };

    use crate::MainWindow;

    const TRAY_ID: u32 = 1;
    const TRAY_MESSAGE: u32 = WM_APP + 1;
    const SHOW_MESSAGE: u32 = WM_APP + 2;
    const MENU_OPEN: usize = 1;
    const MENU_EXIT: usize = 2;
    const WINDOW_CLASS: &str = "NeoControlTrayWindow";
    const WINDOW_TITLE: &str = "Neo Control";
    const MUTEX_NAME: &str = "Local\\NeoControlUiInstance";
    const ICON_BYTES: &[u8] = include_bytes!("../../../assets/neo_control_logo.ico");

    struct TrayContext {
        window: usize,
        main_window: usize,
        icon: usize,
        owns_icon: bool,
        ui: slint::Weak<MainWindow>,
        taskbar_created: u32,
    }

    static CONTEXT: OnceLock<Mutex<TrayContext>> = OnceLock::new();
    static VISIBLE_REQUESTED: AtomicBool = AtomicBool::new(false);

    pub fn hide_main_window(window: usize) {
        if window == 0 {
            return;
        }
        let window = window as HWND;
        unsafe {
            let mut rect = RECT::default();
            let (width, height) = if GetWindowRect(window, &mut rect) != 0 {
                (rect.right - rect.left, rect.bottom - rect.top)
            } else {
                (1180, 820)
            };
            SetWindowPos(
                window,
                ptr::null_mut(),
                -30_000,
                -30_000,
                width,
                height,
                SWP_NOZORDER,
            );
            ShowWindow(window, SW_HIDE);
        }
    }

    pub fn begin_background_hide(window: usize) {
        VISIBLE_REQUESTED.store(false, Ordering::Relaxed);
        crate::log_action("background hide loop started");
        thread::spawn(move || {
            for _ in 0..120 {
                if VISIBLE_REQUESTED.load(Ordering::Relaxed) {
                    break;
                }
                hide_main_window(window);
                thread::sleep(Duration::from_millis(50));
            }
            crate::log_action("background hide loop finished");
        });
    }

    pub fn show_main_window(window: usize) {
        VISIBLE_REQUESTED.store(true, Ordering::Relaxed);
        if window == 0 {
            return;
        }
        let window = window as HWND;
        unsafe {
            let mut rect = RECT::default();
            let (width, height) = if GetWindowRect(window, &mut rect) != 0 {
                (rect.right - rect.left, rect.bottom - rect.top)
            } else {
                (1180, 820)
            };
            let screen_width = GetSystemMetrics(SM_CXSCREEN);
            let screen_height = GetSystemMetrics(SM_CYSCREEN);
            let x = ((screen_width - width) / 2).max(0);
            let y = ((screen_height - height) / 2).max(0);
            SetWindowPos(window, HWND_TOP, x, y, width, height, SWP_SHOWWINDOW);
            ShowWindow(window, SW_RESTORE);
            ShowWindow(window, SW_SHOW);
            SetForegroundWindow(window);
        }
    }

    fn make_tool_window(window: usize) {
        if window == 0 {
            return;
        }
        let window = window as HWND;
        unsafe {
            let style = GetWindowLongPtrW(window, GWL_EXSTYLE);
            SetWindowLongPtrW(window, GWL_EXSTYLE, style | WS_EX_TOOLWINDOW as isize);
        }
    }

    pub fn main_window_handle(ui: &MainWindow) -> Option<usize> {
        let handle = ui.window().window_handle();
        if let Ok(raw) = handle.window_handle()
            && let RawWindowHandle::Win32(window) = raw.as_raw()
        {
            return Some(window.hwnd.get() as usize);
        }
        let title = wide("NEO CONTROL");
        let window = unsafe { FindWindowW(ptr::null(), title.as_ptr()) };
        (!window.is_null()).then_some(window as usize)
    }

    const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const RUN_VALUE_NAME: &str = "NeoControl";

    pub fn install_startup(executable: &std::path::Path) -> Result<(), String> {
        let executable = executable
            .to_str()
            .ok_or_else(|| "the executable path is not valid UTF-16".to_owned())?;
        if executable.contains('"') {
            return Err("the executable path contains a quote".to_owned());
        }
        crate::system_controls::registry::write_registry_string(
            RUN_KEY_PATH,
            RUN_VALUE_NAME,
            &format!("\"{executable}\" --background"),
        )
    }

    pub fn uninstall_startup() -> Result<(), String> {
        crate::system_controls::registry::delete_registry_value(RUN_KEY_PATH, RUN_VALUE_NAME)
    }

    pub fn startup_enabled() -> bool {
        crate::system_controls::registry::read_registry_string(RUN_KEY_PATH, RUN_VALUE_NAME)
            .ok()
            .flatten()
            .is_some_and(|value| !value.is_empty())
    }

    pub struct SingleInstance {
        handle: HANDLE,
    }

    impl SingleInstance {
        pub fn acquire() -> Result<Option<Self>, String> {
            let name = wide(MUTEX_NAME);
            let handle = unsafe { CreateMutexW(ptr::null(), 1, name.as_ptr()) };
            if handle.is_null() {
                return Err(last_error("CreateMutexW"));
            }
            if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                unsafe { CloseHandle(handle) };
                for _ in 0..20 {
                    if signal_existing() {
                        return Ok(None);
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                return Ok(None);
            }
            Ok(Some(Self { handle }))
        }
    }

    impl Drop for SingleInstance {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                unsafe { CloseHandle(self.handle) };
            }
        }
    }

    pub struct Tray {
        window: HWND,
    }

    impl Tray {
        pub fn start(ui: slint::Weak<MainWindow>, main_window: usize) -> Result<Self, String> {
            make_tool_window(main_window);
            let (ready_tx, ready_rx) = mpsc::channel();
            thread::spawn(move || {
                if let Err(error) = run_tray_thread(ui, main_window, ready_tx.clone()) {
                    let _ = ready_tx.send(Err(error));
                }
            });
            match ready_rx.recv_timeout(Duration::from_secs(3)) {
                Ok(Ok(window)) => {
                    crate::log_action("tray icon installed");
                    Ok(Self {
                        window: window as HWND,
                    })
                }
                Ok(Err(error)) => Err(error),
                Err(error) => Err(format!("tray start timed out: {error}")),
            }
        }

        pub fn shutdown(&self) {
            unsafe {
                PostMessageW(self.window, WM_CLOSE, 0, 0);
            }
        }
    }

    fn run_tray_thread(
        ui: slint::Weak<MainWindow>,
        main_window: usize,
        ready: mpsc::Sender<Result<usize, String>>,
    ) -> Result<(), String> {
        let class_name = wide(WINDOW_CLASS);
        let title = wide(WINDOW_TITLE);
        let icon = load_icon();
        let taskbar_name = wide("TaskbarCreated");
        let window_class = WNDCLASSW {
            style: 0,
            lpfnWndProc: Some(window_proc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: std::ptr::null_mut(),
            hIcon: icon.0,
            hCursor: ptr::null_mut(),
            hbrBackground: ptr::null_mut(),
            lpszMenuName: ptr::null(),
            lpszClassName: class_name.as_ptr(),
        };
        let (module, registered, taskbar_created) = unsafe {
            let module = GetModuleHandleW(ptr::null());
            let mut window_class = window_class;
            window_class.hInstance = module;
            (
                module,
                RegisterClassW(&window_class),
                RegisterWindowMessageW(taskbar_name.as_ptr()),
            )
        };
        if registered == 0 {
            let error = last_error("RegisterClassW");
            if !error.contains("1410") {
                return Err(error);
            }
        }
        let context = TrayContext {
            window: 0,
            main_window,
            icon: icon.0 as usize,
            owns_icon: icon.1,
            ui,
            taskbar_created,
        };
        let _ = CONTEXT.set(Mutex::new(context));
        let window = unsafe {
            CreateWindowExW(
                0,
                class_name.as_ptr(),
                title.as_ptr(),
                WS_OVERLAPPED,
                0,
                0,
                0,
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                module,
                ptr::null(),
            )
        };
        if window.is_null() {
            return Err(last_error("CreateWindowExW"));
        }
        if let Some(context) = CONTEXT.get()
            && let Ok(mut context) = context.lock()
        {
            context.window = window as usize;
        }
        if !add_icon(window) {
            unsafe { DestroyWindow(window) };
            return Err(last_error("Shell_NotifyIconW(NIM_ADD)"));
        }
        let _ = ready.send(Ok(window as usize));

        let mut message = windows_sys::Win32::UI::WindowsAndMessaging::MSG::default();
        loop {
            let result = unsafe {
                let result = GetMessageW(&mut message, ptr::null_mut(), 0, 0);
                if result > 0 {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                result
            };
            if result <= 0 {
                break;
            }
        }
        let _ = slint::invoke_from_event_loop(|| {
            let _ = slint::quit_event_loop();
        });
        Ok(())
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: usize,
        lparam: isize,
    ) -> isize {
        let Some(context) = CONTEXT.get() else {
            return default_window_proc(hwnd, message, wparam, lparam);
        };
        let is_taskbar_created = context
            .lock()
            .map(|context| context.taskbar_created == message)
            .unwrap_or(false);
        if is_taskbar_created {
            let _ = add_icon(hwnd);
            return 0;
        }
        match message {
            WM_CLOSE => {
                unsafe { DestroyWindow(hwnd) };
                0
            }
            WM_DESTROY => {
                remove_icon(hwnd);
                unsafe {
                    if let Ok(mut context) = context.lock()
                        && context.owns_icon
                        && context.icon != 0
                    {
                        DestroyIcon(context.icon as HICON);
                        context.icon = 0;
                    }
                    PostQuitMessage(0);
                }
                0
            }
            SHOW_MESSAGE => {
                show_ui(context);
                0
            }
            TRAY_MESSAGE => {
                let event = (lparam as u32) & 0xffff;
                if event == WM_LBUTTONUP {
                    show_ui(context);
                } else if event == WM_RBUTTONUP || event == WM_CONTEXTMENU {
                    show_menu(hwnd);
                }
                0
            }
            WM_COMMAND => {
                match wparam & 0xffff {
                    MENU_OPEN => show_ui(context),
                    MENU_EXIT => {
                        unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
                    }
                    _ => {}
                }
                0
            }
            _ => default_window_proc(hwnd, message, wparam, lparam),
        }
    }

    fn add_icon(hwnd: HWND) -> bool {
        let Some(context) = CONTEXT.get() else {
            return false;
        };
        let (icon, tip) = match context.lock() {
            Ok(context) => (context.icon as HICON, wide("Neo Control")),
            Err(_) => return false,
        };
        let mut data = notify_data(hwnd, icon);
        let tip_len = tip.len().min(data.szTip.len());
        data.szTip[..tip_len].copy_from_slice(&tip[..tip_len]);
        unsafe { Shell_NotifyIconW(NIM_ADD, &data) != 0 }
    }

    fn remove_icon(hwnd: HWND) {
        let data = notify_data(hwnd, ptr::null_mut());
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &data);
        }
    }

    fn notify_data(hwnd: HWND, icon: HICON) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ID,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: TRAY_MESSAGE,
            hIcon: icon,
            Anonymous: NOTIFYICONDATAW_0 { uVersion: 4 },
            ..NOTIFYICONDATAW::default()
        }
    }

    fn show_menu(hwnd: HWND) {
        let open = wide("打开 Neo Control");
        let exit = wide("退出");
        unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return;
            }
            AppendMenuW(menu, MF_STRING, MENU_OPEN, open.as_ptr());
            AppendMenuW(menu, MF_STRING, MENU_EXIT, exit.as_ptr());
            let mut point = Default::default();
            GetCursorPos(&mut point);
            SetForegroundWindow(hwnd);
            let command = TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY,
                point.x,
                point.y,
                0,
                hwnd,
                ptr::null(),
            ) as usize;
            PostMessageW(hwnd, WM_COMMAND, command, 0);
            DestroyMenu(menu);
        }
    }

    fn default_window_proc(hwnd: HWND, message: u32, wparam: usize, lparam: isize) -> isize {
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
    }

    fn show_ui(context: &Mutex<TrayContext>) {
        let (ui, main_window) = context
            .lock()
            .ok()
            .map(|context| (context.ui.clone(), context.main_window as HWND))
            .unwrap_or_default();
        if main_window.is_null() {
            return;
        }
        show_main_window(main_window as usize);
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = ui.upgrade() {
                ui.window().request_redraw();
            }
        });
    }

    fn signal_existing() -> bool {
        let class_name = wide(WINDOW_CLASS);
        let window = unsafe { FindWindowW(class_name.as_ptr(), ptr::null()) };
        if window.is_null() {
            return false;
        }
        unsafe { PostMessageW(window, SHOW_MESSAGE, 0, 0) != 0 }
    }

    fn load_icon() -> (HICON, bool) {
        if let Some(icon) = icon_from_ico(ICON_BYTES) {
            crate::log_action("tray icon: embedded logo");
            return (icon, true);
        }
        crate::log_action("tray icon: embedded logo failed, using fallback");
        let icon = unsafe { LoadIconW(ptr::null_mut(), IDI_APPLICATION) };
        (icon, false)
    }

    fn icon_from_ico(bytes: &[u8]) -> Option<HICON> {
        if bytes.len() < 6 {
            return None;
        }
        let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
        let mut best = None;
        for index in 0..count {
            let offset = 6 + index * 16;
            if offset + 16 > bytes.len() {
                return None;
            }
            let width = bytes[offset] as u32;
            let height = bytes[offset + 1] as u32;
            let size = u32::from_le_bytes(bytes[offset + 8..offset + 12].try_into().ok()?) as usize;
            let data_offset =
                u32::from_le_bytes(bytes[offset + 12..offset + 16].try_into().ok()?) as usize;
            if data_offset.checked_add(size)? > bytes.len() {
                return None;
            }
            let score = width.abs_diff(32) + height.abs_diff(32);
            if best.is_none_or(|(_, best_score)| score < best_score) {
                best = Some(((width, height, data_offset, size), score));
            }
        }
        let ((width, height, offset, size), _) = best?;
        let icon = unsafe {
            CreateIconFromResourceEx(
                bytes[offset..offset + size].as_ptr(),
                size as u32,
                1,
                0x0003_0000,
                width as i32,
                height as i32,
                LR_DEFAULTCOLOR,
            )
        };
        (!icon.is_null()).then_some(icon)
    }

    fn wide(value: &str) -> Vec<u16> {
        OsStr::new(value)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn last_error(operation: &str) -> String {
        let code = unsafe { GetLastError() };
        format!(
            "{operation} failed: {} (code {code})",
            std::io::Error::from_raw_os_error(code as i32)
        )
    }
}

#[cfg(windows)]
pub use platform::{
    SingleInstance, Tray, begin_background_hide, hide_main_window, install_startup,
    main_window_handle, startup_enabled, uninstall_startup,
};

#[cfg(not(windows))]
mod platform {
    pub struct SingleInstance;
    pub struct Tray;

    pub fn install_startup(_executable: &std::path::Path) -> Result<(), String> {
        Err("startup registration is only available on Windows".to_owned())
    }

    pub fn uninstall_startup() -> Result<(), String> {
        Err("startup registration is only available on Windows".to_owned())
    }

    pub fn startup_enabled() -> bool {
        false
    }

    pub fn main_window_handle(_ui: &crate::MainWindow) -> Option<usize> {
        None
    }

    pub fn hide_main_window(_window: usize) {}

    pub fn begin_background_hide(_window: usize) {}

    impl SingleInstance {
        pub fn acquire() -> Result<Option<Self>, String> {
            Ok(Some(Self))
        }
    }

    impl Tray {
        pub fn start(
            _ui: slint::Weak<crate::MainWindow>,
            _main_window: usize,
        ) -> Result<Self, String> {
            Err("tray support is only available on Windows".to_owned())
        }

        pub fn shutdown(&self) {}
    }
}

#[cfg(not(windows))]
pub use platform::{
    SingleInstance, Tray, begin_background_hide, hide_main_window, install_startup,
    main_window_handle, startup_enabled, uninstall_startup,
};
