#![windows_subsystem = "windows"]

use std::mem;
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::*;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

extern "system" {
    fn GetUserDefaultUILanguage() -> u16;
}

const WM_USER_TRAYICON: u32 = WM_USER + 1;
const ID_TRAY_ICON: u32 = 1;
const TIMER_ID: usize = 999;

const IDM_OFF: usize = 1001;
const IDM_FOREVER: usize = 1002;
const IDM_1H: usize = 1003;
const IDM_3H: usize = 1004;
const IDM_AUTOSTART: usize = 1005;
const IDM_EXIT: usize = 1006;

#[derive(PartialEq)]
enum Mode {
    Off,
    Forever,
    Timer1H,
    Timer3H,
}

struct AppState {
    mode: Mode,
    hicon_awake: HICON,
    hicon_sleep: HICON,
    lang_tr: bool,
}

static mut STATE: AppState = AppState {
    mode: Mode::Forever,
    hicon_awake: HICON(std::ptr::null_mut()),
    hicon_sleep: HICON(std::ptr::null_mut()),
    lang_tr: false,
};

fn set_string<const N: usize>(arr: &mut [u16; N], s: &str) {
    arr.fill(0);
    for (i, c) in s.encode_utf16().take(N - 1).enumerate() {
        arr[i] = c;
    }
}

fn main() -> Result<()> {
    unsafe {
        STATE.lang_tr = (GetUserDefaultUILanguage() & 0x3FF) == 0x1F;
        let instance = GetModuleHandleW(None)?;

        STATE.hicon_awake = LoadIconW(instance, PCWSTR(1 as _)).map(|h| HICON(h.0)).unwrap_or_default();
        STATE.hicon_sleep = LoadIconW(instance, PCWSTR(2 as _)).map(|h| HICON(h.0)).unwrap_or_default();

        let class_name = w!("AlwaysOnClass");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };

        let _ = RegisterClassW(&wc);

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("AlwaysOn"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            None,
            None,
            instance,
            None,
        )?;

        let mut nid: NOTIFYICONDATAW = mem::zeroed();
        nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = ID_TRAY_ICON;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_INFO;
        nid.uCallbackMessage = WM_USER_TRAYICON;
        nid.hIcon = STATE.hicon_awake;
        
        set_string(&mut nid.szTip, if STATE.lang_tr { "AlwaysOn: Aktif" } else { "AlwaysOn: Active" });
        
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);

        SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
    }
    Ok(())
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_USER_TRAYICON => {
            let lparam_lo = (lparam.0 & 0xFFFF) as u32;
            if lparam_lo == WM_LBUTTONUP {
                // Toggle between Off and Forever on click
                if STATE.mode == Mode::Off {
                    set_mode(hwnd, Mode::Forever, true);
                } else {
                    set_mode(hwnd, Mode::Off, true);
                }
            } else if lparam_lo == WM_RBUTTONUP {
                show_context_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wparam.0 & 0xFFFF;
            match id as usize {
                IDM_OFF => set_mode(hwnd, Mode::Off, true),
                IDM_FOREVER => set_mode(hwnd, Mode::Forever, true),
                IDM_1H => set_mode(hwnd, Mode::Timer1H, true),
                IDM_3H => set_mode(hwnd, Mode::Timer3H, true),
                IDM_AUTOSTART => toggle_autostart(),
                IDM_EXIT => PostQuitMessage(0),
                _ => {}
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_ID {
                let _ = KillTimer(hwnd, TIMER_ID);
                set_mode(hwnd, Mode::Off, false);
                
                // Show timeout notification
                let mut nid: NOTIFYICONDATAW = mem::zeroed();
                nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
                nid.hWnd = hwnd;
                nid.uID = ID_TRAY_ICON;
                nid.uFlags = NIF_INFO;
                set_string(&mut nid.szInfoTitle, "AlwaysOn");
                set_string(&mut nid.szInfo, if STATE.lang_tr { "Süre doldu, uyku engelleme kapatıldı." } else { "Timer ended, sleep prevention deactivated." });
                nid.dwInfoFlags = NIIF_USER | NIIF_LARGE_ICON;
                let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn set_mode(hwnd: HWND, new_mode: Mode, notify: bool) {
    let _ = KillTimer(hwnd, TIMER_ID);
    STATE.mode = new_mode;
    
    if STATE.mode == Mode::Timer1H {
        let _ = SetTimer(hwnd, TIMER_ID, 3600 * 1000, None);
    } else if STATE.mode == Mode::Timer3H {
        let _ = SetTimer(hwnd, TIMER_ID, 3 * 3600 * 1000, None);
    }
    
    update_tray_and_power(hwnd, notify);
}

unsafe fn update_tray_and_power(hwnd: HWND, notify: bool) {
    let mut nid: NOTIFYICONDATAW = mem::zeroed();
    nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = ID_TRAY_ICON;
    nid.uFlags = NIF_ICON | NIF_TIP | if notify { NIF_INFO } else { Default::default() };

    let is_active = STATE.mode != Mode::Off;

    if is_active {
        SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED);
        nid.hIcon = STATE.hicon_awake;
        set_string(&mut nid.szTip, if STATE.lang_tr { "AlwaysOn: Aktif" } else { "AlwaysOn: Active" });
        if notify {
            set_string(&mut nid.szInfo, if STATE.lang_tr { "Uyku engelleme aktif." } else { "AlwaysOn activated." });
        }
    } else {
        SetThreadExecutionState(ES_CONTINUOUS);
        nid.hIcon = STATE.hicon_sleep;
        set_string(&mut nid.szTip, if STATE.lang_tr { "AlwaysOn: Kapalı" } else { "AlwaysOn: Inactive" });
        if notify {
            set_string(&mut nid.szInfo, if STATE.lang_tr { "Uyku engelleme devre dışı." } else { "AlwaysOn deactivated." });
        }
    }

    if notify {
        set_string(&mut nid.szInfoTitle, "AlwaysOn");
        nid.dwInfoFlags = NIIF_USER | NIIF_LARGE_ICON;
    }

    let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
}

unsafe fn is_autostart() -> bool {
    let mut hkey = HKEY::default();
    if RegOpenKeyExW(HKEY_CURRENT_USER, w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"), 0, KEY_READ, &mut hkey).is_ok() {
        let res = RegQueryValueExW(hkey, w!("AlwaysOn"), None, None, None, None);
        let _ = RegCloseKey(hkey);
        res.is_ok()
    } else {
        false
    }
}

unsafe fn toggle_autostart() {
    let mut hkey = HKEY::default();
    if RegOpenKeyExW(HKEY_CURRENT_USER, w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"), 0, KEY_WRITE, &mut hkey).is_ok() {
        if is_autostart() {
            let _ = RegDeleteValueW(hkey, w!("AlwaysOn"));
        } else {
            let mut path = [0u16; 1024];
            let len = GetModuleFileNameW(None, &mut path);
            let byte_len = (len + 1) * 2;
            let _ = RegSetValueExW(hkey, w!("AlwaysOn"), 0, REG_SZ, Some(std::slice::from_raw_parts(path.as_ptr() as *const u8, byte_len as usize)));
        }
        let _ = RegCloseKey(hkey);
    }
}

unsafe fn show_context_menu(hwnd: HWND) {
    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);

    if let Ok(hmenu) = CreatePopupMenu() {
        let txt_off = if STATE.lang_tr { w!("Kapalı (Normal Uyku)") } else { w!("Off (Normal Sleep)") };
        let txt_forever = if STATE.lang_tr { w!("Sürekli Açık Tut") } else { w!("Always On") };
        let txt_1h = if STATE.lang_tr { w!("1 Saat Açık Tut") } else { w!("Keep On for 1 Hour") };
        let txt_3h = if STATE.lang_tr { w!("3 Saat Açık Tut") } else { w!("Keep On for 3 Hours") };
        let txt_auto = if STATE.lang_tr { w!("Başlangıçta Otomatik Çalıştır") } else { w!("Run at Startup") };
        let txt_exit = if STATE.lang_tr { w!("Çıkış") } else { w!("Exit") };

        let _ = InsertMenuW(hmenu, 0xFFFFFFFF, MF_BYPOSITION | MF_STRING, IDM_OFF, txt_off);
        let _ = InsertMenuW(hmenu, 0xFFFFFFFF, MF_BYPOSITION | MF_STRING, IDM_FOREVER, txt_forever);
        let _ = InsertMenuW(hmenu, 0xFFFFFFFF, MF_BYPOSITION | MF_STRING, IDM_1H, txt_1h);
        let _ = InsertMenuW(hmenu, 0xFFFFFFFF, MF_BYPOSITION | MF_STRING, IDM_3H, txt_3h);
        
        let active_id = match STATE.mode {
            Mode::Off => IDM_OFF,
            Mode::Forever => IDM_FOREVER,
            Mode::Timer1H => IDM_1H,
            Mode::Timer3H => IDM_3H,
        };
        let _ = CheckMenuRadioItem(hmenu, IDM_OFF as _, IDM_3H as _, active_id as _, MF_BYCOMMAND.0);

        let _ = InsertMenuW(hmenu, 0xFFFFFFFF, MF_BYPOSITION | MF_SEPARATOR, 0, PCWSTR(std::ptr::null()));

        let flag_auto = if is_autostart() { MF_CHECKED } else { MF_UNCHECKED };
        let _ = InsertMenuW(hmenu, 0xFFFFFFFF, MF_BYPOSITION | MF_STRING | flag_auto, IDM_AUTOSTART, txt_auto);

        let _ = InsertMenuW(hmenu, 0xFFFFFFFF, MF_BYPOSITION | MF_SEPARATOR, 0, PCWSTR(std::ptr::null()));
        let _ = InsertMenuW(hmenu, 0xFFFFFFFF, MF_BYPOSITION | MF_STRING, IDM_EXIT, txt_exit);

        let _ = SetForegroundWindow(hwnd);
        let _ = TrackPopupMenu(
            hmenu,
            TPM_BOTTOMALIGN | TPM_RIGHTALIGN | TPM_LEFTBUTTON,
            pt.x,
            pt.y,
            0,
            hwnd,
            None,
        );
        let _ = DestroyMenu(hmenu);
    }
}
