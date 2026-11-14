#![windows_subsystem = "windows"]

use std::ffi::c_void;
use std::fs;
use std::mem;
use std::path::{Path, PathBuf};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::{
    GetModuleFileNameW, GetModuleHandleW, GetProcAddress, LoadLibraryW,
};
use windows::Win32::System::Power::*;
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

extern "system" {
    fn GetUserDefaultUILanguage() -> u16;
    fn ReleaseCapture() -> BOOL;
    fn GetCurrentProcess() -> HANDLE;
    fn SetProcessWorkingSetSize(hprocess: HANDLE, min: usize, max: usize) -> BOOL;
}

const WM_USER_TRAYICON: u32 = WM_USER + 1;
const WM_USER_SHOW_UI: u32 = WM_USER + 2;
const ID_TRAY_ICON: u32 = 1;

const TIMER_ID: usize = 999;
const HEARTBEAT_TIMER_ID: usize = 998;
const RETRY_TRAY_TIMER_ID: usize = 997;
const UI_TICK_TIMER_ID: usize = 996;
const ANIM_TIMER_ID: usize = 995;

const IDM_OPEN_UI: usize = 1000;
const IDM_OFF: usize = 1001;
const IDM_FOREVER: usize = 1002;
const IDM_1H: usize = 1003;
const IDM_3H: usize = 1004;
const IDM_AUTOSTART: usize = 1005;
const IDM_EXIT: usize = 1006;
const IDM_30M: usize = 1007;

const UI_LOGICAL_W: i32 = 360;
const UI_LOGICAL_H: i32 = 304;

const MUG_ICON_DIM: usize = 72;
const MUG_AWAKE_RGBA: &[u8] = include_bytes!("../assets/mug_awake.rgba");
const MUG_SLEEP_RGBA: &[u8] = include_bytes!("../assets/mug_sleep.rgba");

const HIT_NONE: usize = 0;
const HIT_HERO: usize = 1;
const HIT_MODE_FOREVER: usize = 2;
const HIT_MODE_30M: usize = 3;
const HIT_MODE_1H: usize = 4;
const HIT_MODE_3H: usize = 5;
const HIT_TOGGLE_DISPLAY: usize = 6;
const HIT_TOGGLE_AUTOSTART: usize = 7;
const HIT_BTN_HIDE: usize = 8;
const HIT_BTN_EXIT: usize = 9;
const HIT_BTN_SETTINGS: usize = 10;
const HIT_BTN_BACK: usize = 11;
const HIT_CLOSE_TO_TRAY: usize = 12;
const HIT_CLOSE_EXIT: usize = 13;

const RUN_KEY_PATH: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const STARTUP_APPROVED_PATH: PCWSTR =
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run");
const APP_REG_NAME: PCWSTR = w!("AlwaysOn");
const SETTINGS_KEY_PATH: PCWSTR = w!("Software\\AlwaysOn");
const REG_CLOSE_TO_TRAY: PCWSTR = w!("CloseToTray");
const REG_KEEP_DISPLAY_ON: PCWSTR = w!("KeepDisplayOn");

#[derive(PartialEq, Clone, Copy, Debug)]
enum Mode {
    Off,
    Forever,
    Timer30M,
    Timer1H,
    Timer3H,
}

struct AppState {
    mode: Mode,
    hicon_awake: HICON,
    hicon_sleep: HICON,
    hicon_hero_awake: HICON,
    hicon_hero_sleep: HICON,
    lang_tr: bool,
    wm_taskbar_created: u32,
    keep_display_on: bool,
    close_to_tray: bool,
    in_settings: bool,
    autostart_cached: bool,
    remaining_secs: u32,
    hover_item: usize,
    last_hide_time: i32,
    has_hidden_recently: bool,
    anim_running: bool,
    anim_hero_toggle: f32,
    anim_disp_toggle: f32,
    anim_auto_toggle: f32,
    anim_mode_idx: f32,
    anim_mode_alpha: f32,
    anim_close_opt: f32,
    anim_view_slide: f32,
    anim_open_progress: f32,
    target_win_x: i32,
    target_win_y: i32,
}

static mut STATE: AppState = AppState {
    mode: Mode::Forever,
    hicon_awake: HICON(std::ptr::null_mut()),
    hicon_sleep: HICON(std::ptr::null_mut()),
    hicon_hero_awake: HICON(std::ptr::null_mut()),
    hicon_hero_sleep: HICON(std::ptr::null_mut()),
    lang_tr: false,
    wm_taskbar_created: 0,
    keep_display_on: true,
    close_to_tray: true,
    in_settings: false,
    autostart_cached: false,
    remaining_secs: 0,
    hover_item: HIT_NONE,
    last_hide_time: 0,
    has_hidden_recently: false,
    anim_running: false,
    anim_hero_toggle: 1.0,
    anim_disp_toggle: 1.0,
    anim_auto_toggle: 0.0,
    anim_mode_idx: 0.0,
    anim_mode_alpha: 1.0,
    anim_close_opt: 0.0,
    anim_view_slide: 0.0,
    anim_open_progress: 1.0,
    target_win_x: 0,
    target_win_y: 0,
};

struct PaintCache {
    dpi: i32,
    cw: i32,
    ch: i32,
    mem_dc: HDC,
    text_dc: HDC,
    main_dib: HBITMAP,
    text_dib: HBITMAP,
    old_main: HGDIOBJ,
    old_text: HGDIOBJ,
    main_ptr: *mut u32,
    text_ptr: *mut u32,
    font_title_disp: HFONT,
    font_section: HFONT,
    font_body: HFONT,
    font_body_bold: HFONT,
    font_sub: HFONT,
    font_caption: HFONT,
    font_fluent: HFONT,
    font_fluent_gear: HFONT,
    font_fluent_sm: HFONT,
}

impl PaintCache {
    const fn new() -> Self {
        Self {
            dpi: 0,
            cw: 0,
            ch: 0,
            mem_dc: HDC(std::ptr::null_mut()),
            text_dc: HDC(std::ptr::null_mut()),
            main_dib: HBITMAP(std::ptr::null_mut()),
            text_dib: HBITMAP(std::ptr::null_mut()),
            old_main: HGDIOBJ(std::ptr::null_mut()),
            old_text: HGDIOBJ(std::ptr::null_mut()),
            main_ptr: std::ptr::null_mut(),
            text_ptr: std::ptr::null_mut(),
            font_title_disp: HFONT(std::ptr::null_mut()),
            font_section: HFONT(std::ptr::null_mut()),
            font_body: HFONT(std::ptr::null_mut()),
            font_body_bold: HFONT(std::ptr::null_mut()),
            font_sub: HFONT(std::ptr::null_mut()),
            font_caption: HFONT(std::ptr::null_mut()),
            font_fluent: HFONT(std::ptr::null_mut()),
            font_fluent_gear: HFONT(std::ptr::null_mut()),
            font_fluent_sm: HFONT(std::ptr::null_mut()),
        }
    }
}

static mut PAINT_CACHE: PaintCache = PaintCache::new();

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((r as u32) | ((g as u32) << 8) | ((b as u32) << 16))
}

fn set_string<const N: usize>(arr: &mut [u16; N], s: &str) {
    arr.fill(0);
    for (i, c) in s.encode_utf16().take(N - 1).enumerate() {
        arr[i] = c;
    }
}

fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

unsafe fn load_icon_sized(instance: HMODULE, res_id: usize, size: i32) -> HICON {
    let flags = if size == 0 {
        LR_DEFAULTSIZE | LR_SHARED
    } else {
        LR_SHARED
    };
    if let Ok(handle) = LoadImageW(
        instance,
        PCWSTR(res_id as _),
        IMAGE_ICON,
        size,
        size,
        flags,
    ) {
        if !handle.is_invalid() {
            return HICON(handle.0);
        }
    }
    LoadIconW(instance, PCWSTR(res_id as _)).unwrap_or_default()
}

unsafe fn apply_execution_state(active: bool) {
    if active {
        let flags = if STATE.keep_display_on {
            ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED
        } else {
            ES_CONTINUOUS | ES_SYSTEM_REQUIRED
        };
        SetThreadExecutionState(flags);
    } else {
        SetThreadExecutionState(ES_CONTINUOUS);
    }
}

fn tray_tooltip_text() -> String {
    unsafe {
        let active = STATE.mode != Mode::Off;
        if !active {
            return if STATE.lang_tr {
                "AlwaysOn: Kapalı".to_string()
            } else {
                "AlwaysOn: Inactive".to_string()
            };
        }
        if STATE.remaining_secs > 0 {
            let h = STATE.remaining_secs / 3600;
            let m = (STATE.remaining_secs % 3600) / 60;
            let s = STATE.remaining_secs % 60;
            if STATE.lang_tr {
                format!("AlwaysOn: Aktif ({h:02}:{m:02}:{s:02})")
            } else {
                format!("AlwaysOn: Active ({h:02}:{m:02}:{s:02})")
            }
        } else if STATE.lang_tr {
            "AlwaysOn: Aktif".to_string()
        } else {
            "AlwaysOn: Active".to_string()
        }
    }
}

unsafe fn add_or_update_tray_icon(hwnd: HWND) -> bool {
    let mut nid: NOTIFYICONDATAW = mem::zeroed();
    nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = ID_TRAY_ICON;
    nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
    nid.uCallbackMessage = WM_USER_TRAYICON;
    nid.hIcon = if STATE.mode != Mode::Off {
        STATE.hicon_awake
    } else {
        STATE.hicon_sleep
    };
    set_string(&mut nid.szTip, &tray_tooltip_text());

    if Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
        true
    } else {
        Shell_NotifyIconW(NIM_MODIFY, &nid).as_bool()
    }
}

fn current_exe_path() -> Option<PathBuf> {
    unsafe {
        let mut buf = [0u16; 1024];
        let len = GetModuleFileNameW(None, &mut buf) as usize;
        if len == 0 {
            None
        } else {
            Some(PathBuf::from(String::from_utf16_lossy(&buf[..len])))
        }
    }
}

fn get_stable_install_path() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|base| {
        PathBuf::from(base)
            .join("Programs")
            .join("AlwaysOn")
            .join("AlwaysOn.exe")
    })
}

fn paths_equal(a: &Path, b: &Path) -> bool {
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}

fn extract_exe_path(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if let Some(stripped) = trimmed.strip_prefix('"') {
        if let Some(end_quote) = stripped.find('"') {
            return PathBuf::from(&stripped[..end_quote]);
        }
    }
    let lower = trimmed.to_ascii_lowercase();
    if let Some(idx) = lower.find(".exe") {
        return PathBuf::from(&trimmed[..idx + 4]);
    }
    PathBuf::from(trimmed.split_whitespace().next().unwrap_or(trimmed))
}

unsafe fn read_autostart_command() -> Option<String> {
    let mut hkey = HKEY::default();
    if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY_PATH, 0, KEY_READ, &mut hkey).is_err() {
        return None;
    }

    let mut val_type = REG_VALUE_TYPE::default();
    let mut byte_len: u32 = 0;
    let res = RegQueryValueExW(
        hkey,
        APP_REG_NAME,
        None,
        Some(&mut val_type),
        None,
        Some(&mut byte_len),
    );
    if !res.is_ok() || (val_type != REG_SZ && val_type != REG_EXPAND_SZ) || byte_len < 2 {
        let _ = RegCloseKey(hkey);
        return None;
    }

    let mut buf = vec![0u16; (byte_len as usize + 1) / 2];
    let res2 = RegQueryValueExW(
        hkey,
        APP_REG_NAME,
        None,
        Some(&mut val_type),
        Some(buf.as_mut_ptr() as *mut u8),
        Some(&mut byte_len),
    );
    let _ = RegCloseKey(hkey);

    if !res2.is_ok() {
        return None;
    }

    while buf.last() == Some(&0) {
        buf.pop();
    }
    Some(String::from_utf16_lossy(&buf))
}

unsafe fn clear_startup_disabled_flag() {
    let mut hkey = HKEY::default();
    if RegOpenKeyExW(
        HKEY_CURRENT_USER,
        STARTUP_APPROVED_PATH,
        0,
        KEY_WRITE,
        &mut hkey,
    )
    .is_ok()
    {
        let _ = RegDeleteValueW(hkey, APP_REG_NAME);
        let _ = RegCloseKey(hkey);
    }
}

unsafe fn enable_autostart(clear_approved: bool) {
    let Some(curr_exe) = current_exe_path() else {
        return;
    };

    let target_exe = match get_stable_install_path() {
        Some(stable) => {
            if paths_equal(&curr_exe, &stable) {
                stable
            } else {
                let copied = stable
                    .parent()
                    .map(fs::create_dir_all)
                    .transpose()
                    .ok()
                    .flatten()
                    .and_then(|_| fs::copy(&curr_exe, &stable).ok())
                    .is_some();
                if copied || stable.exists() {
                    stable
                } else {
                    curr_exe
                }
            }
        }
        None => curr_exe,
    };

    let mut hkey = HKEY::default();
    if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY_PATH, 0, KEY_WRITE, &mut hkey).is_ok() {
        let cmd = format!("\"{}\" --startup", target_exe.display());
        let wide = to_wide_null(&cmd);
        let _ = RegSetValueExW(
            hkey,
            APP_REG_NAME,
            0,
            REG_SZ,
            Some(std::slice::from_raw_parts(
                wide.as_ptr() as *const u8,
                wide.len() * 2,
            )),
        );
        let _ = RegCloseKey(hkey);
    }

    if clear_approved {
        clear_startup_disabled_flag();
    }
}

unsafe fn disable_autostart() {
    let mut hkey = HKEY::default();
    if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY_PATH, 0, KEY_WRITE, &mut hkey).is_ok() {
        let _ = RegDeleteValueW(hkey, APP_REG_NAME);
        let _ = RegCloseKey(hkey);
    }
}

unsafe fn is_autostart() -> bool {
    if let Some(cmd) = read_autostart_command() {
        let exe_path = extract_exe_path(&cmd);
        exe_path.exists()
    } else {
        false
    }
}

unsafe fn sync_autostart_if_configured() {
    let Some(cmd) = read_autostart_command() else {
        return;
    };
    let configured_exe = extract_exe_path(&cmd);
    let Some(curr_exe) = current_exe_path() else {
        return;
    };

    let needs_repair = !configured_exe.exists()
        || get_stable_install_path()
            .map(|stable| !paths_equal(&configured_exe, &stable) || !paths_equal(&curr_exe, &stable))
            .unwrap_or(false);

    if needs_repair {
        enable_autostart(false);
    }
}

unsafe fn toggle_autostart(hwnd: HWND) {
    if is_autostart() {
        disable_autostart();
        STATE.autostart_cached = false;
    } else {
        enable_autostart(true);
        STATE.autostart_cached = true;
    }
    start_ui_anim(hwnd);
    let _ = InvalidateRect(hwnd, None, false);
}

unsafe fn read_reg_dword(name: PCWSTR, default: u32) -> u32 {
    let mut hkey = HKEY::default();
    if RegOpenKeyExW(HKEY_CURRENT_USER, SETTINGS_KEY_PATH, 0, KEY_READ, &mut hkey).is_err() {
        return default;
    }
    let mut val: u32 = default;
    let mut len: u32 = mem::size_of::<u32>() as u32;
    let mut kind = REG_VALUE_TYPE::default();
    let ok = RegQueryValueExW(
        hkey,
        name,
        None,
        Some(&mut kind),
        Some(&mut val as *mut u32 as *mut u8),
        Some(&mut len),
    )
    .is_ok()
        && kind == REG_DWORD;
    let _ = RegCloseKey(hkey);
    if ok {
        val
    } else {
        default
    }
}

unsafe fn write_reg_dword(name: PCWSTR, val: u32) {
    let mut hkey = HKEY::default();
    if RegCreateKeyExW(
        HKEY_CURRENT_USER,
        SETTINGS_KEY_PATH,
        0,
        None,
        REG_OPTION_NON_VOLATILE,
        KEY_WRITE,
        None,
        &mut hkey,
        None,
    )
    .is_ok()
    {
        let bytes = val.to_ne_bytes();
        let _ = RegSetValueExW(hkey, name, 0, REG_DWORD, Some(&bytes));
        let _ = RegCloseKey(hkey);
    }
}

unsafe fn load_app_settings() {
    STATE.close_to_tray = read_reg_dword(REG_CLOSE_TO_TRAY, 1) != 0;
    STATE.keep_display_on = read_reg_dword(REG_KEEP_DISPLAY_ON, 1) != 0;
    STATE.autostart_cached = is_autostart();
    sync_anim_immediately();
}

#[inline]
fn mode_to_index(m: Mode, fallback: f32) -> f32 {
    match m {
        Mode::Forever => 0.0,
        Mode::Timer30M => 1.0,
        Mode::Timer1H => 2.0,
        Mode::Timer3H => 3.0,
        Mode::Off => fallback,
    }
}

#[inline]
fn lerp_step(curr: &mut f32, target: f32, speed: f32) -> bool {
    let diff = target - *curr;
    if diff.abs() < 0.006 {
        if (*curr - target).abs() > f32::EPSILON {
            *curr = target;
            return true;
        }
        return false;
    }
    *curr += diff * speed;
    true
}

unsafe fn sync_anim_immediately() {
    STATE.anim_hero_toggle = if STATE.mode != Mode::Off { 1.0 } else { 0.0 };
    STATE.anim_disp_toggle = if STATE.keep_display_on { 1.0 } else { 0.0 };
    STATE.anim_auto_toggle = if STATE.autostart_cached { 1.0 } else { 0.0 };
    STATE.anim_mode_idx = mode_to_index(STATE.mode, 0.0);
    STATE.anim_mode_alpha = if STATE.mode != Mode::Off { 1.0 } else { 0.0 };
    STATE.anim_close_opt = if STATE.close_to_tray { 0.0 } else { 1.0 };
    STATE.anim_view_slide = 0.0;
    STATE.anim_open_progress = 1.0;
}

unsafe fn start_ui_anim(hwnd: HWND) {
    if !hwnd.0.is_null() && IsWindowVisible(hwnd).as_bool() && !STATE.anim_running {
        STATE.anim_running = true;
        let _ = SetTimer(hwnd, ANIM_TIMER_ID, 15, None);
    }
}

unsafe fn step_ui_anim(hwnd: HWND) {
    let speed = 0.34;
    let mut any_changed = false;

    let hero_target = if STATE.mode != Mode::Off { 1.0 } else { 0.0 };
    let disp_target = if STATE.keep_display_on { 1.0 } else { 0.0 };
    let auto_target = if STATE.autostart_cached { 1.0 } else { 0.0 };
    let mode_idx_target = mode_to_index(STATE.mode, STATE.anim_mode_idx);
    let mode_alpha_target = if STATE.mode != Mode::Off { 1.0 } else { 0.0 };
    let close_opt_target = if STATE.close_to_tray { 0.0 } else { 1.0 };

    let mut hero_val = STATE.anim_hero_toggle;
    any_changed |= lerp_step(&mut hero_val, hero_target, speed);
    STATE.anim_hero_toggle = hero_val;

    let mut disp_val = STATE.anim_disp_toggle;
    any_changed |= lerp_step(&mut disp_val, disp_target, speed);
    STATE.anim_disp_toggle = disp_val;

    let mut auto_val = STATE.anim_auto_toggle;
    any_changed |= lerp_step(&mut auto_val, auto_target, speed);
    STATE.anim_auto_toggle = auto_val;

    let mut mode_val = STATE.anim_mode_idx;
    any_changed |= lerp_step(&mut mode_val, mode_idx_target, speed);
    STATE.anim_mode_idx = mode_val;

    let mut alpha_val = STATE.anim_mode_alpha;
    any_changed |= lerp_step(&mut alpha_val, mode_alpha_target, speed);
    STATE.anim_mode_alpha = alpha_val;

    let mut close_val = STATE.anim_close_opt;
    any_changed |= lerp_step(&mut close_val, close_opt_target, speed);
    STATE.anim_close_opt = close_val;

    let mut view_val = STATE.anim_view_slide;
    any_changed |= lerp_step(&mut view_val, 0.0, speed);
    STATE.anim_view_slide = view_val;

    if STATE.anim_open_progress < 1.0 {
        STATE.anim_open_progress = (STATE.anim_open_progress + 0.25).min(1.0);
        let u = 1.0 - STATE.anim_open_progress;
        let slide_y = (u * u * u * (sc(10, get_window_dpi(hwnd)) as f32)).round() as i32;
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            STATE.target_win_x,
            STATE.target_win_y + slide_y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
        any_changed = true;
    }

    if !any_changed {
        STATE.anim_running = false;
        let _ = KillTimer(hwnd, ANIM_TIMER_ID);
    }
    let _ = InvalidateRect(hwnd, None, false);
}

unsafe fn trim_working_set() {
    let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
}

unsafe fn release_paint_surfaces() {
    if !PAINT_CACHE.mem_dc.0.is_null() {
        if !PAINT_CACHE.old_main.0.is_null() {
            SelectObject(PAINT_CACHE.mem_dc, PAINT_CACHE.old_main);
            PAINT_CACHE.old_main = HGDIOBJ(std::ptr::null_mut());
        }
        let _ = DeleteDC(PAINT_CACHE.mem_dc);
        PAINT_CACHE.mem_dc = HDC(std::ptr::null_mut());
    }
    if !PAINT_CACHE.text_dc.0.is_null() {
        if !PAINT_CACHE.old_text.0.is_null() {
            SelectObject(PAINT_CACHE.text_dc, PAINT_CACHE.old_text);
            PAINT_CACHE.old_text = HGDIOBJ(std::ptr::null_mut());
        }
        let _ = DeleteDC(PAINT_CACHE.text_dc);
        PAINT_CACHE.text_dc = HDC(std::ptr::null_mut());
    }
    if !PAINT_CACHE.main_dib.0.is_null() {
        let _ = DeleteObject(HGDIOBJ(PAINT_CACHE.main_dib.0));
        PAINT_CACHE.main_dib = HBITMAP(std::ptr::null_mut());
    }
    if !PAINT_CACHE.text_dib.0.is_null() {
        let _ = DeleteObject(HGDIOBJ(PAINT_CACHE.text_dib.0));
        PAINT_CACHE.text_dib = HBITMAP(std::ptr::null_mut());
    }
    PAINT_CACHE.main_ptr = std::ptr::null_mut();
    PAINT_CACHE.text_ptr = std::ptr::null_mut();
    PAINT_CACHE.cw = 0;
    PAINT_CACHE.ch = 0;
}

unsafe fn free_hfont(f: *mut HFONT) {
    if !(*f).0.is_null() {
        let _ = DeleteObject(HGDIOBJ((*f).0));
        *f = HFONT(std::ptr::null_mut());
    }
}

unsafe fn release_paint_fonts() {
    let fonts = [
        &raw mut PAINT_CACHE.font_title_disp,
        &raw mut PAINT_CACHE.font_section,
        &raw mut PAINT_CACHE.font_body,
        &raw mut PAINT_CACHE.font_body_bold,
        &raw mut PAINT_CACHE.font_sub,
        &raw mut PAINT_CACHE.font_caption,
        &raw mut PAINT_CACHE.font_fluent,
        &raw mut PAINT_CACHE.font_fluent_gear,
        &raw mut PAINT_CACHE.font_fluent_sm,
    ];
    for f in fonts {
        free_hfont(f);
    }
    PAINT_CACHE.dpi = 0;
}

unsafe fn release_paint_cache() {
    release_paint_surfaces();
    release_paint_fonts();
}

unsafe fn ensure_paint_cache(hdc: HDC, cw: i32, ch: i32, dpi: i32) -> bool {
    if PAINT_CACHE.dpi != dpi || PAINT_CACHE.font_body.0.is_null() {
        release_paint_fonts();
        PAINT_CACHE.font_title_disp =
            make_font_face(dpi, 15, FW_SEMIBOLD.0 as i32, w!("Segoe UI Variable Display"));
        PAINT_CACHE.font_section =
            make_font_face(dpi, 13, FW_SEMIBOLD.0 as i32, w!("Segoe UI Variable Text"));
        PAINT_CACHE.font_body =
            make_font_face(dpi, 14, FW_NORMAL.0 as i32, w!("Segoe UI Variable Text"));
        PAINT_CACHE.font_body_bold =
            make_font_face(dpi, 14, FW_SEMIBOLD.0 as i32, w!("Segoe UI Variable Text"));
        PAINT_CACHE.font_sub =
            make_font_face(dpi, 12, FW_NORMAL.0 as i32, w!("Segoe UI Variable Text"));
        PAINT_CACHE.font_caption =
            make_font_face(dpi, 13, FW_NORMAL.0 as i32, w!("Segoe UI Variable Text"));
        PAINT_CACHE.font_fluent =
            make_font_face(dpi, 16, FW_NORMAL.0 as i32, w!("Segoe Fluent Icons"));
        PAINT_CACHE.font_fluent_gear =
            make_font_face(dpi, 13, FW_NORMAL.0 as i32, w!("Segoe Fluent Icons"));
        PAINT_CACHE.font_fluent_sm =
            make_font_face(dpi, 11, FW_NORMAL.0 as i32, w!("Segoe Fluent Icons"));
        PAINT_CACHE.dpi = dpi;
    }

    if PAINT_CACHE.cw != cw
        || PAINT_CACHE.ch != ch
        || PAINT_CACHE.main_ptr.is_null()
        || PAINT_CACHE.text_ptr.is_null()
    {
        release_paint_surfaces();

        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: cw,
                biHeight: -ch,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };

        let mem_dc = CreateCompatibleDC(hdc);
        let mut main_ptr: *mut c_void = std::ptr::null_mut();
        let main_dib = CreateDIBSection(mem_dc, &bmi, DIB_RGB_COLORS, &mut main_ptr, None, 0);

        let text_dc = CreateCompatibleDC(hdc);
        let mut text_ptr: *mut c_void = std::ptr::null_mut();
        let text_dib = CreateDIBSection(text_dc, &bmi, DIB_RGB_COLORS, &mut text_ptr, None, 0);

        if main_dib.is_err() || text_dib.is_err() || main_ptr.is_null() || text_ptr.is_null() {
            if let Ok(b) = main_dib {
                let _ = DeleteObject(HGDIOBJ(b.0));
            }
            if let Ok(b) = text_dib {
                let _ = DeleteObject(HGDIOBJ(b.0));
            }
            let _ = DeleteDC(mem_dc);
            let _ = DeleteDC(text_dc);
            return false;
        }

        let main_dib = main_dib.unwrap();
        let text_dib = text_dib.unwrap();
        PAINT_CACHE.old_main = SelectObject(mem_dc, HGDIOBJ(main_dib.0));
        PAINT_CACHE.old_text = SelectObject(text_dc, HGDIOBJ(text_dib.0));
        SetBkMode(text_dc, TRANSPARENT);
        PAINT_CACHE.mem_dc = mem_dc;
        PAINT_CACHE.text_dc = text_dc;
        PAINT_CACHE.main_dib = main_dib;
        PAINT_CACHE.text_dib = text_dib;
        PAINT_CACHE.main_ptr = main_ptr as *mut u32;
        PAINT_CACHE.text_ptr = text_ptr as *mut u32;
        PAINT_CACHE.cw = cw;
        PAINT_CACHE.ch = ch;
    }

    true
}

#[repr(C)]
struct DwmMargins {
    cx_left_width: i32,
    cx_right_width: i32,
    cy_top_height: i32,
    cy_bottom_height: i32,
}

unsafe fn apply_dwm_winui(hwnd: HWND) {
    type DwmSetWindowAttributeFn =
        unsafe extern "system" fn(HWND, u32, *const c_void, u32) -> i32;
    type DwmExtendFrameFn = unsafe extern "system" fn(HWND, *const DwmMargins) -> i32;

    if let Ok(dwmapi) = LoadLibraryW(w!("dwmapi.dll")) {
        if let Some(proc) = GetProcAddress(dwmapi, s!("DwmExtendFrameIntoClientArea")) {
            let extend: DwmExtendFrameFn = mem::transmute(proc);
            let margins = DwmMargins {
                cx_left_width: -1,
                cx_right_width: -1,
                cy_top_height: -1,
                cy_bottom_height: -1,
            };
            let _ = extend(hwnd, &margins);
        }
        if let Some(proc) = GetProcAddress(dwmapi, s!("DwmSetWindowAttribute")) {
            let set_attr: DwmSetWindowAttributeFn = mem::transmute(proc);
            let dark: i32 = 1;
            let _ = set_attr(hwnd, 20, &dark as *const i32 as *const c_void, 4);
            let corner_round: i32 = 2;
            let _ = set_attr(hwnd, 33, &corner_round as *const i32 as *const c_void, 4);
            let border_color: u32 = rgb(52, 52, 52).0;
            let _ = set_attr(hwnd, 34, &border_color as *const u32 as *const c_void, 4);
            let backdrop: i32 = 4; // DWMSBT_TABBEDWINDOW (Mica Alt)
            let _ = set_attr(hwnd, 38, &backdrop as *const i32 as *const c_void, 4);
        }
    }
}

unsafe fn get_window_dpi(hwnd: HWND) -> i32 {
    type GetDpiForWindowFn = unsafe extern "system" fn(HWND) -> u32;

    if !hwnd.0.is_null() {
        if let Ok(user32) = LoadLibraryW(w!("user32.dll")) {
            if let Some(proc) = GetProcAddress(user32, s!("GetDpiForWindow")) {
                let get_dpi: GetDpiForWindowFn = mem::transmute(proc);
                let d = get_dpi(hwnd) as i32;
                if d > 0 {
                    return d;
                }
            }
        }
    }

    let hdc = GetDC(hwnd);
    let dpi = if !hdc.is_invalid() {
        let d = GetDeviceCaps(hdc, LOGPIXELSX);
        ReleaseDC(hwnd, hdc);
        d
    } else {
        96
    };
    if dpi > 0 {
        dpi
    } else {
        96
    }
}

fn sc(val: i32, dpi: i32) -> i32 {
    (val * dpi + 48) / 96
}

fn unscale(val: i32, dpi: i32) -> i32 {
    (val * 96) / dpi
}

unsafe fn position_flyout_bottom_right(hwnd: HWND) {
    let dpi = get_window_dpi(hwnd);
    let win_w = sc(UI_LOGICAL_W, dpi);
    let win_h = sc(UI_LOGICAL_H, dpi);
    let margin = sc(12, dpi);

    let mut work_rc = RECT::default();
    let ok = SystemParametersInfoW(
        SPI_GETWORKAREA,
        0,
        Some(&mut work_rc as *mut _ as *mut c_void),
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
    )
    .is_ok();

    let (x, y) = if ok {
        (work_rc.right - win_w - margin, work_rc.bottom - win_h - margin)
    } else {
        (
            GetSystemMetrics(SM_CXSCREEN) - win_w - margin,
            GetSystemMetrics(SM_CYSCREEN) - win_h - margin,
        )
    };

    STATE.target_win_x = x;
    STATE.target_win_y = y;
    let slide_y = if STATE.anim_open_progress < 1.0 {
        let u = 1.0 - STATE.anim_open_progress;
        (u * u * u * (sc(10, dpi) as f32)).round() as i32
    } else {
        0
    };
    let _ = SetWindowPos(hwnd, HWND_TOPMOST, x, y + slide_y, win_w, win_h, SWP_SHOWWINDOW);
}

unsafe fn show_control_panel(hwnd: HWND) {
    STATE.has_hidden_recently = false;
    STATE.hover_item = HIT_NONE;
    STATE.autostart_cached = is_autostart();
    sync_anim_immediately();
    STATE.anim_open_progress = 0.0;
    position_flyout_bottom_right(hwnd);
    let _ = ShowWindow(hwnd, SW_SHOW);
    let _ = SetForegroundWindow(hwnd);
    start_ui_anim(hwnd);
    let _ = InvalidateRect(hwnd, None, false);
}

unsafe fn hide_control_panel(hwnd: HWND, from_deactivate: bool) {
    STATE.hover_item = HIT_NONE;
    if STATE.anim_running {
        STATE.anim_running = false;
        let _ = KillTimer(hwnd, ANIM_TIMER_ID);
    }
    sync_anim_immediately();
    if from_deactivate {
        STATE.last_hide_time = GetMessageTime();
        STATE.has_hidden_recently = true;
    } else {
        STATE.has_hidden_recently = false;
    }
    let _ = ShowWindow(hwnd, SW_HIDE);
    release_paint_cache();
    trim_working_set();
}

fn in_rect(x: i32, y: i32, rx: i32, ry: i32, rw: i32, rh: i32) -> bool {
    x >= rx && x < rx + rw && y >= ry && y < ry + rh
}

fn hit_test_ui(lx: i32, ly: i32) -> usize {
    let in_settings = unsafe { STATE.in_settings };
    if in_rect(lx, ly, 236, 5, 36, 28) {
        return HIT_BTN_SETTINGS;
    }
    if in_rect(lx, ly, 276, 5, 36, 28) {
        return HIT_BTN_HIDE;
    }
    if in_rect(lx, ly, 316, 5, 36, 28) {
        return HIT_BTN_EXIT;
    }
    if in_settings {
        if in_rect(lx, ly, 12, 5, 32, 28) {
            HIT_BTN_BACK
        } else if in_rect(lx, ly, 16, 66, 328, 62) {
            HIT_TOGGLE_AUTOSTART
        } else if in_rect(lx, ly, 32, 228, 146, 38) {
            HIT_CLOSE_TO_TRAY
        } else if in_rect(lx, ly, 182, 228, 146, 38) {
            HIT_CLOSE_EXIT
        } else {
            HIT_NONE
        }
    } else if in_rect(lx, ly, 16, 44, 328, 72) {
        HIT_HERO
    } else if in_rect(lx, ly, 20, 154, 78, 36) {
        HIT_MODE_FOREVER
    } else if in_rect(lx, ly, 100, 154, 78, 36) {
        HIT_MODE_30M
    } else if in_rect(lx, ly, 180, 154, 78, 36) {
        HIT_MODE_1H
    } else if in_rect(lx, ly, 260, 154, 78, 36) {
        HIT_MODE_3H
    } else if in_rect(lx, ly, 16, 228, 328, 58) {
        HIT_TOGGLE_DISPLAY
    } else {
        HIT_NONE
    }
}

struct SdfCanvas<'a> {
    buf: &'a mut [u32],
    w: i32,
    h: i32,
    s: f32,
}

impl<'a> SdfCanvas<'a> {
    #[inline]
    fn blend_px(&mut self, px: i32, py: i32, r: u8, g: u8, b: u8, a: f32) {
        if px < 0 || py < 0 || px >= self.w || py >= self.h || a <= 0.002 {
            return;
        }
        let a = a.min(1.0);
        let idx = (py as usize) * (self.w as usize) + (px as usize);
        let dst = self.buf[idx];
        let da = ((dst >> 24) & 0xFF) as f32;
        let dr = ((dst >> 16) & 0xFF) as f32;
        let dg = ((dst >> 8) & 0xFF) as f32;
        let db = (dst & 0xFF) as f32;
        let inv_a = 1.0 - a;
        let out_a = (a * 255.0 + da * inv_a + 0.5).clamp(0.0, 255.0) as u32;
        let out_r = ((r as f32) * a + dr * inv_a + 0.5).clamp(0.0, 255.0) as u32;
        let out_g = ((g as f32) * a + dg * inv_a + 0.5).clamp(0.0, 255.0) as u32;
        let out_b = ((b as f32) * a + db * inv_a + 0.5).clamp(0.0, 255.0) as u32;
        self.buf[idx] = (out_a << 24) | (out_r << 16) | (out_g << 8) | out_b;
    }

    #[inline]
    fn sd_round_rect(px: f32, py: f32, x: f32, y: f32, w: f32, h: f32, r: f32) -> f32 {
        let cx = x + w * 0.5;
        let cy = y + h * 0.5;
        let r = r.min(w * 0.5).min(h * 0.5);
        let qx = (px - cx).abs() - (w * 0.5 - r);
        let qy = (py - cy).abs() - (h * 0.5 - r);
        let out = qx.max(0.0).hypot(qy.max(0.0));
        let inside = qx.max(qy).min(0.0);
        out + inside - r
    }

    fn fill_winui_base(&mut self) {
        // Premultiplied #1F1F1F at 94% opacity so Mica Alt tints lightly while staying crisp in all states
        let a: u32 = 240;
        let ch: u32 = (31 * a + 127) / 255;
        let pix = (a << 24) | (ch << 16) | (ch << 8) | ch;
        self.buf.fill(pix);
    }

    #[inline]
    #[allow(clippy::too_many_arguments)]
    fn draw_sdf_px(
        &mut self,
        px: i32,
        py: i32,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        border_w: f32,
        fill: [u8; 3],
        fill_alpha: f32,
        border_col: [u8; 3],
        border_alpha: f32,
    ) {
        let d = Self::sd_round_rect(px as f32 + 0.5, py as f32 + 0.5, x, y, w, h, r);
        if d < 0.7 {
            let body_cov = (0.55 - d).clamp(0.0, 1.0);
            self.blend_px(px, py, fill[0], fill[1], fill[2], fill_alpha * body_cov);
            if border_alpha > 0.0 {
                let ring_dist = (d + border_w * 0.48).abs();
                let rim_cov = ((border_w * 0.62 - ring_dist) / 1.05).clamp(0.0, 1.0);
                if rim_cov > 0.005 {
                    self.blend_px(
                        px,
                        py,
                        border_col[0],
                        border_col[1],
                        border_col[2],
                        border_alpha * rim_cov,
                    );
                }
            }
        }
    }

    fn draw_round_card(
        &mut self,
        lx: f32,
        ly: f32,
        lw: f32,
        lh: f32,
        lr: f32,
        fill: [u8; 3],
        fill_alpha: f32,
        border_col: [u8; 3],
        border_alpha: f32,
    ) {
        let s = self.s;
        let x = lx * s;
        let y = ly * s;
        let w = lw * s;
        let h = lh * s;
        let r = lr * s;
        let px0 = ((x - 2.0).floor() as i32).max(0);
        let py0 = ((y - 2.0).floor() as i32).max(0);
        let px1 = ((x + w + 2.0).ceil() as i32).min(self.w);
        let py1 = ((y + h + 2.0).ceil() as i32).min(self.h);
        let border_w = (1.0 * s).max(1.0);

        let inset = r.max(border_w * 1.25 + 1.5);
        let ix0 = ((x + inset).ceil() as i32).clamp(px0, px1);
        let iy0 = ((y + inset).ceil() as i32).clamp(py0, py1);
        let ix1 = ((x + w - inset).floor() as i32).clamp(ix0, px1);
        let iy1 = ((y + h - inset).floor() as i32).clamp(iy0, py1);
        let has_interior = ix1 > ix0 && iy1 > iy0;
        let opaque_fill = fill_alpha >= 0.999;
        let opaque_pix =
            (0xFFu32 << 24) | ((fill[0] as u32) << 16) | ((fill[1] as u32) << 8) | (fill[2] as u32);

        for py in py0..py1 {
            if has_interior && py >= iy0 && py < iy1 {
                for px in px0..ix0 {
                    self.draw_sdf_px(px, py, x, y, w, h, r, border_w, fill, fill_alpha, border_col, border_alpha);
                }
                if opaque_fill {
                    let row = (py as usize) * (self.w as usize);
                    self.buf[row + ix0 as usize..row + ix1 as usize].fill(opaque_pix);
                } else {
                    for px in ix0..ix1 {
                        self.blend_px(px, py, fill[0], fill[1], fill[2], fill_alpha);
                    }
                }
                for px in ix1..px1 {
                    self.draw_sdf_px(px, py, x, y, w, h, r, border_w, fill, fill_alpha, border_col, border_alpha);
                }
            } else {
                for px in px0..px1 {
                    self.draw_sdf_px(px, py, x, y, w, h, r, border_w, fill, fill_alpha, border_col, border_alpha);
                }
            }
        }
    }

    fn draw_winui_toggle(&mut self, lx: f32, ly: f32, lw: f32, lh: f32, t: f32) {
        let t = t.clamp(0.0, 1.0);
        let lerp_u8 = |a: u8, b: u8| -> u8 {
            ((a as f32) + ((b as f32) - (a as f32)) * t)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        let track_col = [lerp_u8(35, 76), lerp_u8(35, 194), lerp_u8(35, 255)];
        let r = lh * 0.5;
        self.draw_round_card(
            lx,
            ly,
            lw,
            lh,
            r,
            track_col,
            1.0,
            [138, 138, 138],
            1.0 - t,
        );

        let stretch = 4.0 * t * (1.0 - t) * 3.5;
        let kh = 11.0 + t * 1.0;
        let kw = kh + stretch;
        let kx = lx + 4.5 + t * (lw - 8.5 - kw) + stretch * 0.5 - stretch * t;
        let ky = ly + (lh - kh) * 0.5;
        let thumb_col = [lerp_u8(200, 20), lerp_u8(200, 20), lerp_u8(200, 20)];
        self.draw_round_card(kx, ky, kw, kh, kh * 0.5, thumb_col, 1.0, thumb_col, 0.0);
    }

    fn draw_rgba_icon(&mut self, rgba: &[u8], lx: f32, ly: f32, lw: f32, lh: f32) {
        let dim = MUG_ICON_DIM;
        if rgba.len() < dim * dim * 4 {
            return;
        }
        let s = self.s;
        let x0 = (lx * s).round();
        let y0 = (ly * s).round();
        let dw = (lw * s).max(1.0);
        let dh = (lh * s).max(1.0);
        let px0 = (x0.floor() as i32).max(0);
        let py0 = (y0.floor() as i32).max(0);
        let px1 = ((x0 + dw).ceil() as i32).min(self.w);
        let py1 = ((y0 + dh).ceil() as i32).min(self.h);

        for py in py0..py1 {
            let v = (((py as f32 + 0.5) - y0) / dh).clamp(0.0, 1.0) * ((dim - 1) as f32);
            let iy0 = (v.floor() as usize).min(dim - 1);
            let iy1 = (iy0 + 1).min(dim - 1);
            let fy = v - (iy0 as f32);
            for px in px0..px1 {
                let u = (((px as f32 + 0.5) - x0) / dw).clamp(0.0, 1.0) * ((dim - 1) as f32);
                let ix0 = (u.floor() as usize).min(dim - 1);
                let ix1 = (ix0 + 1).min(dim - 1);
                let fx = u - (ix0 as f32);

                let i00 = (iy0 * dim + ix0) * 4;
                let i10 = (iy0 * dim + ix1) * 4;
                let i01 = (iy1 * dim + ix0) * 4;
                let i11 = (iy1 * dim + ix1) * 4;

                let w00 = (1.0 - fx) * (1.0 - fy);
                let w10 = fx * (1.0 - fy);
                let w01 = (1.0 - fx) * fy;
                let w11 = fx * fy;

                let sa = rgba[i00 + 3] as f32 * w00
                    + rgba[i10 + 3] as f32 * w10
                    + rgba[i01 + 3] as f32 * w01
                    + rgba[i11 + 3] as f32 * w11;
                let a = sa / 255.0;
                if a > 0.005 {
                    let sr = rgba[i00] as f32 * w00
                        + rgba[i10] as f32 * w10
                        + rgba[i01] as f32 * w01
                        + rgba[i11] as f32 * w11;
                    let sg = rgba[i00 + 1] as f32 * w00
                        + rgba[i10 + 1] as f32 * w10
                        + rgba[i01 + 1] as f32 * w01
                        + rgba[i11 + 1] as f32 * w11;
                    let sb = rgba[i00 + 2] as f32 * w00
                        + rgba[i10 + 2] as f32 * w10
                        + rgba[i01 + 2] as f32 * w01
                        + rgba[i11 + 2] as f32 * w11;
                    self.blend_px(
                        px,
                        py,
                        sr.round().clamp(0.0, 255.0) as u8,
                        sg.round().clamp(0.0, 255.0) as u8,
                        sb.round().clamp(0.0, 255.0) as u8,
                        a,
                    );
                }
            }
        }
    }
}

unsafe fn make_font_face(dpi: i32, px_size: i32, weight: i32, face: PCWSTR) -> HFONT {
    let height = -sc(px_size, dpi);
    CreateFontW(
        height,
        0,
        0,
        0,
        weight,
        0,
        0,
        0,
        DEFAULT_CHARSET.0 as u32,
        OUT_DEFAULT_PRECIS.0 as u32,
        CLIP_DEFAULT_PRECIS.0 as u32,
        CLEARTYPE_QUALITY.0 as u32,
        (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
        face,
    )
}

unsafe fn draw_glass_text(
    canvas: &mut SdfCanvas<'_>,
    text_dc: HDC,
    text_buf: &mut [u32],
    text: &str,
    lx: i32,
    ly: i32,
    lw: i32,
    lh: i32,
    dpi: i32,
    font: HFONT,
    col: [u8; 3],
    alpha: f32,
    flags: DRAW_TEXT_FORMAT,
) {
    let x0 = sc(lx, dpi).clamp(0, canvas.w);
    let y0 = sc(ly, dpi).clamp(0, canvas.h);
    let x1 = sc(lx + lw, dpi).clamp(0, canvas.w);
    let y1 = sc(ly + lh, dpi).clamp(0, canvas.h);
    if x1 <= x0 || y1 <= y0 {
        return;
    }

    for py in y0..y1 {
        let off = (py as usize) * (canvas.w as usize);
        text_buf[off + x0 as usize..off + x1 as usize].fill(0);
    }

    let old_font = SelectObject(text_dc, HGDIOBJ(font.0));
    SetTextColor(text_dc, rgb(255, 255, 255));
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    let mut rc = RECT {
        left: x0,
        top: y0,
        right: x1,
        bottom: y1,
    };
    DrawTextW(text_dc, &mut wide, &mut rc, flags | DT_NOPREFIX);
    SelectObject(text_dc, old_font);
    let _ = GdiFlush();

    for py in y0..y1 {
        let off = (py as usize) * (canvas.w as usize);
        for px in x0..x1 {
            let pix = text_buf[off + px as usize];
            if (pix & 0x00FF_FFFF) != 0 {
                let r = ((pix >> 16) & 0xFF) as f32;
                let g = ((pix >> 8) & 0xFF) as f32;
                let b = (pix & 0xFF) as f32;
                let lum = (r * 0.299 + g * 0.587 + b * 0.114) / 255.0;
                if lum > 0.004 {
                    canvas.blend_px(px, py, col[0], col[1], col[2], alpha * lum);
                }
            }
        }
    }
}

unsafe fn paint_control_panel(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut client = RECT::default();
    let _ = GetClientRect(hwnd, &mut client);
    let cw = client.right - client.left;
    let ch = client.bottom - client.top;
    if cw <= 0 || ch <= 0 {
        let _ = EndPaint(hwnd, &ps);
        return;
    }

    let dpi = get_window_dpi(hwnd);
    let s = dpi as f32 / 96.0;

    if !ensure_paint_cache(hdc, cw, ch, dpi) {
        let _ = EndPaint(hwnd, &ps);
        return;
    }

    let mem_dc = PAINT_CACHE.mem_dc;
    let text_dc = PAINT_CACHE.text_dc;
    let total_px = (cw as usize) * (ch as usize);
    let main_buf = std::slice::from_raw_parts_mut(PAINT_CACHE.main_ptr, total_px);
    let text_buf = std::slice::from_raw_parts_mut(PAINT_CACHE.text_ptr, total_px);

    let tr = STATE.lang_tr;
    let active = STATE.mode != Mode::Off;
    let hover = STATE.hover_item;
    let rem = STATE.remaining_secs;
    let in_settings = STATE.in_settings;
    let close_to_tray = STATE.close_to_tray;
    let vx_i = (STATE.anim_view_slide * 12.0).round() as i32;
    let vx = vx_i as f32;

    let font_title_disp = PAINT_CACHE.font_title_disp;
    let font_section = PAINT_CACHE.font_section;
    let font_body = PAINT_CACHE.font_body;
    let font_body_bold = PAINT_CACHE.font_body_bold;
    let font_sub = PAINT_CACHE.font_sub;
    let font_caption = PAINT_CACHE.font_caption;
    let font_fluent = PAINT_CACHE.font_fluent;
    let font_fluent_gear = PAINT_CACHE.font_fluent_gear;
    let font_fluent_sm = PAINT_CACHE.font_fluent_sm;

    {
        let mut canvas = SdfCanvas {
            buf: main_buf,
            w: cw,
            h: ch,
            s,
        };

        // 1. WinUI 3 Dark Surface Base (#1F1F1F with subtle Mica transparency)
        canvas.fill_winui_base();

        let mug_rgba = if active { MUG_AWAKE_RGBA } else { MUG_SLEEP_RGBA };

        // 2. Title Bar (y = 0..38)
        if !in_settings {
            canvas.draw_rgba_icon(mug_rgba, 16.0, 9.0, 20.0, 20.0);
        } else if hover == HIT_BTN_BACK {
            canvas.draw_round_card(12.0, 5.0, 32.0, 28.0, 4.0, [46, 46, 46], 1.0, [46, 46, 46], 0.0);
        }

        // Settings Gear Button (236, 5, 36, 28)
        if in_settings {
            canvas.draw_round_card(236.0, 5.0, 36.0, 28.0, 4.0, [52, 52, 52], 1.0, [68, 68, 68], 1.0);
            canvas.draw_round_card(246.0, 30.0, 16.0, 2.5, 1.25, [76, 194, 255], 1.0, [76, 194, 255], 0.0);
        } else if hover == HIT_BTN_SETTINGS {
            canvas.draw_round_card(236.0, 5.0, 36.0, 28.0, 4.0, [46, 46, 46], 1.0, [46, 46, 46], 0.0);
        }
        // Minimize Caption Button (276, 5, 36, 28)
        if hover == HIT_BTN_HIDE {
            canvas.draw_round_card(276.0, 5.0, 36.0, 28.0, 4.0, [46, 46, 46], 1.0, [46, 46, 46], 0.0);
        }
        // Close Caption Button (316, 5, 36, 28)
        if hover == HIT_BTN_EXIT {
            canvas.draw_round_card(316.0, 5.0, 36.0, 28.0, 4.0, [196, 43, 28], 1.0, [196, 43, 28], 0.0);
        }

        // Title bar text & Fluent caption icons
        if !in_settings {
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                "AlwaysOn",
                44,
                9,
                150,
                20,
                dpi,
                font_caption,
                [230, 230, 230],
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );
        } else {
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                "\u{E72B}",
                12,
                5,
                32,
                28,
                dpi,
                font_fluent_gear,
                [230, 230, 230],
                1.0,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                if tr { "Ayarlar" } else { "Settings" },
                48,
                8,
                160,
                22,
                dpi,
                font_body_bold,
                [245, 245, 245],
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );
        }

        // Caption glyphs: Settings (\uE713), Minimize (\uE921), Close (\uE8BB)
        draw_glass_text(
            &mut canvas,
            text_dc,
            text_buf,
            "\u{E713}",
            236,
            5,
            36,
            28,
            dpi,
            font_fluent_gear,
            if in_settings { [76, 194, 255] } else { [214, 214, 214] },
            1.0,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        draw_glass_text(
            &mut canvas,
            text_dc,
            text_buf,
            "\u{E921}",
            276,
            5,
            36,
            28,
            dpi,
            font_fluent_sm,
            [214, 214, 214],
            1.0,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        draw_glass_text(
            &mut canvas,
            text_dc,
            text_buf,
            "\u{E8BB}",
            316,
            5,
            36,
            28,
            dpi,
            font_fluent_sm,
            if hover == HIT_BTN_EXIT { [255, 255, 255] } else { [214, 214, 214] },
            1.0,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );

        if in_settings {
            // === SETTINGS VIEW ===
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                if tr { "Başlangıç" } else { "Startup" },
                18 + vx_i,
                44,
                200,
                18,
                dpi,
                font_section,
                [158, 158, 158],
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );

            let auto_fill = if hover == HIT_TOGGLE_AUTOSTART { [50, 50, 50] } else { [43, 43, 43] };
            canvas.draw_round_card(16.0 + vx, 66.0, 328.0, 62.0, 8.0, auto_fill, 1.0, [58, 58, 58], 1.0);
            canvas.draw_winui_toggle(286.0 + vx, 87.0, 40.0, 20.0, STATE.anim_auto_toggle);

            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                "\u{E7B5}",
                28 + vx_i,
                66,
                24,
                62,
                dpi,
                font_fluent,
                [224, 224, 224],
                1.0,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                if tr { "Windows başlangıcında çalıştır" } else { "Run at Windows startup" },
                60 + vx_i,
                76,
                216,
                20,
                dpi,
                font_body,
                [243, 243, 243],
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                if tr { "Oturum açıldığında otomatik başlar" } else { "Launch automatically at sign-in" },
                60 + vx_i,
                96,
                216,
                16,
                dpi,
                font_sub,
                [158, 158, 158],
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );

            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                if tr { "Kapatma Tuşu (\u{2715}) Davranışı" } else { "Close Button (\u{2715}) Behavior" },
                18 + vx_i,
                142,
                260,
                18,
                dpi,
                font_section,
                [158, 158, 158],
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );

            canvas.draw_round_card(16.0 + vx, 164.0, 328.0, 122.0, 8.0, [43, 43, 43], 1.0, [58, 58, 58], 1.0);
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                "\u{E8BB}",
                28 + vx_i,
                174,
                24,
                38,
                dpi,
                font_fluent_gear,
                [224, 224, 224],
                1.0,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                if tr { "Kapatma tuşuna basıldığında" } else { "When clicking the close button" },
                60 + vx_i,
                174,
                268,
                20,
                dpi,
                font_body,
                [243, 243, 243],
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );
            let close_desc = if close_to_tray {
                if tr {
                    "Sistem tepsisine küçültülür (arka planda çalışır)"
                } else {
                    "Minimizes to system tray (runs in background)"
                }
            } else if tr {
                "Uygulama tamamen kapatılır"
            } else {
                "Closes the application completely"
            };
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                close_desc,
                60 + vx_i,
                194,
                268,
                16,
                dpi,
                font_sub,
                [96, 205, 255],
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );

            canvas.draw_round_card(28.0 + vx, 224.0, 304.0, 46.0, 6.0, [32, 32, 32], 1.0, [54, 54, 54], 1.0);
            let close_opts: [(usize, bool, f32, &str); 2] = [
                (
                    HIT_CLOSE_TO_TRAY,
                    true,
                    32.0,
                    if tr { "Alta Al (Tepsi)" } else { "Minimize to Tray" },
                ),
                (
                    HIT_CLOSE_EXIT,
                    false,
                    182.0,
                    if tr { "Uygulamayı Kapat" } else { "Exit Application" },
                ),
            ];
            for &(hid, is_tray_opt, ox, _) in &close_opts {
                if hover == hid && close_to_tray != is_tray_opt {
                    canvas.draw_round_card(ox + vx, 228.0, 146.0, 38.0, 5.0, [46, 46, 46], 1.0, [46, 46, 46], 0.0);
                }
            }
            let opt_x = 32.0 + STATE.anim_close_opt * 150.0 + vx;
            canvas.draw_round_card(opt_x, 228.0, 146.0, 38.0, 5.0, [56, 56, 56], 1.0, [70, 70, 70], 1.0);
            canvas.draw_round_card(opt_x + 53.0, 261.0, 40.0, 3.0, 1.5, [76, 194, 255], 1.0, [76, 194, 255], 0.0);

            for &(_, is_tray_opt, ox, label) in &close_opts {
                let selected = close_to_tray == is_tray_opt;
                draw_glass_text(
                    &mut canvas,
                    text_dc,
                    text_buf,
                    label,
                    ox as i32 + vx_i,
                    227,
                    146,
                    34,
                    dpi,
                    if selected { font_body_bold } else { font_body },
                    if selected { [255, 255, 255] } else { [196, 196, 196] },
                    1.0,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
            }
        } else {
            // === MAIN CONTROL VIEW ===
            // 3. Hero SettingsCard (16, 44, 328, 72, r = 8)
            let hero_fill = if hover == HIT_HERO { [50, 50, 50] } else { [43, 43, 43] };
            let hero_border = if active { [56, 120, 168] } else { [58, 58, 58] };
            canvas.draw_round_card(16.0 + vx, 44.0, 328.0, 72.0, 8.0, hero_fill, 1.0, hero_border, 1.0);
            canvas.draw_rgba_icon(mug_rgba, 28.0 + vx, 56.0, 48.0, 48.0);
            canvas.draw_winui_toggle(286.0 + vx, 70.0, 40.0, 20.0, STATE.anim_hero_toggle);

            // 4. WinUI 3 SelectorBar / Segmented Control (16, 150, 328, 44, r = 8)
            canvas.draw_round_card(16.0 + vx, 150.0, 328.0, 44.0, 8.0, [43, 43, 43], 1.0, [58, 58, 58], 1.0);
            let segs: [(usize, Mode, f32, &str); 4] = [
                (HIT_MODE_FOREVER, Mode::Forever, 20.0, if tr { "Süresiz" } else { "Always" }),
                (HIT_MODE_30M, Mode::Timer30M, 100.0, if tr { "30 Dakika" } else { "30 Min" }),
                (HIT_MODE_1H, Mode::Timer1H, 180.0, if tr { "1 Saat" } else { "1 Hour" }),
                (HIT_MODE_3H, Mode::Timer3H, 260.0, if tr { "3 Saat" } else { "3 Hours" }),
            ];
            for &(hid, mode, sx, _) in &segs {
                if hover == hid && STATE.mode != mode {
                    canvas.draw_round_card(sx + vx, 154.0, 78.0, 36.0, 6.0, [50, 50, 50], 1.0, [50, 50, 50], 0.0);
                }
            }
            if STATE.anim_mode_alpha > 0.01 {
                let pill_x = 20.0 + STATE.anim_mode_idx * 80.0 + vx;
                canvas.draw_round_card(
                    pill_x,
                    154.0,
                    78.0,
                    36.0,
                    6.0,
                    [56, 56, 56],
                    STATE.anim_mode_alpha,
                    [70, 70, 70],
                    STATE.anim_mode_alpha,
                );
                canvas.draw_round_card(
                    pill_x + 27.0,
                    185.0,
                    24.0,
                    3.0,
                    1.5,
                    [76, 194, 255],
                    STATE.anim_mode_alpha,
                    [76, 194, 255],
                    0.0,
                );
            }

            // 5. Display SettingsCard (16, 228, 328, 58, r = 8)
            let disp_fill = if hover == HIT_TOGGLE_DISPLAY { [50, 50, 50] } else { [43, 43, 43] };
            canvas.draw_round_card(16.0 + vx, 228.0, 328.0, 58.0, 8.0, disp_fill, 1.0, [58, 58, 58], 1.0);
            canvas.draw_winui_toggle(286.0 + vx, 247.0, 40.0, 20.0, STATE.anim_disp_toggle);

            // Hero Card Title & Status
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                if tr { "Uyku Engelleme" } else { "Sleep Prevention" },
                88 + vx_i,
                58,
                188,
                22,
                dpi,
                font_title_disp,
                [255, 255, 255],
                1.0,
                DT_LEFT | DT_SINGLELINE,
            );
            let hero_sub = if active && rem > 0 {
                let h = rem / 3600;
                let m = (rem % 3600) / 60;
                let sec = rem % 60;
                if tr {
                    format!("Kalan süre: {h:02}:{m:02}:{sec:02}")
                } else {
                    format!("Remaining: {h:02}:{m:02}:{sec:02}")
                }
            } else if active {
                if tr {
                    "Aktif • Süresiz açık".to_string()
                } else {
                    "Active • Always on".to_string()
                }
            } else if tr {
                "Kapalı • Normal uyku modu".to_string()
            } else {
                "Off • Normal sleep mode".to_string()
            };
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                &hero_sub,
                88 + vx_i,
                82,
                188,
                18,
                dpi,
                font_sub,
                if active { [96, 205, 255] } else { [158, 158, 158] },
                1.0,
                DT_LEFT | DT_SINGLELINE,
            );

            // Section Label 1
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                if tr { "Çalışma Modu" } else { "Duration Mode" },
                18 + vx_i,
                128,
                200,
                18,
                dpi,
                font_section,
                [158, 158, 158],
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );

            // SelectorBar labels
            for &(_, mode, sx, label) in &segs {
                let selected = STATE.mode == mode;
                draw_glass_text(
                    &mut canvas,
                    text_dc,
                    text_buf,
                    label,
                    sx as i32 + vx_i,
                    153,
                    78,
                    32,
                    dpi,
                    if selected { font_body_bold } else { font_body },
                    if selected { [255, 255, 255] } else { [204, 204, 204] },
                    1.0,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
            }

            // Section Label 2
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                if tr { "Ekran ve Güç" } else { "Display & Power" },
                18 + vx_i,
                206,
                200,
                18,
                dpi,
                font_section,
                [158, 158, 158],
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );

            // Display SettingsCard text & icon (\uE7F4)
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                "\u{E7F4}",
                28 + vx_i,
                228,
                24,
                58,
                dpi,
                font_fluent,
                [224, 224, 224],
                1.0,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                if tr { "Ekranı da açık tut" } else { "Keep display awake" },
                60 + vx_i,
                236,
                216,
                20,
                dpi,
                font_body,
                [243, 243, 243],
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                if tr { "Monitörün kapanmasını engeller" } else { "Prevents monitor from turning off" },
                60 + vx_i,
                256,
                216,
                16,
                dpi,
                font_sub,
                [158, 158, 158],
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );
        }
    }

    let _ = BitBlt(hdc, 0, 0, cw, ch, mem_dc, 0, 0, SRCCOPY);
    let _ = EndPaint(hwnd, &ps);
}

fn main() -> Result<()> {
    unsafe {
        let mutex = CreateMutexW(None, true, w!("Local\\AlwaysOn_Tray_Mutex"));
        if let Ok(existing) = FindWindowW(w!("AlwaysOnClass"), None) {
            if !existing.0.is_null() {
                let _ = PostMessageW(existing, WM_USER_SHOW_UI, WPARAM(0), LPARAM(0));
                if let Ok(h) = mutex {
                    let _ = CloseHandle(h);
                }
                return Ok(());
            }
        }

        STATE.lang_tr = (GetUserDefaultUILanguage() & 0x3FF) == 0x1F;
        load_app_settings();
        STATE.in_settings = std::env::args().any(|a| a == "--settings");
        let instance = GetModuleHandleW(None)?;

        if std::env::args().any(|a| a == "--install-autostart") {
            enable_autostart(true);
        } else {
            sync_autostart_if_configured();
        }
        let launched_at_startup = std::env::args().any(|a| a == "--startup");

        let sm_icon_cx = GetSystemMetrics(SM_CXSMICON);
        STATE.hicon_awake = load_icon_sized(instance, 1, sm_icon_cx);
        STATE.hicon_sleep = load_icon_sized(instance, 2, sm_icon_cx);
        STATE.hicon_hero_awake = load_icon_sized(instance, 1, 64);
        STATE.hicon_hero_sleep = load_icon_sized(instance, 2, 64);
        STATE.wm_taskbar_created = RegisterWindowMessageW(w!("TaskbarCreated"));

        let class_name = w!("AlwaysOnClass");

        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: class_name,
            hIcon: STATE.hicon_hero_awake,
            ..Default::default()
        };

        let _ = RegisterClassW(&wc);

        let title = if STATE.lang_tr {
            w!("AlwaysOn Kontrol Paneli")
        } else {
            w!("AlwaysOn Control Panel")
        };

        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            class_name,
            title,
            WS_POPUP,
            0,
            0,
            UI_LOGICAL_W,
            UI_LOGICAL_H,
            None,
            None,
            instance,
            None,
        )?;

        apply_dwm_winui(hwnd);
        let _ = SendMessageW(
            hwnd,
            WM_SETICON,
            WPARAM(ICON_BIG as usize),
            LPARAM(STATE.hicon_hero_awake.0 as isize),
        );
        let _ = SendMessageW(
            hwnd,
            WM_SETICON,
            WPARAM(ICON_SMALL as usize),
            LPARAM(STATE.hicon_awake.0 as isize),
        );

        if !add_or_update_tray_icon(hwnd) {
            let _ = SetTimer(hwnd, RETRY_TRAY_TIMER_ID, 2000, None);
        }

        apply_execution_state(true);
        let _ = SetTimer(hwnd, HEARTBEAT_TIMER_ID, 30_000, None);

        if !launched_at_startup {
            show_control_panel(hwnd);
        } else {
            trim_working_set();
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = KillTimer(hwnd, ANIM_TIMER_ID);
        let _ = KillTimer(hwnd, HEARTBEAT_TIMER_ID);
        let _ = KillTimer(hwnd, RETRY_TRAY_TIMER_ID);
        let _ = KillTimer(hwnd, UI_TICK_TIMER_ID);
        let _ = KillTimer(hwnd, TIMER_ID);
        release_paint_cache();

        let mut nid: NOTIFYICONDATAW = mem::zeroed();
        nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = hwnd;
        nid.uID = ID_TRAY_ICON;
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid);

        apply_execution_state(false);
        if let Ok(h) = mutex {
            let _ = CloseHandle(h);
        }
    }
    Ok(())
}

unsafe fn show_timeout_notification(hwnd: HWND) {
    let mut nid: NOTIFYICONDATAW = mem::zeroed();
    nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = ID_TRAY_ICON;
    nid.uFlags = NIF_INFO;
    set_string(&mut nid.szInfoTitle, "AlwaysOn");
    set_string(
        &mut nid.szInfo,
        if STATE.lang_tr {
            "Süre doldu, uyku engelleme kapatıldı."
        } else {
            "Timer ended, sleep prevention deactivated."
        },
    );
    nid.dwInfoFlags = NIIF_USER | NIIF_LARGE_ICON;
    let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if STATE.wm_taskbar_created != 0 && msg == STATE.wm_taskbar_created {
        if add_or_update_tray_icon(hwnd) {
            let _ = KillTimer(hwnd, RETRY_TRAY_TIMER_ID);
        }
        update_tray_and_power(hwnd, false);
        return LRESULT(0);
    }

    match msg {
        WM_USER_SHOW_UI => {
            show_control_panel(hwnd);
            LRESULT(0)
        }
        WM_USER_TRAYICON => {
            let lparam_lo = (lparam.0 & 0xFFFF) as u32;
            if lparam_lo == WM_LBUTTONUP {
                let recently_hidden = STATE.has_hidden_recently
                    && GetMessageTime().wrapping_sub(STATE.last_hide_time) < 250;
                STATE.has_hidden_recently = false;

                if !recently_hidden {
                    if IsWindowVisible(hwnd).as_bool() {
                        hide_control_panel(hwnd, false);
                    } else {
                        show_control_panel(hwnd);
                    }
                }
            } else if lparam_lo == WM_RBUTTONUP {
                show_context_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_KEYDOWN => {
            if wparam.0 == 0x1B {
                if STATE.in_settings {
                    STATE.in_settings = false;
                    STATE.anim_view_slide = -1.0;
                    STATE.hover_item = HIT_NONE;
                    start_ui_anim(hwnd);
                    let _ = InvalidateRect(hwnd, None, false);
                } else {
                    hide_control_panel(hwnd, false);
                }
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            paint_control_panel(hwnd);
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let dpi = get_window_dpi(hwnd);
            let lx = unscale((lparam.0 & 0xFFFF) as i16 as i32, dpi);
            let ly = unscale(((lparam.0 >> 16) & 0xFFFF) as i16 as i32, dpi);
            let hit = hit_test_ui(lx, ly);
            if hit != STATE.hover_item {
                STATE.hover_item = hit;
                let _ = InvalidateRect(hwnd, None, false);
            }
            LRESULT(0)
        }
        WM_NCMOUSEMOVE => {
            if STATE.hover_item != HIT_NONE {
                STATE.hover_item = HIT_NONE;
                let _ = InvalidateRect(hwnd, None, false);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_SETCURSOR => {
            if (lparam.0 & 0xFFFF) as u32 == HTCLIENT && STATE.hover_item != HIT_NONE {
                if let Ok(hand) = LoadCursorW(None, IDC_HAND) {
                    SetCursor(hand);
                    return LRESULT(1);
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_LBUTTONDOWN => {
            let dpi = get_window_dpi(hwnd);
            let lx = unscale((lparam.0 & 0xFFFF) as i16 as i32, dpi);
            let ly = unscale(((lparam.0 >> 16) & 0xFFFF) as i16 as i32, dpi);
            if hit_test_ui(lx, ly) == HIT_NONE {
                let _ = ReleaseCapture();
                let _ = SendMessageW(hwnd, WM_NCLBUTTONDOWN, WPARAM(HTCAPTION as usize), LPARAM(0));
                return LRESULT(0);
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
        }
        WM_LBUTTONUP => {
            let dpi = get_window_dpi(hwnd);
            let lx = unscale((lparam.0 & 0xFFFF) as i16 as i32, dpi);
            let ly = unscale(((lparam.0 >> 16) & 0xFFFF) as i16 as i32, dpi);
            match hit_test_ui(lx, ly) {
                HIT_HERO => {
                    if STATE.mode == Mode::Off {
                        set_mode(hwnd, Mode::Forever, false);
                    } else {
                        set_mode(hwnd, Mode::Off, false);
                    }
                }
                HIT_MODE_FOREVER => set_mode(hwnd, Mode::Forever, false),
                HIT_MODE_30M => set_mode(hwnd, Mode::Timer30M, false),
                HIT_MODE_1H => set_mode(hwnd, Mode::Timer1H, false),
                HIT_MODE_3H => set_mode(hwnd, Mode::Timer3H, false),
                HIT_TOGGLE_DISPLAY => {
                    STATE.keep_display_on = !STATE.keep_display_on;
                    write_reg_dword(REG_KEEP_DISPLAY_ON, if STATE.keep_display_on { 1 } else { 0 });
                    apply_execution_state(STATE.mode != Mode::Off);
                    start_ui_anim(hwnd);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                HIT_TOGGLE_AUTOSTART => toggle_autostart(hwnd),
                HIT_BTN_SETTINGS => {
                    STATE.in_settings = !STATE.in_settings;
                    STATE.anim_view_slide = if STATE.in_settings { 1.0 } else { -1.0 };
                    STATE.hover_item = HIT_NONE;
                    start_ui_anim(hwnd);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                HIT_BTN_BACK => {
                    STATE.in_settings = false;
                    STATE.anim_view_slide = -1.0;
                    STATE.hover_item = HIT_NONE;
                    start_ui_anim(hwnd);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                HIT_CLOSE_TO_TRAY => {
                    STATE.close_to_tray = true;
                    write_reg_dword(REG_CLOSE_TO_TRAY, 1);
                    start_ui_anim(hwnd);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                HIT_CLOSE_EXIT => {
                    STATE.close_to_tray = false;
                    write_reg_dword(REG_CLOSE_TO_TRAY, 0);
                    start_ui_anim(hwnd);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                HIT_BTN_HIDE => hide_control_panel(hwnd, false),
                HIT_BTN_EXIT => {
                    if STATE.close_to_tray {
                        hide_control_panel(hwnd, false);
                    } else {
                        PostQuitMessage(0);
                    }
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = wparam.0 & 0xFFFF;
            match id as usize {
                IDM_OPEN_UI => show_control_panel(hwnd),
                IDM_OFF => set_mode(hwnd, Mode::Off, true),
                IDM_FOREVER => set_mode(hwnd, Mode::Forever, true),
                IDM_30M => set_mode(hwnd, Mode::Timer30M, true),
                IDM_1H => set_mode(hwnd, Mode::Timer1H, true),
                IDM_3H => set_mode(hwnd, Mode::Timer3H, true),
                IDM_AUTOSTART => toggle_autostart(hwnd),
                IDM_EXIT => PostQuitMessage(0),
                _ => {}
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == ANIM_TIMER_ID {
                step_ui_anim(hwnd);
            } else if wparam.0 == UI_TICK_TIMER_ID {
                if STATE.remaining_secs > 1 {
                    STATE.remaining_secs -= 1;
                    let visible = IsWindowVisible(hwnd).as_bool();
                    if visible || STATE.remaining_secs % 15 == 0 {
                        update_tray_tooltip_only(hwnd);
                    }
                    if visible {
                        let _ = InvalidateRect(hwnd, None, false);
                    }
                } else if STATE.mode != Mode::Off && STATE.mode != Mode::Forever {
                    let _ = KillTimer(hwnd, UI_TICK_TIMER_ID);
                    set_mode(hwnd, Mode::Off, false);
                    show_timeout_notification(hwnd);
                }
            } else if wparam.0 == TIMER_ID {
                let _ = KillTimer(hwnd, TIMER_ID);
                let _ = KillTimer(hwnd, UI_TICK_TIMER_ID);
                set_mode(hwnd, Mode::Off, false);
                show_timeout_notification(hwnd);
            } else if wparam.0 == HEARTBEAT_TIMER_ID {
                if STATE.mode != Mode::Off {
                    apply_execution_state(true);
                }
            } else if wparam.0 == RETRY_TRAY_TIMER_ID {
                if add_or_update_tray_icon(hwnd) {
                    let _ = KillTimer(hwnd, RETRY_TRAY_TIMER_ID);
                }
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            if STATE.close_to_tray {
                hide_control_panel(hwnd, false);
            } else {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            release_paint_cache();
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe fn update_tray_tooltip_only(hwnd: HWND) {
    let mut nid: NOTIFYICONDATAW = mem::zeroed();
    nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = ID_TRAY_ICON;
    nid.uFlags = NIF_TIP;
    set_string(&mut nid.szTip, &tray_tooltip_text());
    let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
}

unsafe fn set_mode(hwnd: HWND, new_mode: Mode, notify: bool) {
    let _ = KillTimer(hwnd, TIMER_ID);
    let _ = KillTimer(hwnd, UI_TICK_TIMER_ID);
    STATE.mode = new_mode;

    match STATE.mode {
        Mode::Timer30M => {
            STATE.remaining_secs = 30 * 60;
            let _ = SetTimer(hwnd, UI_TICK_TIMER_ID, 1000, None);
        }
        Mode::Timer1H => {
            STATE.remaining_secs = 3600;
            let _ = SetTimer(hwnd, UI_TICK_TIMER_ID, 1000, None);
        }
        Mode::Timer3H => {
            STATE.remaining_secs = 3 * 3600;
            let _ = SetTimer(hwnd, UI_TICK_TIMER_ID, 1000, None);
        }
        Mode::Off | Mode::Forever => {
            STATE.remaining_secs = 0;
        }
    }

    update_tray_and_power(hwnd, notify);
    start_ui_anim(hwnd);
    let _ = InvalidateRect(hwnd, None, false);
}

unsafe fn update_tray_and_power(hwnd: HWND, notify: bool) {
    let mut nid: NOTIFYICONDATAW = mem::zeroed();
    nid.cbSize = mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = ID_TRAY_ICON;
    nid.uFlags = NIF_ICON | NIF_TIP | if notify { NIF_INFO } else { Default::default() };

    let is_active = STATE.mode != Mode::Off;
    apply_execution_state(is_active);
    if is_active {
        let _ = SetTimer(hwnd, HEARTBEAT_TIMER_ID, 30_000, None);
    } else {
        let _ = KillTimer(hwnd, HEARTBEAT_TIMER_ID);
    }

    if is_active {
        nid.hIcon = STATE.hicon_awake;
        if notify {
            set_string(
                &mut nid.szInfo,
                if STATE.lang_tr {
                    "Uyku engelleme aktif."
                } else {
                    "AlwaysOn activated."
                },
            );
        }
    } else {
        nid.hIcon = STATE.hicon_sleep;
        if notify {
            set_string(
                &mut nid.szInfo,
                if STATE.lang_tr {
                    "Uyku engelleme devre dışı."
                } else {
                    "AlwaysOn deactivated."
                },
            );
        }
    }
    set_string(&mut nid.szTip, &tray_tooltip_text());

    if notify {
        set_string(&mut nid.szInfoTitle, "AlwaysOn");
        nid.dwInfoFlags = NIIF_USER | NIIF_LARGE_ICON;
    }

    if !Shell_NotifyIconW(NIM_MODIFY, &nid).as_bool() {
        let _ = add_or_update_tray_icon(hwnd);
    }

    let win_icon = if is_active {
        STATE.hicon_awake
    } else {
        STATE.hicon_sleep
    };
    let _ = SendMessageW(
        hwnd,
        WM_SETICON,
        WPARAM(ICON_SMALL as usize),
        LPARAM(win_icon.0 as isize),
    );
}

unsafe fn show_context_menu(hwnd: HWND) {
    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);

    if let Ok(hmenu) = CreatePopupMenu() {
        let txt_open = if STATE.lang_tr {
            w!("Kontrol Panelini Aç")
        } else {
            w!("Open Control Panel")
        };
        let txt_off = if STATE.lang_tr {
            w!("Kapalı (Normal Uyku)")
        } else {
            w!("Off (Normal Sleep)")
        };
        let txt_forever = if STATE.lang_tr {
            w!("Sürekli Açık Tut")
        } else {
            w!("Always On")
        };
        let txt_30m = if STATE.lang_tr {
            w!("30 Dakika Açık Tut")
        } else {
            w!("Keep On for 30 Minutes")
        };
        let txt_1h = if STATE.lang_tr {
            w!("1 Saat Açık Tut")
        } else {
            w!("Keep On for 1 Hour")
        };
        let txt_3h = if STATE.lang_tr {
            w!("3 Saat Açık Tut")
        } else {
            w!("Keep On for 3 Hours")
        };
        let txt_auto = if STATE.lang_tr {
            w!("Başlangıçta Otomatik Çalıştır")
        } else {
            w!("Run at Startup")
        };
        let txt_exit = if STATE.lang_tr {
            w!("Çıkış")
        } else {
            w!("Exit")
        };

        let _ = InsertMenuW(hmenu, 0xFFFFFFFF, MF_BYPOSITION | MF_STRING, IDM_OPEN_UI, txt_open);
        let _ = InsertMenuW(
            hmenu,
            0xFFFFFFFF,
            MF_BYPOSITION | MF_SEPARATOR,
            0,
            PCWSTR(std::ptr::null()),
        );

        let _ = InsertMenuW(hmenu, 0xFFFFFFFF, MF_BYPOSITION | MF_STRING, IDM_OFF, txt_off);
        let _ = InsertMenuW(
            hmenu,
            0xFFFFFFFF,
            MF_BYPOSITION | MF_STRING,
            IDM_FOREVER,
            txt_forever,
        );
        let _ = InsertMenuW(hmenu, 0xFFFFFFFF, MF_BYPOSITION | MF_STRING, IDM_30M, txt_30m);
        let _ = InsertMenuW(hmenu, 0xFFFFFFFF, MF_BYPOSITION | MF_STRING, IDM_1H, txt_1h);
        let _ = InsertMenuW(hmenu, 0xFFFFFFFF, MF_BYPOSITION | MF_STRING, IDM_3H, txt_3h);

        let active_pos = match STATE.mode {
            Mode::Off => 2,
            Mode::Forever => 3,
            Mode::Timer30M => 4,
            Mode::Timer1H => 5,
            Mode::Timer3H => 6,
        };
        let _ = CheckMenuRadioItem(hmenu, 2, 6, active_pos, MF_BYPOSITION.0);

        let _ = InsertMenuW(
            hmenu,
            0xFFFFFFFF,
            MF_BYPOSITION | MF_SEPARATOR,
            0,
            PCWSTR(std::ptr::null()),
        );

        let flag_auto = if is_autostart() {
            MF_CHECKED
        } else {
            MF_UNCHECKED
        };
        let _ = InsertMenuW(
            hmenu,
            0xFFFFFFFF,
            MF_BYPOSITION | MF_STRING | flag_auto,
            IDM_AUTOSTART,
            txt_auto,
        );

        let _ = InsertMenuW(
            hmenu,
            0xFFFFFFFF,
            MF_BYPOSITION | MF_SEPARATOR,
            0,
            PCWSTR(std::ptr::null()),
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static STATE_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct RegRestoreGuard {
        orig_close: u32,
        orig_disp: u32,
        had_close: bool,
        had_disp: bool,
    }

    unsafe fn reg_has_value(name: PCWSTR) -> Option<u32> {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, SETTINGS_KEY_PATH, 0, KEY_READ, &mut hkey).is_err() {
            return None;
        }
        let mut val: u32 = 0;
        let mut len: u32 = mem::size_of::<u32>() as u32;
        let mut kind = REG_VALUE_TYPE::default();
        let ok = RegQueryValueExW(
            hkey,
            name,
            None,
            Some(&mut kind),
            Some(&mut val as *mut u32 as *mut u8),
            Some(&mut len),
        )
        .is_ok()
            && kind == REG_DWORD;
        let _ = RegCloseKey(hkey);
        if ok {
            Some(val)
        } else {
            None
        }
    }

    impl RegRestoreGuard {
        unsafe fn capture() -> Self {
            let c = reg_has_value(REG_CLOSE_TO_TRAY);
            let d = reg_has_value(REG_KEEP_DISPLAY_ON);
            Self {
                orig_close: c.unwrap_or(1),
                orig_disp: d.unwrap_or(1),
                had_close: c.is_some(),
                had_disp: d.is_some(),
            }
        }
    }

    impl Drop for RegRestoreGuard {
        fn drop(&mut self) {
            unsafe {
                if self.had_close {
                    write_reg_dword(REG_CLOSE_TO_TRAY, self.orig_close);
                } else {
                    write_reg_dword(REG_CLOSE_TO_TRAY, 1);
                }
                if self.had_disp {
                    write_reg_dword(REG_KEEP_DISPLAY_ON, self.orig_disp);
                } else {
                    write_reg_dword(REG_KEEP_DISPLAY_ON, 1);
                }
                load_app_settings();
            }
        }
    }

    #[test]
    fn test_scenario_hit_testing_main_and_settings_views() {
        let _lock = STATE_TEST_LOCK.lock().unwrap();
        unsafe {
            let orig_settings = STATE.in_settings;
            STATE.in_settings = false;

            // Main view caption + controls
            assert_eq!(hit_test_ui(254, 15), HIT_BTN_SETTINGS);
            assert_eq!(hit_test_ui(294, 15), HIT_BTN_HIDE);
            assert_eq!(hit_test_ui(334, 15), HIT_BTN_EXIT);
            assert_eq!(hit_test_ui(120, 80), HIT_HERO);
            assert_eq!(hit_test_ui(55, 170), HIT_MODE_FOREVER);
            assert_eq!(hit_test_ui(135, 170), HIT_MODE_30M);
            assert_eq!(hit_test_ui(215, 170), HIT_MODE_1H);
            assert_eq!(hit_test_ui(295, 170), HIT_MODE_3H);
            assert_eq!(hit_test_ui(120, 250), HIT_TOGGLE_DISPLAY);
            assert_eq!(hit_test_ui(120, 130), HIT_NONE);

            // Settings view caption + startup toggle + close behavior options
            STATE.in_settings = true;
            assert_eq!(hit_test_ui(24, 18), HIT_BTN_BACK);
            assert_eq!(hit_test_ui(254, 15), HIT_BTN_SETTINGS);
            assert_eq!(hit_test_ui(120, 95), HIT_TOGGLE_AUTOSTART);
            assert_eq!(hit_test_ui(100, 245), HIT_CLOSE_TO_TRAY);
            assert_eq!(hit_test_ui(250, 245), HIT_CLOSE_EXIT);
            // Main-only selector bar area should be HIT_NONE while in Settings
            assert_eq!(hit_test_ui(135, 170), HIT_NONE);

            STATE.in_settings = orig_settings;
        }
    }

    #[test]
    fn test_scenario_animation_convergence_and_sdf_rendering() {
        let mut val = 0.0f32;
        for _ in 0..30 {
            lerp_step(&mut val, 3.0, 0.34);
        }
        assert!((val - 3.0).abs() < f32::EPSILON);

        let mut val_zero = 1.0f32;
        for _ in 0..30 {
            lerp_step(&mut val_zero, 0.0, 0.34);
        }
        assert!((val_zero - 0.0).abs() < f32::EPSILON);

        assert_eq!(mode_to_index(Mode::Forever, 2.0), 0.0);
        assert_eq!(mode_to_index(Mode::Timer30M, 0.0), 1.0);
        assert_eq!(mode_to_index(Mode::Timer1H, 0.0), 2.0);
        assert_eq!(mode_to_index(Mode::Timer3H, 0.0), 3.0);
        assert_eq!(mode_to_index(Mode::Off, 2.0), 2.0);

        let w = UI_LOGICAL_W;
        let h = UI_LOGICAL_H;
        let mut buf = vec![0u32; (w * h) as usize];
        let mut canvas = SdfCanvas {
            buf: &mut buf,
            w,
            h,
            s: 1.0,
        };
        canvas.fill_winui_base();
        let base_corner = canvas.buf[0];
        assert_eq!((base_corner >> 24) & 0xFF, 240);

        // Draw opaque card covering (16, 44, 328, 72) and verify fast interior fill exactness
        canvas.draw_round_card(16.0, 44.0, 328.0, 72.0, 8.0, [43, 43, 43], 1.0, [58, 58, 58], 1.0);
        let inside_idx = (80 * w + 180) as usize;
        assert_eq!(canvas.buf[inside_idx], 0xFF_2B_2B_2B);

        // Draw embedded awake/sleep mug icons + animated toggle states
        canvas.draw_rgba_icon(MUG_AWAKE_RGBA, 28.0, 56.0, 48.0, 48.0);
        canvas.draw_rgba_icon(MUG_SLEEP_RGBA, 28.0, 120.0, 32.0, 32.0);
        canvas.draw_winui_toggle(286.0, 70.0, 40.0, 20.0, 0.0);
        canvas.draw_winui_toggle(286.0, 140.0, 40.0, 20.0, 0.5);
        canvas.draw_winui_toggle(286.0, 210.0, 40.0, 20.0, 1.0);

        let mug_center = canvas.buf[(80 * w + 52) as usize];
        assert_ne!(mug_center, 0xFF_2B_2B_2B);
        let toggle_on_track = canvas.buf[(220 * w + 292) as usize];
        let toggle_off_track = canvas.buf[(80 * w + 316) as usize];
        assert_ne!(toggle_on_track, toggle_off_track);
    }

    #[test]
    fn test_scenario_registry_settings_roundtrip() {
        let _lock = STATE_TEST_LOCK.lock().unwrap();
        unsafe {
            let _guard = RegRestoreGuard::capture();

            write_reg_dword(REG_CLOSE_TO_TRAY, 0);
            write_reg_dword(REG_KEEP_DISPLAY_ON, 0);
            load_app_settings();
            assert!(!STATE.close_to_tray);
            assert!(!STATE.keep_display_on);
            assert!((STATE.anim_close_opt - 1.0).abs() < f32::EPSILON);
            assert!((STATE.anim_disp_toggle - 0.0).abs() < f32::EPSILON);

            write_reg_dword(REG_CLOSE_TO_TRAY, 1);
            write_reg_dword(REG_KEEP_DISPLAY_ON, 1);
            load_app_settings();
            assert!(STATE.close_to_tray);
            assert!(STATE.keep_display_on);
            assert!((STATE.anim_close_opt - 0.0).abs() < f32::EPSILON);
            assert!((STATE.anim_disp_toggle - 1.0).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn test_scenario_exe_path_extraction() {
        let p1 = extract_exe_path(
            r#""C:\Users\yunusemre\AppData\Local\Programs\AlwaysOn\AlwaysOn.exe" --startup"#,
        );
        let p2 = extract_exe_path(
            r#"C:\Users\yunusemre\AppData\Local\Programs\AlwaysOn\alwayson.exe --startup"#,
        );
        assert!(paths_equal(&p1, &p2));

        let p3 = extract_exe_path(r#"  "D:\Tools\AlwaysOn.exe"  "#);
        assert_eq!(p3, PathBuf::from(r#"D:\Tools\AlwaysOn.exe"#));
    }
}
