#![windows_subsystem = "windows"]

use std::ffi::c_void;
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
const UI_LOGICAL_H: i32 = 388;

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
const HIT_LANG_BASE: usize = 14; // 14..=25 (System + 11 languages)
const HIT_THEME_SYSTEM: usize = 26;
const HIT_THEME_LIGHT: usize = 27;
const HIT_THEME_DARK: usize = 28;
const HIT_BTN_GITHUB: usize = 29;
const HIT_BTN_PORTABLE_INFO: usize = 30;
const HIT_FIRST_RUN_OK: usize = 31;

const RUN_KEY_PATH: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const STARTUP_APPROVED_PATH: PCWSTR =
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run");
const APP_REG_NAME: PCWSTR = w!("AlwaysOn");
const SETTINGS_KEY_PATH: PCWSTR = w!("Software\\AlwaysOn");
const REG_CLOSE_TO_TRAY: PCWSTR = w!("CloseToTray");
const REG_KEEP_DISPLAY_ON: PCWSTR = w!("KeepDisplayOn");
const REG_LANGUAGE: PCWSTR = w!("Language");
const REG_THEME: PCWSTR = w!("Theme");
const REG_FIRST_RUN_DONE: PCWSTR = w!("FirstRunShown");
const GITHUB_PROFILE_URL: PCWSTR = w!("https://github.com/yunusemreyl");
const PERSONALIZE_KEY_PATH: PCWSTR =
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
const REG_APPS_USE_LIGHT_THEME: PCWSTR = w!("AppsUseLightTheme");

#[derive(PartialEq, Clone, Copy, Debug)]
enum Mode {
    Off,
    Forever,
    Timer30M,
    Timer1H,
    Timer3H,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
enum ThemePref {
    System = 0,
    Light = 1,
    Dark = 2,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u32)]
enum LangPref {
    System = 0,
    Tr = 1,
    En = 2,
    De = 3,
    Fr = 4,
    Es = 5,
    It = 6,
    Pt = 7,
    Ru = 8,
    Zh = 9,
    Ja = 10,
    Ko = 11,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(usize)]
enum Lang {
    Tr = 0,
    En = 1,
    De = 2,
    Fr = 3,
    Es = 4,
    It = 5,
    Pt = 6,
    Ru = 7,
    Zh = 8,
    Ja = 9,
    Ko = 10,
}

const LANG_NATIVE_NAMES: [&str; 11] = [
    "Türkçe",
    "English",
    "Deutsch",
    "Français",
    "Español",
    "Italiano",
    "Português",
    "Русский",
    "中文",
    "日本語",
    "한국어",
];

struct UiStrings {
    window_title: PCWSTR,
    settings_title: &'static str,
    hero_title: &'static str,
    hero_active_forever: &'static str,
    hero_off: &'static str,
    hero_remaining_prefix: &'static str,
    section_mode: &'static str,
    mode_forever: &'static str,
    mode_30m: &'static str,
    mode_1h: &'static str,
    mode_3h: &'static str,
    section_display: &'static str,
    display_title: &'static str,
    display_sub: &'static str,
    section_startup: &'static str,
    startup_title: &'static str,
    startup_sub: &'static str,
    section_close: &'static str,
    close_desc_tray: &'static str,
    close_desc_exit: &'static str,
    close_opt_tray: &'static str,
    close_opt_exit: &'static str,
    section_theme: &'static str,
    theme_desc_system_dark: &'static str,
    theme_desc_system_light: &'static str,
    theme_desc_light: &'static str,
    theme_desc_dark: &'static str,
    theme_opt_light: &'static str,
    theme_opt_dark: &'static str,
    section_lang: &'static str,
    lang_desc_system_prefix: &'static str,
    lang_desc_selected: &'static str,
    lang_opt_system: &'static str,
    portable_title: &'static str,
    portable_body: &'static str,
    portable_ok: &'static str,
    portable_footer_tip: &'static str,
    tray_inactive: &'static str,
    tray_active: &'static str,
    notify_activated: &'static str,
    notify_deactivated: &'static str,
    notify_timeout: &'static str,
    menu_open: PCWSTR,
    menu_off: PCWSTR,
    menu_forever: PCWSTR,
    menu_30m: PCWSTR,
    menu_1h: PCWSTR,
    menu_3h: PCWSTR,
    menu_auto: PCWSTR,
    menu_exit: PCWSTR,
}

const UI_STRINGS: [UiStrings; 11] = [
    // 0: Turkish
    UiStrings {
        window_title: w!("AlwaysOn Kontrol Paneli"),
        settings_title: "Ayarlar",
        hero_title: "Uyku Engelleme",
        hero_active_forever: "Aktif • Süresiz açık",
        hero_off: "Kapalı • Normal uyku modu",
        hero_remaining_prefix: "Kalan süre",
        section_mode: "Çalışma Modu",
        mode_forever: "Süresiz",
        mode_30m: "30 Dakika",
        mode_1h: "1 Saat",
        mode_3h: "3 Saat",
        section_display: "Ekran ve Güç",
        display_title: "Ekranı da açık tut",
        display_sub: "Monitörün kapanmasını engeller",
        section_startup: "Başlangıç",
        startup_title: "Windows başlangıcında çalıştır",
        startup_sub: "Oturum açıldığında otomatik başlar",
        section_close: "Kapatma Tuşu (\u{2715})",
        close_desc_tray: "Sistem tepsisine küçültülür (arka planda)",
        close_desc_exit: "Uygulama tamamen kapatılır",
        close_opt_tray: "Alta Al (Tepsi)",
        close_opt_exit: "Uygulamayı Kapat",
        section_theme: "Uygulama Teması",
        theme_desc_system_dark: "Sistem teması algılandı (Koyu)",
        theme_desc_system_light: "Sistem teması algılandı (Açık)",
        theme_desc_light: "Açık tema seçili",
        theme_desc_dark: "Koyu tema seçili",
        theme_opt_light: "Açık",
        theme_opt_dark: "Koyu",
        section_lang: "Uygulama Dili",
        lang_desc_system_prefix: "Sistem dili algılandı",
        lang_desc_selected: "Türkçe arayüz seçili",
        lang_opt_system: "Sistem",
        portable_title: "Taşınabilir (Portable) Sürüm",
        portable_body: "AlwaysOn tek bir çalışabilir (.exe) dosyadır; kurulum gerektirmez.\n\nKalıcılık ve Windows başlangıç ayarının sorunsuz çalışması için bu dosyayı kalıcı ve güvenilir bir klasöre kopyalayıp oradan çalıştırmanız önerilir.",
        portable_ok: "Anladım, Devam Et",
        portable_footer_tip: "Kalıcılık Notu",
        tray_inactive: "AlwaysOn: Kapalı",
        tray_active: "AlwaysOn: Aktif",
        notify_activated: "Uyku engelleme aktif.",
        notify_deactivated: "Uyku engelleme devre dışı.",
        notify_timeout: "Süre doldu, uyku engelleme kapatıldı.",
        menu_open: w!("Kontrol Panelini Aç"),
        menu_off: w!("Kapalı (Normal Uyku)"),
        menu_forever: w!("Sürekli Açık Tut"),
        menu_30m: w!("30 Dakika Açık Tut"),
        menu_1h: w!("1 Saat Açık Tut"),
        menu_3h: w!("3 Saat Açık Tut"),
        menu_auto: w!("Başlangıçta Otomatik Çalıştır"),
        menu_exit: w!("Çıkış"),
    },
    // 1: English
    UiStrings {
        window_title: w!("AlwaysOn Control Panel"),
        settings_title: "Settings",
        hero_title: "Sleep Prevention",
        hero_active_forever: "Active • Always on",
        hero_off: "Off • Normal sleep mode",
        hero_remaining_prefix: "Remaining",
        section_mode: "Duration Mode",
        mode_forever: "Always",
        mode_30m: "30 Min",
        mode_1h: "1 Hour",
        mode_3h: "3 Hours",
        section_display: "Display & Power",
        display_title: "Keep display awake",
        display_sub: "Prevents monitor from turning off",
        section_startup: "Startup",
        startup_title: "Run at Windows startup",
        startup_sub: "Launch automatically at sign-in",
        section_close: "Close Button (\u{2715})",
        close_desc_tray: "Minimizes to system tray (background)",
        close_desc_exit: "Closes the application completely",
        close_opt_tray: "Minimize to Tray",
        close_opt_exit: "Exit Application",
        section_theme: "App Theme",
        theme_desc_system_dark: "System theme detected (Dark)",
        theme_desc_system_light: "System theme detected (Light)",
        theme_desc_light: "Light theme selected",
        theme_desc_dark: "Dark theme selected",
        theme_opt_light: "Light",
        theme_opt_dark: "Dark",
        section_lang: "App Language",
        lang_desc_system_prefix: "System language detected",
        lang_desc_selected: "English interface selected",
        lang_opt_system: "System",
        portable_title: "Portable Single-EXE Edition",
        portable_body: "AlwaysOn runs from a single standalone executable (.exe) with no installer.\n\nTo ensure persistence and reliable Windows startup, copy this file into a permanent, trusted folder before enabling startup.",
        portable_ok: "Got it, Continue",
        portable_footer_tip: "Portable Note",
        tray_inactive: "AlwaysOn: Inactive",
        tray_active: "AlwaysOn: Active",
        notify_activated: "AlwaysOn activated.",
        notify_deactivated: "AlwaysOn deactivated.",
        notify_timeout: "Timer ended, sleep prevention deactivated.",
        menu_open: w!("Open Control Panel"),
        menu_off: w!("Off (Normal Sleep)"),
        menu_forever: w!("Always On"),
        menu_30m: w!("Keep On for 30 Minutes"),
        menu_1h: w!("Keep On for 1 Hour"),
        menu_3h: w!("Keep On for 3 Hours"),
        menu_auto: w!("Run at Startup"),
        menu_exit: w!("Exit"),
    },
    // 2: Deutsch
    UiStrings {
        window_title: w!("AlwaysOn Bedienfeld"),
        settings_title: "Einstellungen",
        hero_title: "Schlaf verhindern",
        hero_active_forever: "Aktiv • Dauerhaft ein",
        hero_off: "Aus • Normaler Ruhemodus",
        hero_remaining_prefix: "Verbleibend",
        section_mode: "Laufzeitmodus",
        mode_forever: "Immer",
        mode_30m: "30 Min.",
        mode_1h: "1 Std.",
        mode_3h: "3 Std.",
        section_display: "Bildschirm & Energie",
        display_title: "Bildschirm wach halten",
        display_sub: "Verhindert Ausschalten des Monitors",
        section_startup: "Autostart",
        startup_title: "Bei Windows-Start ausführen",
        startup_sub: "Automatisch nach Anmeldung starten",
        section_close: "Schließen-Taste (\u{2715})",
        close_desc_tray: "In den Infobereich minimieren",
        close_desc_exit: "Anwendung vollständig beenden",
        close_opt_tray: "In Tray ablegen",
        close_opt_exit: "App beenden",
        section_theme: "App-Design",
        theme_desc_system_dark: "Systemdesign erkannt (Dunkel)",
        theme_desc_system_light: "Systemdesign erkannt (Hell)",
        theme_desc_light: "Helles Design ausgewählt",
        theme_desc_dark: "Dunkles Design ausgewählt",
        theme_opt_light: "Hell",
        theme_opt_dark: "Dunkel",
        section_lang: "Sprache",
        lang_desc_system_prefix: "Systemsprache erkannt",
        lang_desc_selected: "Deutsche Oberfläche ausgewählt",
        lang_opt_system: "System",
        portable_title: "Portable Einzeldatei (.exe)",
        portable_body: "AlwaysOn läuft als einzelne portable EXE ohne Installation.\n\nFür dauerhafte Speicherung und zuverlässigen Autostart kopieren Sie die Datei bitte in einen festen, vertrauenswürdigen Ordner.",
        portable_ok: "Verstanden, weiter",
        portable_footer_tip: "Portable-Hinweis",
        tray_inactive: "AlwaysOn: Inaktiv",
        tray_active: "AlwaysOn: Aktiv",
        notify_activated: "AlwaysOn aktiviert.",
        notify_deactivated: "AlwaysOn deaktiviert.",
        notify_timeout: "Timer abgelaufen, Ruhezustand wieder aktiv.",
        menu_open: w!("Bedienfeld öffnen"),
        menu_off: w!("Aus (Normaler Ruhezustand)"),
        menu_forever: w!("Dauerhaft aktiv"),
        menu_30m: w!("30 Minuten aktiv"),
        menu_1h: w!("1 Stunde aktiv"),
        menu_3h: w!("3 Stunden aktiv"),
        menu_auto: w!("Beim Start ausführen"),
        menu_exit: w!("Beenden"),
    },
    // 3: Français
    UiStrings {
        window_title: w!("Panneau AlwaysOn"),
        settings_title: "Paramètres",
        hero_title: "Maintien actif",
        hero_active_forever: "Actif • Toujours allumé",
        hero_off: "Inactif • Mise en veille normale",
        hero_remaining_prefix: "Restant",
        section_mode: "Mode de durée",
        mode_forever: "Toujours",
        mode_30m: "30 Min",
        mode_1h: "1 Heure",
        mode_3h: "3 Heures",
        section_display: "Écran et énergie",
        display_title: "Garder l'écran allumé",
        display_sub: "Empêche l'extinction du moniteur",
        section_startup: "Démarrage",
        startup_title: "Lancer avec Windows",
        startup_sub: "Démarrage automatique à la session",
        section_close: "Bouton fermer (\u{2715})",
        close_desc_tray: "Réduire dans la zone de notification",
        close_desc_exit: "Quitter complètement l'application",
        close_opt_tray: "Réduire (Tray)",
        close_opt_exit: "Quitter l'app",
        section_theme: "Thème",
        theme_desc_system_dark: "Thème système détecté (Sombre)",
        theme_desc_system_light: "Thème système détecté (Clair)",
        theme_desc_light: "Thème clair sélectionné",
        theme_desc_dark: "Thème sombre sélectionné",
        theme_opt_light: "Clair",
        theme_opt_dark: "Sombre",
        section_lang: "Langue",
        lang_desc_system_prefix: "Langue système détectée",
        lang_desc_selected: "Interface française sélectionnée",
        lang_opt_system: "Système",
        portable_title: "Édition portable (.exe unique)",
        portable_body: "AlwaysOn fonctionne depuis un seul exécutable sans installation.\n\nPour garantir la persistance et un démarrage Windows fiable, copiez ce fichier dans un dossier fixe et approuvé.",
        portable_ok: "Compris, continuer",
        portable_footer_tip: "Note portable",
        tray_inactive: "AlwaysOn : Inactif",
        tray_active: "AlwaysOn : Actif",
        notify_activated: "AlwaysOn activé.",
        notify_deactivated: "AlwaysOn désactivé.",
        notify_timeout: "Minuterie terminée, veille réactivée.",
        menu_open: w!("Ouvrir le panneau"),
        menu_off: w!("Désactivé (Veille normale)"),
        menu_forever: w!("Toujours actif"),
        menu_30m: w!("Activer pendant 30 minutes"),
        menu_1h: w!("Activer pendant 1 heure"),
        menu_3h: w!("Activer pendant 3 heures"),
        menu_auto: w!("Lancer au démarrage"),
        menu_exit: w!("Quitter"),
    },
    // 4: Español
    UiStrings {
        window_title: w!("Panel de AlwaysOn"),
        settings_title: "Ajustes",
        hero_title: "Evitar suspensión",
        hero_active_forever: "Activo • Siempre encendido",
        hero_off: "Inactivo • Suspensión normal",
        hero_remaining_prefix: "Restante",
        section_mode: "Duración",
        mode_forever: "Siempre",
        mode_30m: "30 Min",
        mode_1h: "1 Hora",
        mode_3h: "3 Horas",
        section_display: "Pantalla y energía",
        display_title: "Mantener pantalla activa",
        display_sub: "Evita que el monitor se apague",
        section_startup: "Inicio",
        startup_title: "Ejecutar al iniciar Windows",
        startup_sub: "Iniciar automáticamente al entrar",
        section_close: "Botón cerrar (\u{2715})",
        close_desc_tray: "Minimizar a la bandeja del sistema",
        close_desc_exit: "Cerrar la aplicación por completo",
        close_opt_tray: "Minimizar (Tray)",
        close_opt_exit: "Salir de la app",
        section_theme: "Tema",
        theme_desc_system_dark: "Tema del sistema (Oscuro)",
        theme_desc_system_light: "Tema del sistema (Claro)",
        theme_desc_light: "Tema claro seleccionado",
        theme_desc_dark: "Tema oscuro seleccionado",
        theme_opt_light: "Claro",
        theme_opt_dark: "Oscuro",
        section_lang: "Idioma",
        lang_desc_system_prefix: "Idioma del sistema",
        lang_desc_selected: "Interfaz en español seleccionada",
        lang_opt_system: "Sistema",
        portable_title: "Edición portátil (.exe único)",
        portable_body: "AlwaysOn funciona desde un único ejecutable sin instalador.\n\nPara asegurar persistencia y un inicio fiable con Windows, copie este archivo a una carpeta permanente y de confianza.",
        portable_ok: "Entendido, continuar",
        portable_footer_tip: "Nota portátil",
        tray_inactive: "AlwaysOn: Inactivo",
        tray_active: "AlwaysOn: Activo",
        notify_activated: "AlwaysOn activado.",
        notify_deactivated: "AlwaysOn desactivado.",
        notify_timeout: "Tiempo finalizado, suspensión reactivada.",
        menu_open: w!("Abrir panel de control"),
        menu_off: w!("Desactivado (Suspensión)"),
        menu_forever: w!("Siempre activo"),
        menu_30m: w!("Mantener 30 minutos"),
        menu_1h: w!("Mantener 1 hora"),
        menu_3h: w!("Mantener 3 horas"),
        menu_auto: w!("Ejecutar al inicio"),
        menu_exit: w!("Salir"),
    },
    // 5: Italiano
    UiStrings {
        window_title: w!("Pannello AlwaysOn"),
        settings_title: "Impostazioni",
        hero_title: "Sempre attivo",
        hero_active_forever: "Attivo • Sempre acceso",
        hero_off: "Spento • Sospensione normale",
        hero_remaining_prefix: "Rimanente",
        section_mode: "Modalità durata",
        mode_forever: "Sempre",
        mode_30m: "30 Min",
        mode_1h: "1 Ora",
        mode_3h: "3 Ore",
        section_display: "Schermo e energia",
        display_title: "Mantieni schermo acceso",
        display_sub: "Impedisce lo spegnimento del monitor",
        section_startup: "Avvio",
        startup_title: "Avvia con Windows",
        startup_sub: "Avvio automatico all'accesso",
        section_close: "Pulsante chiudi (\u{2715})",
        close_desc_tray: "Riduci nell'area di notifica",
        close_desc_exit: "Chiudi completamente l'app",
        close_opt_tray: "Riduci a icona",
        close_opt_exit: "Chiudi l'app",
        section_theme: "Tema",
        theme_desc_system_dark: "Tema di sistema (Scuro)",
        theme_desc_system_light: "Tema di sistema (Chiaro)",
        theme_desc_light: "Tema chiaro selezionato",
        theme_desc_dark: "Tema scuro selezionato",
        theme_opt_light: "Chiaro",
        theme_opt_dark: "Scuro",
        section_lang: "Lingua",
        lang_desc_system_prefix: "Lingua di sistema",
        lang_desc_selected: "Interfaccia italiana selezionata",
        lang_opt_system: "Sistema",
        portable_title: "Edizione portatile (.exe singolo)",
        portable_body: "AlwaysOn funziona da un singolo file eseguibile senza installazione.\n\nPer garantire la persistenza e un avvio affidabile con Windows, copia il file in una cartella permanente e sicura.",
        portable_ok: "Ho capito, continua",
        portable_footer_tip: "Nota portatile",
        tray_inactive: "AlwaysOn: Inattivo",
        tray_active: "AlwaysOn: Attivo",
        notify_activated: "AlwaysOn attivato.",
        notify_deactivated: "AlwaysOn disattivato.",
        notify_timeout: "Timer scaduto, sospensione riattivata.",
        menu_open: w!("Apri pannello"),
        menu_off: w!("Spento (Sospensione)"),
        menu_forever: w!("Sempre attivo"),
        menu_30m: w!("Attivo per 30 minuti"),
        menu_1h: w!("Attivo per 1 ora"),
        menu_3h: w!("Attivo per 3 ore"),
        menu_auto: w!("Avvia con Windows"),
        menu_exit: w!("Esci"),
    },
    // 6: Português
    UiStrings {
        window_title: w!("Painel AlwaysOn"),
        settings_title: "Definições",
        hero_title: "Impedir suspensão",
        hero_active_forever: "Ativo • Sempre ligado",
        hero_off: "Desligado • Suspensão normal",
        hero_remaining_prefix: "Restante",
        section_mode: "Modo de duração",
        mode_forever: "Sempre",
        mode_30m: "30 Min",
        mode_1h: "1 Hora",
        mode_3h: "3 Horas",
        section_display: "Tela e energia",
        display_title: "Manter tela ligada",
        display_sub: "Impede o monitor de desligar",
        section_startup: "Inicialização",
        startup_title: "Iniciar com o Windows",
        startup_sub: "Iniciar automaticamente na sessão",
        section_close: "Botão fechar (\u{2715})",
        close_desc_tray: "Minimizar para a bandeja do sistema",
        close_desc_exit: "Fechar o aplicativo completamente",
        close_opt_tray: "Minimizar (Tray)",
        close_opt_exit: "Sair do app",
        section_theme: "Tema",
        theme_desc_system_dark: "Tema do sistema (Escuro)",
        theme_desc_system_light: "Tema do sistema (Claro)",
        theme_desc_light: "Tema claro selecionado",
        theme_desc_dark: "Tema escuro selecionado",
        theme_opt_light: "Claro",
        theme_opt_dark: "Escuro",
        section_lang: "Idioma",
        lang_desc_system_prefix: "Idioma do sistema",
        lang_desc_selected: "Interface em português selecionada",
        lang_opt_system: "Sistema",
        portable_title: "Edição portátil (.exe único)",
        portable_body: "AlwaysOn executa a partir de um único arquivo executável sem instalação.\n\nPara garantir persistência e inicialização confiável com o Windows, copie este arquivo para uma pasta permanente e confiável.",
        portable_ok: "Entendi, continuar",
        portable_footer_tip: "Nota portátil",
        tray_inactive: "AlwaysOn: Inativo",
        tray_active: "AlwaysOn: Ativo",
        notify_activated: "AlwaysOn ativado.",
        notify_deactivated: "AlwaysOn desativado.",
        notify_timeout: "Tempo esgotado, suspensão reativada.",
        menu_open: w!("Abrir painel"),
        menu_off: w!("Desligado (Suspensão)"),
        menu_forever: w!("Sempre ligado"),
        menu_30m: w!("Manter por 30 minutos"),
        menu_1h: w!("Manter por 1 hora"),
        menu_3h: w!("Manter por 3 horas"),
        menu_auto: w!("Iniciar com Windows"),
        menu_exit: w!("Sair"),
    },
    // 7: Русский
    UiStrings {
        window_title: w!("Панель AlwaysOn"),
        settings_title: "Настройки",
        hero_title: "Запрет сна",
        hero_active_forever: "Активно • Постоянно вкл.",
        hero_off: "Выкл. • Обычный режим сна",
        hero_remaining_prefix: "Осталось",
        section_mode: "Режим работы",
        mode_forever: "Всегда",
        mode_30m: "30 мин",
        mode_1h: "1 час",
        mode_3h: "3 часа",
        section_display: "Экран и питание",
        display_title: "Не выключать экран",
        display_sub: "Запрещает отключение монитора",
        section_startup: "Автозапуск",
        startup_title: "Запуск вместе с Windows",
        startup_sub: "Автостарт при входе в систему",
        section_close: "Кнопка закрытия (\u{2715})",
        close_desc_tray: "Свернуть в системный трей",
        close_desc_exit: "Полностью завершить программу",
        close_opt_tray: "Свернуть в трей",
        close_opt_exit: "Выйти полностью",
        section_theme: "Тема оформления",
        theme_desc_system_dark: "Системная тема (Тёмная)",
        theme_desc_system_light: "Системная тема (Светлая)",
        theme_desc_light: "Выбрана светлая тема",
        theme_desc_dark: "Выбрана тёмная тема",
        theme_opt_light: "Светлая",
        theme_opt_dark: "Тёмная",
        section_lang: "Язык приложения",
        lang_desc_system_prefix: "Системный язык",
        lang_desc_selected: "Выбран русский интерфейс",
        lang_opt_system: "Система",
        portable_title: "Портативная версия (один .exe)",
        portable_body: "AlwaysOn запускается из одного переносимого EXE без установки.\n\nДля надёжного автозапуска и сохранения выберите для файла постоянную доверенную папку.",
        portable_ok: "Понятно, продолжить",
        portable_footer_tip: "О портативности",
        tray_inactive: "AlwaysOn: Откл.",
        tray_active: "AlwaysOn: Активно",
        notify_activated: "AlwaysOn включён.",
        notify_deactivated: "AlwaysOn выключен.",
        notify_timeout: "Таймер завершён, режим сна включён.",
        menu_open: w!("Открыть панель"),
        menu_off: w!("Выкл. (Обычный сон)"),
        menu_forever: w!("Всегда активно"),
        menu_30m: w!("Включить на 30 минут"),
        menu_1h: w!("Включить на 1 час"),
        menu_3h: w!("Включить на 3 часа"),
        menu_auto: w!("Запускать с Windows"),
        menu_exit: w!("Выход"),
    },
    // 8: 中文
    UiStrings {
        window_title: w!("AlwaysOn 控制面板"),
        settings_title: "设置",
        hero_title: "防止休眠",
        hero_active_forever: "已开启 • 始终保持唤醒",
        hero_off: "已关闭 • 正常休眠模式",
        hero_remaining_prefix: "剩余时间",
        section_mode: "运行时长",
        mode_forever: "始终开启",
        mode_30m: "30 分钟",
        mode_1h: "1 小时",
        mode_3h: "3 小时",
        section_display: "显示与电源",
        display_title: "同时保持屏幕常亮",
        display_sub: "阻止显示器自动关闭",
        section_startup: "开机启动",
        startup_title: "开机自动启动",
        startup_sub: "登录 Windows 时自动运行",
        section_close: "关闭按钮 (\u{2715})",
        close_desc_tray: "最小化到系统托盘（后台运行）",
        close_desc_exit: "完全退出应用程序",
        close_opt_tray: "最小化到托盘",
        close_opt_exit: "退出应用",
        section_theme: "应用主题",
        theme_desc_system_dark: "检测到系统主题（深色）",
        theme_desc_system_light: "检测到系统主题（浅色）",
        theme_desc_light: "已选择浅色主题",
        theme_desc_dark: "已选择深色主题",
        theme_opt_light: "浅色",
        theme_opt_dark: "深色",
        section_lang: "界面语言",
        lang_desc_system_prefix: "检测到系统语言",
        lang_desc_selected: "已选择简体中文界面",
        lang_opt_system: "系统",
        portable_title: "绿色便携版（单文件 EXE）",
        portable_body: "AlwaysOn 为单文件绿色便携程序，无需安装。\n\n为确保开机自启稳定与长期使用，建议将此 EXE 复制到固定且受信任的文件夹中运行。",
        portable_ok: "知道了，继续",
        portable_footer_tip: "便携说明",
        tray_inactive: "AlwaysOn: 已关闭",
        tray_active: "AlwaysOn: 已激活",
        notify_activated: "AlwaysOn 已激活。",
        notify_deactivated: "AlwaysOn 已关闭。",
        notify_timeout: "计时结束，已恢复系统休眠。",
        menu_open: w!("打开控制面板"),
        menu_off: w!("关闭（正常休眠）"),
        menu_forever: w!("始终保持唤醒"),
        menu_30m: w!("保持唤醒 30 分钟"),
        menu_1h: w!("保持唤醒 1 小时"),
        menu_3h: w!("保持唤醒 3 小时"),
        menu_auto: w!("开机自动启动"),
        menu_exit: w!("退出"),
    },
    // 9: 日本語
    UiStrings {
        window_title: w!("AlwaysOn コントロールパネル"),
        settings_title: "設定",
        hero_title: "スリープ防止",
        hero_active_forever: "有効 • 常時スリープ防止",
        hero_off: "無効 • 通常スリープ",
        hero_remaining_prefix: "残り時間",
        section_mode: "動作モード",
        mode_forever: "常時オン",
        mode_30m: "30 分",
        mode_1h: "1 時間",
        mode_3h: "3 時間",
        section_display: "画面と電源",
        display_title: "画面も点灯したままにする",
        display_sub: "モニターの消灯を防止します",
        section_startup: "スタートアップ",
        startup_title: "Windows 起動時に実行",
        startup_sub: "サインイン時に自動起動",
        section_close: "閉じるボタン (\u{2715})",
        close_desc_tray: "タスクトレイに最小化（バックグラウンド）",
        close_desc_exit: "アプリケーションを完全に終了",
        close_opt_tray: "トレイに最小化",
        close_opt_exit: "アプリを終了",
        section_theme: "アプリテーマ",
        theme_desc_system_dark: "システムテーマ検出（ダーク）",
        theme_desc_system_light: "システムテーマ検出（ライト）",
        theme_desc_light: "ライトテーマ選択中",
        theme_desc_dark: "ダークテーマ選択中",
        theme_opt_light: "ライト",
        theme_opt_dark: "ダーク",
        section_lang: "アプリ言語",
        lang_desc_system_prefix: "システム言語を検出",
        lang_desc_selected: "日本語インターフェース選択中",
        lang_opt_system: "システム",
        portable_title: "ポータブル単体 EXE 版",
        portable_body: "AlwaysOn はインストール不要の単体実行ファイルです。\n\n設定の永続化と安定した Windows 自動起動のため、信頼できる固定フォルダへコピーして実行することをおすすめします。",
        portable_ok: "了解、続行する",
        portable_footer_tip: "ポータブル情報",
        tray_inactive: "AlwaysOn: 停止",
        tray_active: "AlwaysOn: 有効",
        notify_activated: "AlwaysOn を有効にしました。",
        notify_deactivated: "AlwaysOn を無効にしました。",
        notify_timeout: "タイマー終了、スリープ防止を解除しました。",
        menu_open: w!("パネルを開く"),
        menu_off: w!("オフ（通常スリープ）"),
        menu_forever: w!("常時オン"),
        menu_30m: w!("30 分間オンにする"),
        menu_1h: w!("1 時間オンにする"),
        menu_3h: w!("3 時間オンにする"),
        menu_auto: w!("スタートアップ時に実行"),
        menu_exit: w!("終了"),
    },
    // 10: 한국어
    UiStrings {
        window_title: w!("AlwaysOn 제어판"),
        settings_title: "설정",
        hero_title: "절전 방지",
        hero_active_forever: "활성 • 항상 켜짐",
        hero_off: "꺼짐 • 일반 절전 모드",
        hero_remaining_prefix: "남은 시간",
        section_mode: "실행 모드",
        mode_forever: "항상 유지",
        mode_30m: "30분",
        mode_1h: "1시간",
        mode_3h: "3시간",
        section_display: "디스플레이 및 전원",
        display_title: "화면도 켜진 상태로 유지",
        display_sub: "모니터가 꺼지지 않도록 방지",
        section_startup: "시작 프로그램",
        startup_title: "Windows 시작 시 실행",
        startup_sub: "로그인 시 자동으로 시작",
        section_close: "닫기 버튼 (\u{2715})",
        close_desc_tray: "시스템 트레이로 최소화(백그라운드)",
        close_desc_exit: "애플리케이션 완전히 종료",
        close_opt_tray: "트레이로 최소화",
        close_opt_exit: "앱 종료",
        section_theme: "앱 테마",
        theme_desc_system_dark: "시스템 테마 감지됨 (다크)",
        theme_desc_system_light: "시스템 테마 감지됨 (라이트)",
        theme_desc_light: "라이트 테마 선택됨",
        theme_desc_dark: "다크 테마 선택됨",
        theme_opt_light: "라이트",
        theme_opt_dark: "다크",
        section_lang: "앱 언어",
        lang_desc_system_prefix: "시스템 언어 감지됨",
        lang_desc_selected: "한국어 인터페이스 선택됨",
        lang_opt_system: "시스템",
        portable_title: "포터블 단일 EXE 버전",
        portable_body: "AlwaysOn은 설치가 필요 없는 단일 실행 파일입니다.\n\n안정적인 시작 프로그램 등록과 지속적인 사용을 위해 신뢰할 수 있는 고정 폴더에 복사하여 실행하세요.",
        portable_ok: "확인, 계속하기",
        portable_footer_tip: "포터블 안내",
        tray_inactive: "AlwaysOn: 꺼짐",
        tray_active: "AlwaysOn: 활성",
        notify_activated: "AlwaysOn이 활성화되었습니다.",
        notify_deactivated: "AlwaysOn이 비활성화되었습니다.",
        notify_timeout: "타이머가 종료되어 절전 방지가 해제되었습니다.",
        menu_open: w!("제어판 열기"),
        menu_off: w!("끄기 (일반 절전)"),
        menu_forever: w!("항상 켜기"),
        menu_30m: w!("30분 동안 켜기"),
        menu_1h: w!("1시간 동안 켜기"),
        menu_3h: w!("3시간 동안 켜기"),
        menu_auto: w!("시작 시 자동 실행"),
        menu_exit: w!("종료"),
    },
];

struct AppState {
    mode: Mode,
    hicon_awake: HICON,
    hicon_sleep: HICON,
    hicon_hero_awake: HICON,
    hicon_hero_sleep: HICON,
    lang_pref: LangPref,
    lang: Lang,
    theme_pref: ThemePref,
    is_light_theme: bool,
    wm_taskbar_created: u32,
    keep_display_on: bool,
    close_to_tray: bool,
    in_settings: bool,
    show_first_run: bool,
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
    anim_theme_opt: f32,
    anim_lang_col: f32,
    anim_lang_row: f32,
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
    lang_pref: LangPref::System,
    lang: Lang::En,
    theme_pref: ThemePref::System,
    is_light_theme: false,
    wm_taskbar_created: 0,
    keep_display_on: true,
    close_to_tray: true,
    in_settings: false,
    show_first_run: false,
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
    anim_theme_opt: 0.0,
    anim_lang_col: 0.0,
    anim_lang_row: 0.0,
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
        let s = ui_strings(STATE.lang);
        let active = STATE.mode != Mode::Off;
        if !active {
            return s.tray_inactive.to_string();
        }
        if STATE.remaining_secs > 0 {
            let h = STATE.remaining_secs / 3600;
            let m = (STATE.remaining_secs % 3600) / 60;
            let sec = STATE.remaining_secs % 60;
            format!("{} ({h:02}:{m:02}:{sec:02})", s.tray_active)
        } else {
            s.tray_active.to_string()
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
    let Some(target_exe) = current_exe_path() else {
        return;
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

    if !configured_exe.exists() || !paths_equal(&configured_exe, &curr_exe) {
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

fn lang_pref_from_dword(val: u32) -> LangPref {
    match val {
        1 => LangPref::Tr,
        2 => LangPref::En,
        3 => LangPref::De,
        4 => LangPref::Fr,
        5 => LangPref::Es,
        6 => LangPref::It,
        7 => LangPref::Pt,
        8 => LangPref::Ru,
        9 => LangPref::Zh,
        10 => LangPref::Ja,
        11 => LangPref::Ko,
        _ => LangPref::System,
    }
}

fn lang_from_primary_lang_id(primary: u16) -> Lang {
    match primary {
        0x1F => Lang::Tr,
        0x09 => Lang::En,
        0x07 => Lang::De,
        0x0C => Lang::Fr,
        0x0A => Lang::Es,
        0x10 => Lang::It,
        0x16 => Lang::Pt,
        0x19 => Lang::Ru,
        0x04 => Lang::Zh,
        0x11 => Lang::Ja,
        0x12 => Lang::Ko,
        _ => Lang::En,
    }
}

fn system_detected_lang() -> Lang {
    lang_from_primary_lang_id(unsafe { GetUserDefaultUILanguage() & 0x3FF })
}

fn resolve_lang(pref: LangPref, sys_lang: Lang) -> Lang {
    match pref {
        LangPref::System => sys_lang,
        LangPref::Tr => Lang::Tr,
        LangPref::En => Lang::En,
        LangPref::De => Lang::De,
        LangPref::Fr => Lang::Fr,
        LangPref::Es => Lang::Es,
        LangPref::It => Lang::It,
        LangPref::Pt => Lang::Pt,
        LangPref::Ru => Lang::Ru,
        LangPref::Zh => Lang::Zh,
        LangPref::Ja => Lang::Ja,
        LangPref::Ko => Lang::Ko,
    }
}

#[inline]
fn lang_grid_pos(pref: LangPref) -> (f32, f32) {
    let idx = pref as usize;
    ((idx % 4) as f32, (idx / 4) as f32)
}

#[inline]
fn ui_strings(lang: Lang) -> &'static UiStrings {
    &UI_STRINGS[lang as usize]
}

fn theme_pref_from_dword(val: u32) -> ThemePref {
    match val {
        1 => ThemePref::Light,
        2 => ThemePref::Dark,
        _ => ThemePref::System,
    }
}

unsafe fn system_uses_light_theme() -> bool {
    let mut hkey = HKEY::default();
    if RegOpenKeyExW(HKEY_CURRENT_USER, PERSONALIZE_KEY_PATH, 0, KEY_READ, &mut hkey).is_err() {
        return false;
    }
    let mut val: u32 = 0;
    let mut len: u32 = mem::size_of::<u32>() as u32;
    let mut kind = REG_VALUE_TYPE::default();
    let ok = RegQueryValueExW(
        hkey,
        REG_APPS_USE_LIGHT_THEME,
        None,
        Some(&mut kind),
        Some(&mut val as *mut u32 as *mut u8),
        Some(&mut len),
    )
    .is_ok()
        && kind == REG_DWORD;
    let _ = RegCloseKey(hkey);
    ok && val != 0
}

#[inline]
fn resolve_is_light_theme(pref: ThemePref, sys_light: bool) -> bool {
    match pref {
        ThemePref::System => sys_light,
        ThemePref::Light => true,
        ThemePref::Dark => false,
    }
}

unsafe fn set_theme_pref(hwnd: HWND, pref: ThemePref) {
    STATE.theme_pref = pref;
    STATE.is_light_theme = resolve_is_light_theme(pref, system_uses_light_theme());
    write_reg_dword(REG_THEME, pref as u32);
    apply_dwm_winui(hwnd);
    start_ui_anim(hwnd);
    let _ = InvalidateRect(hwnd, None, false);
}

unsafe fn set_language_pref(hwnd: HWND, pref: LangPref) {
    STATE.lang_pref = pref;
    STATE.lang = resolve_lang(pref, system_detected_lang());
    write_reg_dword(REG_LANGUAGE, pref as u32);
    let _ = SetWindowTextW(hwnd, ui_strings(STATE.lang).window_title);
    update_tray_tooltip_only(hwnd);
    start_ui_anim(hwnd);
    let _ = InvalidateRect(hwnd, None, false);
}

unsafe fn load_app_settings() {
    STATE.close_to_tray = read_reg_dword(REG_CLOSE_TO_TRAY, 1) != 0;
    STATE.keep_display_on = read_reg_dword(REG_KEEP_DISPLAY_ON, 1) != 0;
    STATE.lang_pref = lang_pref_from_dword(read_reg_dword(REG_LANGUAGE, 0));
    STATE.lang = resolve_lang(STATE.lang_pref, system_detected_lang());
    STATE.theme_pref = theme_pref_from_dword(read_reg_dword(REG_THEME, 0));
    STATE.is_light_theme = resolve_is_light_theme(STATE.theme_pref, system_uses_light_theme());
    STATE.show_first_run = read_reg_dword(REG_FIRST_RUN_DONE, 0) == 0;
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
    STATE.anim_theme_opt = (STATE.theme_pref as u32) as f32;
    let (lcol, lrow) = lang_grid_pos(STATE.lang_pref);
    STATE.anim_lang_col = lcol;
    STATE.anim_lang_row = lrow;
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
    let theme_opt_target = (STATE.theme_pref as u32) as f32;
    let (lang_col_target, lang_row_target) = lang_grid_pos(STATE.lang_pref);

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

    let mut theme_val = STATE.anim_theme_opt;
    any_changed |= lerp_step(&mut theme_val, theme_opt_target, speed);
    STATE.anim_theme_opt = theme_val;

    let mut lang_col_val = STATE.anim_lang_col;
    any_changed |= lerp_step(&mut lang_col_val, lang_col_target, speed);
    STATE.anim_lang_col = lang_col_val;

    let mut lang_row_val = STATE.anim_lang_row;
    any_changed |= lerp_step(&mut lang_row_val, lang_row_target, speed);
    STATE.anim_lang_row = lang_row_val;

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
            let light = STATE.is_light_theme;
            let dark: i32 = if light { 0 } else { 1 };
            let _ = set_attr(hwnd, 20, &dark as *const i32 as *const c_void, 4);
            let corner_round: i32 = 2;
            let _ = set_attr(hwnd, 33, &corner_round as *const i32 as *const c_void, 4);
            let border_color: u32 = if light {
                rgb(214, 214, 214).0
            } else {
                rgb(52, 52, 52).0
            };
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
    let show_first_run = unsafe { STATE.show_first_run };
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
            return HIT_BTN_BACK;
        } else if in_rect(lx, ly, 16, 52, 328, 44) {
            return HIT_TOGGLE_AUTOSTART;
        } else if in_rect(lx, ly, 32, 140, 146, 25) {
            return HIT_CLOSE_TO_TRAY;
        } else if in_rect(lx, ly, 182, 140, 146, 25) {
            return HIT_CLOSE_EXIT;
        } else if in_rect(lx, ly, 32, 216, 96, 25) {
            return HIT_THEME_SYSTEM;
        } else if in_rect(lx, ly, 132, 216, 96, 25) {
            return HIT_THEME_LIGHT;
        } else if in_rect(lx, ly, 232, 216, 96, 25) {
            return HIT_THEME_DARK;
        }
        for idx in 0..12usize {
            let col = (idx % 4) as i32;
            let row = (idx / 4) as i32;
            if in_rect(lx, ly, 30 + col * 76, 293 + row * 26, 72, 23) {
                return HIT_LANG_BASE + idx;
            }
        }
        HIT_NONE
    } else if show_first_run {
        if in_rect(lx, ly, 28, 266, 304, 38) {
            HIT_BTN_GITHUB
        } else if in_rect(lx, ly, 16, 332, 328, 40) {
            HIT_FIRST_RUN_OK
        } else {
            HIT_NONE
        }
    } else if in_rect(lx, ly, 16, 52, 328, 86) {
        HIT_HERO
    } else if in_rect(lx, ly, 20, 180, 78, 40) {
        HIT_MODE_FOREVER
    } else if in_rect(lx, ly, 100, 180, 78, 40) {
        HIT_MODE_30M
    } else if in_rect(lx, ly, 180, 180, 78, 40) {
        HIT_MODE_1H
    } else if in_rect(lx, ly, 260, 180, 78, 40) {
        HIT_MODE_3H
    } else if in_rect(lx, ly, 16, 264, 328, 72) {
        HIT_TOGGLE_DISPLAY
    } else if in_rect(lx, ly, 16, 348, 196, 28) {
        HIT_BTN_GITHUB
    } else if in_rect(lx, ly, 218, 348, 126, 28) {
        HIT_BTN_PORTABLE_INFO
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

    fn fill_winui_base(&mut self, light: bool) {
        let (a, rgb_v): (u32, u32) = if light {
            (245, 243) // #F3F3F3 WinUI 3 Light Mica Alt
        } else {
            (240, 31) // #1F1F1F WinUI 3 Dark Mica Alt
        };
        let ch: u32 = (rgb_v * a + 127) / 255;
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

    fn draw_winui_toggle(&mut self, lx: f32, ly: f32, lw: f32, lh: f32, t: f32, light: bool) {
        let t = t.clamp(0.0, 1.0);
        let lerp_u8 = |a: u8, b: u8| -> u8 {
            ((a as f32) + ((b as f32) - (a as f32)) * t)
                .round()
                .clamp(0.0, 255.0) as u8
        };
        let (off_track, on_track, off_border, off_thumb, on_thumb) = if light {
            (
                [238u8, 238, 238],
                [0u8, 103, 192],
                [134u8, 134, 134],
                [92u8, 92, 92],
                [255u8, 255, 255],
            )
        } else {
            (
                [35u8, 35, 35],
                [76u8, 194, 255],
                [138u8, 138, 138],
                [200u8, 200, 200],
                [20u8, 20, 20],
            )
        };
        let track_col = [
            lerp_u8(off_track[0], on_track[0]),
            lerp_u8(off_track[1], on_track[1]),
            lerp_u8(off_track[2], on_track[2]),
        ];
        let r = lh * 0.5;
        self.draw_round_card(
            lx,
            ly,
            lw,
            lh,
            r,
            track_col,
            1.0,
            off_border,
            1.0 - t,
        );

        let stretch = 4.0 * t * (1.0 - t) * 3.5;
        let kh = 11.0 + t * 1.0;
        let kw = kh + stretch;
        let kx = lx + 4.5 + t * (lw - 8.5 - kw) + stretch * 0.5 - stretch * t;
        let ky = ly + (lh - kh) * 0.5;
        let thumb_col = [
            lerp_u8(off_thumb[0], on_thumb[0]),
            lerp_u8(off_thumb[1], on_thumb[1]),
            lerp_u8(off_thumb[2], on_thumb[2]),
        ];
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

    let is_dark_ink = (col[0] as u16 + col[1] as u16 + col[2] as u16) < 300;
    for py in y0..y1 {
        let off = (py as usize) * (canvas.w as usize);
        for px in x0..x1 {
            let pix = text_buf[off + px as usize];
            if (pix & 0x00FF_FFFF) != 0 {
                let r = ((pix >> 16) & 0xFF) as f32;
                let g = ((pix >> 8) & 0xFF) as f32;
                let b = (pix & 0xFF) as f32;
                let mut lum = (r * 0.299 + g * 0.587 + b * 0.114) / 255.0;
                if is_dark_ink {
                    lum = lum.sqrt() * 0.35 + lum * 0.65;
                }
                if lum > 0.004 {
                    canvas.blend_px(px, py, col[0], col[1], col[2], alpha * lum);
                }
            }
        }
    }
}

struct ThemePalette {
    is_light: bool,
    card_fill: [u8; 3],
    card_hover: [u8; 3],
    card_border: [u8; 3],
    card_border_active: [u8; 3],
    track_fill: [u8; 3],
    track_border: [u8; 3],
    pill_fill: [u8; 3],
    pill_border: [u8; 3],
    pill_hover: [u8; 3],
    btn_hover: [u8; 3],
    btn_active_bg: [u8; 3],
    btn_active_border: [u8; 3],
    accent: [u8; 3],
    accent_text: [u8; 3],
    text_primary: [u8; 3],
    text_selected: [u8; 3],
    text_secondary: [u8; 3],
    text_muted: [u8; 3],
    text_caption: [u8; 3],
    icon_fg: [u8; 3],
    icon_caption: [u8; 3],
}

const fn theme_palette(light: bool) -> ThemePalette {
    if light {
        ThemePalette {
            is_light: true,
            card_fill: [252, 252, 252],
            card_hover: [247, 247, 247],
            card_border: [224, 224, 224],
            card_border_active: [120, 178, 228],
            track_fill: [241, 241, 241],
            track_border: [222, 222, 222],
            pill_fill: [255, 255, 255],
            pill_border: [206, 206, 206],
            pill_hover: [232, 232, 232],
            btn_hover: [230, 230, 230],
            btn_active_bg: [235, 235, 235],
            btn_active_border: [206, 206, 206],
            accent: [0, 103, 192],
            accent_text: [0, 95, 184],
            text_primary: [27, 27, 27],
            text_selected: [16, 16, 16],
            text_secondary: [82, 82, 82],
            text_muted: [102, 102, 102],
            text_caption: [28, 28, 28],
            icon_fg: [48, 48, 48],
            icon_caption: [56, 56, 56],
        }
    } else {
        ThemePalette {
            is_light: false,
            card_fill: [43, 43, 43],
            card_hover: [50, 50, 50],
            card_border: [58, 58, 58],
            card_border_active: [56, 120, 168],
            track_fill: [32, 32, 32],
            track_border: [54, 54, 54],
            pill_fill: [56, 56, 56],
            pill_border: [70, 70, 70],
            pill_hover: [46, 46, 46],
            btn_hover: [46, 46, 46],
            btn_active_bg: [52, 52, 52],
            btn_active_border: [68, 68, 68],
            accent: [76, 194, 255],
            accent_text: [96, 205, 255],
            text_primary: [243, 243, 243],
            text_selected: [255, 255, 255],
            text_secondary: [196, 196, 196],
            text_muted: [158, 158, 158],
            text_caption: [230, 230, 230],
            icon_fg: [224, 224, 224],
            icon_caption: [214, 214, 214],
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

    let ui = ui_strings(STATE.lang);
    let active = STATE.mode != Mode::Off;
    let hover = STATE.hover_item;
    let rem = STATE.remaining_secs;
    let in_settings = STATE.in_settings;
    let show_first_run = STATE.show_first_run;
    let close_to_tray = STATE.close_to_tray;
    let lang_pref = STATE.lang_pref;
    let theme_pref = STATE.theme_pref;
    let pal = theme_palette(STATE.is_light_theme);
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

        // 1. WinUI 3 Surface Base (#F3F3F3 Light / #1F1F1F Dark with Mica Alt blend)
        canvas.fill_winui_base(pal.is_light);

        let mug_rgba = if active { MUG_AWAKE_RGBA } else { MUG_SLEEP_RGBA };

        // 2. Title Bar (y = 0..38)
        if !in_settings {
            canvas.draw_rgba_icon(mug_rgba, 16.0, 9.0, 20.0, 20.0);
        } else if hover == HIT_BTN_BACK {
            canvas.draw_round_card(12.0, 5.0, 32.0, 28.0, 4.0, pal.btn_hover, 1.0, pal.btn_hover, 0.0);
        }

        // Settings Gear Button (236, 5, 36, 28)
        if in_settings {
            canvas.draw_round_card(236.0, 5.0, 36.0, 28.0, 4.0, pal.btn_active_bg, 1.0, pal.btn_active_border, 1.0);
            canvas.draw_round_card(246.0, 30.0, 16.0, 2.5, 1.25, pal.accent, 1.0, pal.accent, 0.0);
        } else if hover == HIT_BTN_SETTINGS {
            canvas.draw_round_card(236.0, 5.0, 36.0, 28.0, 4.0, pal.btn_hover, 1.0, pal.btn_hover, 0.0);
        }
        // Minimize Caption Button (276, 5, 36, 28)
        if hover == HIT_BTN_HIDE {
            canvas.draw_round_card(276.0, 5.0, 36.0, 28.0, 4.0, pal.btn_hover, 1.0, pal.btn_hover, 0.0);
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
                pal.text_caption,
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
                pal.text_caption,
                1.0,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                ui.settings_title,
                48,
                8,
                160,
                22,
                dpi,
                font_body_bold,
                pal.text_primary,
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
            if in_settings { pal.accent } else { pal.icon_caption },
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
            pal.icon_caption,
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
            if hover == HIT_BTN_EXIT { [255, 255, 255] } else { pal.icon_caption },
            1.0,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );

        if in_settings {
            // === SETTINGS VIEW (360 x 388) ===
            // Section 1: Startup
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                ui.section_startup,
                18 + vx_i,
                35,
                200,
                16,
                dpi,
                font_section,
                pal.text_muted,
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );

            let auto_fill = if hover == HIT_TOGGLE_AUTOSTART { pal.card_hover } else { pal.card_fill };
            canvas.draw_round_card(16.0 + vx, 52.0, 328.0, 44.0, 8.0, auto_fill, 1.0, pal.card_border, 1.0);
            canvas.draw_winui_toggle(286.0 + vx, 64.0, 40.0, 20.0, STATE.anim_auto_toggle, pal.is_light);

            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                "\u{E7B5}",
                28 + vx_i,
                52,
                24,
                44,
                dpi,
                font_fluent,
                pal.icon_fg,
                1.0,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                ui.startup_title,
                60 + vx_i,
                56,
                216,
                18,
                dpi,
                font_body,
                pal.text_primary,
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                ui.startup_sub,
                60 + vx_i,
                74,
                216,
                16,
                dpi,
                font_sub,
                pal.text_muted,
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );

            // Section 2: Close Button Behavior
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                ui.section_close,
                18 + vx_i,
                98,
                260,
                17,
                dpi,
                font_section,
                pal.text_muted,
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );

            canvas.draw_round_card(16.0 + vx, 116.0, 328.0, 56.0, 8.0, pal.card_fill, 1.0, pal.card_border, 1.0);
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                "\u{E8BB}",
                28 + vx_i,
                120,
                24,
                15,
                dpi,
                font_fluent_gear,
                pal.icon_fg,
                1.0,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            let close_desc = if close_to_tray { ui.close_desc_tray } else { ui.close_desc_exit };
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                close_desc,
                56 + vx_i,
                120,
                276,
                15,
                dpi,
                font_sub,
                pal.accent_text,
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );

            canvas.draw_round_card(28.0 + vx, 138.0, 304.0, 29.0, 6.0, pal.track_fill, 1.0, pal.track_border, 1.0);
            let close_opts: [(usize, bool, f32, &str); 2] = [
                (HIT_CLOSE_TO_TRAY, close_to_tray, 32.0, ui.close_opt_tray),
                (HIT_CLOSE_EXIT, !close_to_tray, 182.0, ui.close_opt_exit),
            ];
            for &(hid, selected, ox, _) in &close_opts {
                if hover == hid && !selected {
                    canvas.draw_round_card(ox + vx, 140.0, 146.0, 25.0, 5.0, pal.pill_hover, 1.0, pal.pill_hover, 0.0);
                }
            }
            let opt_x = 32.0 + STATE.anim_close_opt * 150.0 + vx;
            canvas.draw_round_card(opt_x, 140.0, 146.0, 25.0, 5.0, pal.pill_fill, 1.0, pal.pill_border, 1.0);
            canvas.draw_round_card(opt_x + 53.0, 162.5, 40.0, 2.0, 1.0, pal.accent, 1.0, pal.accent, 0.0);
            for &(_, selected, ox, label) in &close_opts {
                draw_glass_text(
                    &mut canvas,
                    text_dc,
                    text_buf,
                    label,
                    ox as i32 + vx_i,
                    139,
                    146,
                    23,
                    dpi,
                    if selected { font_body_bold } else { font_body },
                    if selected { pal.text_selected } else { pal.text_secondary },
                    1.0,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
            }

            // Section 3: App Theme (System / Light / Dark)
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                ui.section_theme,
                18 + vx_i,
                174,
                260,
                17,
                dpi,
                font_section,
                pal.text_muted,
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );

            canvas.draw_round_card(16.0 + vx, 192.0, 328.0, 56.0, 8.0, pal.card_fill, 1.0, pal.card_border, 1.0);
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                "\u{E790}",
                28 + vx_i,
                196,
                24,
                15,
                dpi,
                font_fluent_gear,
                pal.icon_fg,
                1.0,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            let theme_desc = match theme_pref {
                ThemePref::System => {
                    if pal.is_light {
                        ui.theme_desc_system_light
                    } else {
                        ui.theme_desc_system_dark
                    }
                }
                ThemePref::Light => ui.theme_desc_light,
                ThemePref::Dark => ui.theme_desc_dark,
            };
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                theme_desc,
                56 + vx_i,
                196,
                276,
                15,
                dpi,
                font_sub,
                pal.accent_text,
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );

            canvas.draw_round_card(28.0 + vx, 214.0, 304.0, 29.0, 6.0, pal.track_fill, 1.0, pal.track_border, 1.0);
            let theme_opts: [(usize, ThemePref, f32, &str); 3] = [
                (HIT_THEME_SYSTEM, ThemePref::System, 32.0, ui.lang_opt_system),
                (HIT_THEME_LIGHT, ThemePref::Light, 132.0, ui.theme_opt_light),
                (HIT_THEME_DARK, ThemePref::Dark, 232.0, ui.theme_opt_dark),
            ];
            for &(hid, tpref, tx, _) in &theme_opts {
                if hover == hid && theme_pref != tpref {
                    canvas.draw_round_card(tx + vx, 216.0, 96.0, 25.0, 5.0, pal.pill_hover, 1.0, pal.pill_hover, 0.0);
                }
            }
            let theme_x = 32.0 + STATE.anim_theme_opt * 100.0 + vx;
            canvas.draw_round_card(theme_x, 216.0, 96.0, 25.0, 5.0, pal.pill_fill, 1.0, pal.pill_border, 1.0);
            canvas.draw_round_card(theme_x + 30.0, 238.5, 36.0, 2.0, 1.0, pal.accent, 1.0, pal.accent, 0.0);
            for &(_, tpref, tx, label) in &theme_opts {
                let selected = theme_pref == tpref;
                draw_glass_text(
                    &mut canvas,
                    text_dc,
                    text_buf,
                    label,
                    tx as i32 + vx_i,
                    215,
                    96,
                    23,
                    dpi,
                    if selected { font_body_bold } else { font_body },
                    if selected { pal.text_selected } else { pal.text_secondary },
                    1.0,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
            }

            // Section 4: App Language (3x4 Grid: System + 11 popular languages)
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                ui.section_lang,
                18 + vx_i,
                250,
                260,
                17,
                dpi,
                font_section,
                pal.text_muted,
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );

            canvas.draw_round_card(16.0 + vx, 268.0, 328.0, 108.0, 8.0, pal.card_fill, 1.0, pal.card_border, 1.0);
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                "\u{E774}",
                28 + vx_i,
                272,
                24,
                15,
                dpi,
                font_fluent_gear,
                pal.icon_fg,
                1.0,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            let lang_desc = if lang_pref == LangPref::System {
                let sys_lang = system_detected_lang();
                format!("{} ({})", ui.lang_desc_system_prefix, LANG_NATIVE_NAMES[sys_lang as usize])
            } else {
                ui.lang_desc_selected.to_string()
            };
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                &lang_desc,
                56 + vx_i,
                272,
                276,
                15,
                dpi,
                font_sub,
                pal.accent_text,
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );

            canvas.draw_round_card(26.0 + vx, 290.0, 308.0, 81.0, 6.0, pal.track_fill, 1.0, pal.track_border, 1.0);
            let sel_idx = lang_pref as usize;
            for idx in 0..12usize {
                if hover == HIT_LANG_BASE + idx && sel_idx != idx {
                    let col = (idx % 4) as f32;
                    let row = (idx / 4) as f32;
                    canvas.draw_round_card(
                        30.0 + col * 76.0 + vx,
                        293.0 + row * 26.0,
                        72.0,
                        23.0,
                        5.0,
                        pal.pill_hover,
                        1.0,
                        pal.pill_hover,
                        0.0,
                    );
                }
            }
            let active_x = 30.0 + STATE.anim_lang_col * 76.0 + vx;
            let active_y = 293.0 + STATE.anim_lang_row * 26.0;
            canvas.draw_round_card(active_x, active_y, 72.0, 23.0, 5.0, pal.pill_fill, 1.0, pal.pill_border, 1.0);
            canvas.draw_round_card(active_x + 20.0, active_y + 20.5, 32.0, 2.0, 1.0, pal.accent, 1.0, pal.accent, 0.0);

            for idx in 0..12usize {
                let col = (idx % 4) as i32;
                let row = (idx / 4) as i32;
                let selected = sel_idx == idx;
                let label = if idx == 0 {
                    ui.lang_opt_system
                } else {
                    LANG_NATIVE_NAMES[idx - 1]
                };
                draw_glass_text(
                    &mut canvas,
                    text_dc,
                    text_buf,
                    label,
                    30 + col * 76 + vx_i,
                    292 + row * 26,
                    72,
                    21,
                    dpi,
                    font_sub,
                    if selected { pal.text_selected } else { pal.text_secondary },
                    1.0,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
            }
        } else if show_first_run {
            // === FIRST-RUN PORTABLE PERSISTENCE NOTICE ===
            canvas.draw_round_card(16.0 + vx, 48.0, 328.0, 272.0, 10.0, pal.card_fill, 1.0, pal.card_border, 1.0);
            canvas.draw_rgba_icon(mug_rgba, 28.0 + vx, 62.0, 40.0, 40.0);
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                ui.portable_title,
                78 + vx_i,
                63,
                252,
                20,
                dpi,
                font_title_disp,
                pal.text_selected,
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                "Portable • Single EXE",
                78 + vx_i,
                84,
                252,
                16,
                dpi,
                font_sub,
                pal.accent_text,
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                ui.portable_body,
                28 + vx_i,
                118,
                304,
                138,
                dpi,
                font_body,
                pal.text_primary,
                1.0,
                DT_LEFT | DT_TOP | DT_WORDBREAK,
            );

            // Clickable GitHub link pill inside notice card
            let gh_fill = if hover == HIT_BTN_GITHUB { pal.pill_hover } else { pal.track_fill };
            canvas.draw_round_card(28.0 + vx, 266.0, 304.0, 38.0, 6.0, gh_fill, 1.0, pal.track_border, 1.0);
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                "\u{E71B}",
                42 + vx_i,
                266,
                20,
                38,
                dpi,
                font_fluent_sm,
                pal.accent_text,
                1.0,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                "github.com/yunusemreyl",
                68 + vx_i,
                266,
                248,
                38,
                dpi,
                font_body_bold,
                pal.accent_text,
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );

            // Primary Confirm Button
            let ok_fill = if hover == HIT_FIRST_RUN_OK {
                if pal.is_light { [0, 90, 172] } else { [96, 206, 255] }
            } else {
                pal.accent
            };
            canvas.draw_round_card(16.0 + vx, 332.0, 328.0, 40.0, 8.0, ok_fill, 1.0, ok_fill, 0.0);
            let ok_ink = if pal.is_light { [255, 255, 255] } else { [18, 18, 18] };
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                ui.portable_ok,
                16 + vx_i,
                332,
                328,
                40,
                dpi,
                font_body_bold,
                ok_ink,
                1.0,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
        } else {
            // === MAIN CONTROL VIEW (360 x 388) ===
            // 3. Hero SettingsCard (16, 52, 328, 86, r = 8)
            let hero_fill = if hover == HIT_HERO { pal.card_hover } else { pal.card_fill };
            let hero_border = if active { pal.card_border_active } else { pal.card_border };
            canvas.draw_round_card(16.0 + vx, 52.0, 328.0, 86.0, 8.0, hero_fill, 1.0, hero_border, 1.0);
            canvas.draw_rgba_icon(mug_rgba, 28.0 + vx, 69.0, 52.0, 52.0);
            canvas.draw_winui_toggle(286.0 + vx, 85.0, 40.0, 20.0, STATE.anim_hero_toggle, pal.is_light);

            // 4. WinUI 3 SelectorBar / Segmented Control (16, 176, 328, 48, r = 8)
            canvas.draw_round_card(16.0 + vx, 176.0, 328.0, 48.0, 8.0, pal.card_fill, 1.0, pal.card_border, 1.0);
            let segs: [(usize, Mode, f32, &str); 4] = [
                (HIT_MODE_FOREVER, Mode::Forever, 20.0, ui.mode_forever),
                (HIT_MODE_30M, Mode::Timer30M, 100.0, ui.mode_30m),
                (HIT_MODE_1H, Mode::Timer1H, 180.0, ui.mode_1h),
                (HIT_MODE_3H, Mode::Timer3H, 260.0, ui.mode_3h),
            ];
            for &(hid, mode, sx, _) in &segs {
                if hover == hid && STATE.mode != mode {
                    canvas.draw_round_card(sx + vx, 180.0, 78.0, 40.0, 6.0, pal.pill_hover, 1.0, pal.pill_hover, 0.0);
                }
            }
            if STATE.anim_mode_alpha > 0.01 {
                let pill_x = 20.0 + STATE.anim_mode_idx * 80.0 + vx;
                canvas.draw_round_card(
                    pill_x,
                    180.0,
                    78.0,
                    40.0,
                    6.0,
                    pal.pill_fill,
                    STATE.anim_mode_alpha,
                    pal.pill_border,
                    STATE.anim_mode_alpha,
                );
                canvas.draw_round_card(
                    pill_x + 27.0,
                    215.0,
                    24.0,
                    3.0,
                    1.5,
                    pal.accent,
                    STATE.anim_mode_alpha,
                    pal.accent,
                    0.0,
                );
            }

            // 5. Display SettingsCard (16, 264, 328, 72, r = 8)
            let disp_fill = if hover == HIT_TOGGLE_DISPLAY { pal.card_hover } else { pal.card_fill };
            canvas.draw_round_card(16.0 + vx, 264.0, 328.0, 72.0, 8.0, disp_fill, 1.0, pal.card_border, 1.0);
            canvas.draw_winui_toggle(286.0 + vx, 290.0, 40.0, 20.0, STATE.anim_disp_toggle, pal.is_light);

            // Hero Card Title & Status
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                ui.hero_title,
                88 + vx_i,
                70,
                188,
                22,
                dpi,
                font_title_disp,
                pal.text_selected,
                1.0,
                DT_LEFT | DT_SINGLELINE,
            );
            let hero_sub = if active && rem > 0 {
                let h = rem / 3600;
                let m = (rem % 3600) / 60;
                let sec = rem % 60;
                format!("{}: {h:02}:{m:02}:{sec:02}", ui.hero_remaining_prefix)
            } else if active {
                ui.hero_active_forever.to_string()
            } else {
                ui.hero_off.to_string()
            };
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                &hero_sub,
                88 + vx_i,
                94,
                188,
                18,
                dpi,
                font_sub,
                if active { pal.accent_text } else { pal.text_muted },
                1.0,
                DT_LEFT | DT_SINGLELINE,
            );

            // Section Label 1
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                ui.section_mode,
                18 + vx_i,
                154,
                200,
                18,
                dpi,
                font_section,
                pal.text_muted,
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
                    179,
                    78,
                    36,
                    dpi,
                    if selected { font_body_bold } else { font_body },
                    if selected { pal.text_selected } else { pal.text_secondary },
                    1.0,
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE,
                );
            }

            // Section Label 2
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                ui.section_display,
                18 + vx_i,
                242,
                200,
                18,
                dpi,
                font_section,
                pal.text_muted,
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
                264,
                24,
                72,
                dpi,
                font_fluent,
                pal.icon_fg,
                1.0,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                ui.display_title,
                60 + vx_i,
                279,
                216,
                20,
                dpi,
                font_body,
                pal.text_primary,
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                ui.display_sub,
                60 + vx_i,
                301,
                216,
                16,
                dpi,
                font_sub,
                pal.text_muted,
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );

            // 6. Footer Strip: Clickable GitHub link & Portable Note re-opener (y = 348..376)
            let gh_foot_fill = if hover == HIT_BTN_GITHUB { pal.card_hover } else { pal.card_fill };
            canvas.draw_round_card(16.0 + vx, 348.0, 196.0, 28.0, 6.0, gh_foot_fill, 1.0, pal.card_border, 1.0);
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                "\u{E71B}",
                24 + vx_i,
                348,
                16,
                28,
                dpi,
                font_fluent_sm,
                pal.accent_text,
                1.0,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                "github.com/yunusemreyl",
                44 + vx_i,
                348,
                162,
                28,
                dpi,
                font_sub,
                pal.accent_text,
                1.0,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            );

            let info_foot_fill = if hover == HIT_BTN_PORTABLE_INFO { pal.card_hover } else { pal.card_fill };
            canvas.draw_round_card(218.0 + vx, 348.0, 126.0, 28.0, 6.0, info_foot_fill, 1.0, pal.card_border, 1.0);
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                "\u{E946}",
                224 + vx_i,
                348,
                16,
                28,
                dpi,
                font_fluent_sm,
                pal.text_muted,
                1.0,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE,
            );
            draw_glass_text(
                &mut canvas,
                text_dc,
                text_buf,
                ui.portable_footer_tip,
                242 + vx_i,
                348,
                96,
                28,
                dpi,
                font_sub,
                pal.text_secondary,
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

        let title = ui_strings(STATE.lang).window_title;

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
    set_string(&mut nid.szInfo, ui_strings(STATE.lang).notify_timeout);
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
                HIT_THEME_SYSTEM => set_theme_pref(hwnd, ThemePref::System),
                HIT_THEME_LIGHT => set_theme_pref(hwnd, ThemePref::Light),
                HIT_THEME_DARK => set_theme_pref(hwnd, ThemePref::Dark),
                hit if (HIT_LANG_BASE..HIT_LANG_BASE + 12).contains(&hit) => {
                    set_language_pref(hwnd, lang_pref_from_dword((hit - HIT_LANG_BASE) as u32));
                }
                HIT_BTN_HIDE => hide_control_panel(hwnd, false),
                HIT_BTN_EXIT => {
                    if STATE.close_to_tray {
                        hide_control_panel(hwnd, false);
                    } else {
                        PostQuitMessage(0);
                    }
                }
                HIT_FIRST_RUN_OK => {
                    STATE.show_first_run = false;
                    write_reg_dword(REG_FIRST_RUN_DONE, 1);
                    STATE.anim_view_slide = -1.0;
                    STATE.hover_item = HIT_NONE;
                    start_ui_anim(hwnd);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                HIT_BTN_PORTABLE_INFO => {
                    STATE.show_first_run = true;
                    STATE.anim_view_slide = 1.0;
                    STATE.hover_item = HIT_NONE;
                    start_ui_anim(hwnd);
                    let _ = InvalidateRect(hwnd, None, false);
                }
                HIT_BTN_GITHUB => {
                    let _ = ShellExecuteW(
                        hwnd,
                        w!("open"),
                        GITHUB_PROFILE_URL,
                        None,
                        None,
                        SW_SHOWNORMAL,
                    );
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
        WM_SETTINGCHANGE => {
            if STATE.theme_pref == ThemePref::System {
                let sys_light = system_uses_light_theme();
                if sys_light != STATE.is_light_theme {
                    STATE.is_light_theme = sys_light;
                    apply_dwm_winui(hwnd);
                    let _ = InvalidateRect(hwnd, None, false);
                }
            }
            DefWindowProcW(hwnd, msg, wparam, lparam)
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

    let s = ui_strings(STATE.lang);
    if is_active {
        nid.hIcon = STATE.hicon_awake;
        if notify {
            set_string(&mut nid.szInfo, s.notify_activated);
        }
    } else {
        nid.hIcon = STATE.hicon_sleep;
        if notify {
            set_string(&mut nid.szInfo, s.notify_deactivated);
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
        let s = ui_strings(STATE.lang);
        let txt_open = s.menu_open;
        let txt_off = s.menu_off;
        let txt_forever = s.menu_forever;
        let txt_30m = s.menu_30m;
        let txt_1h = s.menu_1h;
        let txt_3h = s.menu_3h;
        let txt_auto = s.menu_auto;
        let txt_exit = s.menu_exit;

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

    unsafe fn delete_reg_value(name: PCWSTR) {
        let mut hkey = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, SETTINGS_KEY_PATH, 0, KEY_SET_VALUE, &mut hkey).is_ok() {
            let _ = RegDeleteValueW(hkey, name);
            let _ = RegCloseKey(hkey);
        }
    }

    struct RegRestoreGuard {
        orig_close: u32,
        orig_disp: u32,
        orig_lang: u32,
        orig_theme: u32,
        orig_first_run: u32,
        had_close: bool,
        had_disp: bool,
        had_lang: bool,
        had_theme: bool,
        had_first_run: bool,
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
            let l = reg_has_value(REG_LANGUAGE);
            let t = reg_has_value(REG_THEME);
            let f = reg_has_value(REG_FIRST_RUN_DONE);
            Self {
                orig_close: c.unwrap_or(1),
                orig_disp: d.unwrap_or(1),
                orig_lang: l.unwrap_or(0),
                orig_theme: t.unwrap_or(0),
                orig_first_run: f.unwrap_or(0),
                had_close: c.is_some(),
                had_disp: d.is_some(),
                had_lang: l.is_some(),
                had_theme: t.is_some(),
                had_first_run: f.is_some(),
            }
        }
    }

    impl Drop for RegRestoreGuard {
        fn drop(&mut self) {
            unsafe {
                if self.had_close {
                    write_reg_dword(REG_CLOSE_TO_TRAY, self.orig_close);
                } else {
                    delete_reg_value(REG_CLOSE_TO_TRAY);
                }
                if self.had_disp {
                    write_reg_dword(REG_KEEP_DISPLAY_ON, self.orig_disp);
                } else {
                    delete_reg_value(REG_KEEP_DISPLAY_ON);
                }
                if self.had_lang {
                    write_reg_dword(REG_LANGUAGE, self.orig_lang);
                } else {
                    delete_reg_value(REG_LANGUAGE);
                }
                if self.had_theme {
                    write_reg_dword(REG_THEME, self.orig_theme);
                } else {
                    delete_reg_value(REG_THEME);
                }
                if self.had_first_run {
                    write_reg_dword(REG_FIRST_RUN_DONE, self.orig_first_run);
                } else {
                    delete_reg_value(REG_FIRST_RUN_DONE);
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
            let orig_first_run = STATE.show_first_run;
            STATE.in_settings = false;
            STATE.show_first_run = false;

            // Main view caption + controls + footer links (360 x 388)
            assert_eq!(hit_test_ui(254, 15), HIT_BTN_SETTINGS);
            assert_eq!(hit_test_ui(294, 15), HIT_BTN_HIDE);
            assert_eq!(hit_test_ui(334, 15), HIT_BTN_EXIT);
            assert_eq!(hit_test_ui(120, 95), HIT_HERO);
            assert_eq!(hit_test_ui(55, 198), HIT_MODE_FOREVER);
            assert_eq!(hit_test_ui(135, 198), HIT_MODE_30M);
            assert_eq!(hit_test_ui(215, 198), HIT_MODE_1H);
            assert_eq!(hit_test_ui(295, 198), HIT_MODE_3H);
            assert_eq!(hit_test_ui(120, 295), HIT_TOGGLE_DISPLAY);
            assert_eq!(hit_test_ui(100, 360), HIT_BTN_GITHUB);
            assert_eq!(hit_test_ui(280, 360), HIT_BTN_PORTABLE_INFO);
            assert_eq!(hit_test_ui(120, 162), HIT_NONE);

            // First-run portable & GitHub notice overlay
            STATE.show_first_run = true;
            assert_eq!(hit_test_ui(180, 285), HIT_BTN_GITHUB);
            assert_eq!(hit_test_ui(180, 352), HIT_FIRST_RUN_OK);
            assert_eq!(hit_test_ui(120, 162), HIT_NONE);
            STATE.show_first_run = false;

            // Settings view caption + startup toggle + close behavior + theme selector + 3x4 language grid
            STATE.in_settings = true;
            assert_eq!(hit_test_ui(24, 18), HIT_BTN_BACK);
            assert_eq!(hit_test_ui(254, 15), HIT_BTN_SETTINGS);
            assert_eq!(hit_test_ui(120, 74), HIT_TOGGLE_AUTOSTART);
            assert_eq!(hit_test_ui(100, 152), HIT_CLOSE_TO_TRAY);
            assert_eq!(hit_test_ui(250, 152), HIT_CLOSE_EXIT);
            assert_eq!(hit_test_ui(80, 228), HIT_THEME_SYSTEM);
            assert_eq!(hit_test_ui(180, 228), HIT_THEME_LIGHT);
            assert_eq!(hit_test_ui(280, 228), HIT_THEME_DARK);
            for idx in 0..12usize {
                let col = (idx % 4) as i32;
                let row = (idx / 4) as i32;
                let cx = 30 + col * 76 + 36;
                let cy = 293 + row * 26 + 11;
                assert_eq!(hit_test_ui(cx, cy), HIT_LANG_BASE + idx);
            }
            // Gaps & card headers outside buttons must return HIT_NONE
            assert_eq!(hit_test_ui(180, 108), HIT_NONE);
            assert_eq!(hit_test_ui(180, 204), HIT_NONE);
            assert_eq!(hit_test_ui(180, 278), HIT_NONE);

            STATE.in_settings = orig_settings;
            STATE.show_first_run = orig_first_run;
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

        // Dark mode surface & card verification
        canvas.fill_winui_base(false);
        assert_eq!((canvas.buf[0] >> 24) & 0xFF, 240);
        canvas.draw_round_card(16.0, 44.0, 328.0, 72.0, 8.0, [43, 43, 43], 1.0, [58, 58, 58], 1.0);
        assert_eq!(canvas.buf[(80 * w + 180) as usize], 0xFF_2B_2B_2B);

        // Light mode surface & card verification
        canvas.fill_winui_base(true);
        assert_eq!((canvas.buf[0] >> 24) & 0xFF, 245);
        let light_pal = theme_palette(true);
        canvas.draw_round_card(16.0, 44.0, 328.0, 72.0, 8.0, light_pal.card_fill, 1.0, light_pal.card_border, 1.0);
        assert_eq!(canvas.buf[(80 * w + 180) as usize], 0xFF_FC_FC_FC);

        // Draw embedded awake/sleep mug icons + animated toggle states in both Dark and Light palettes
        canvas.draw_rgba_icon(MUG_AWAKE_RGBA, 28.0, 56.0, 48.0, 48.0);
        canvas.draw_rgba_icon(MUG_SLEEP_RGBA, 28.0, 120.0, 32.0, 32.0);
        canvas.draw_winui_toggle(286.0, 70.0, 40.0, 20.0, 0.0, false);
        canvas.draw_winui_toggle(286.0, 140.0, 40.0, 20.0, 0.5, true);
        canvas.draw_winui_toggle(286.0, 210.0, 40.0, 20.0, 1.0, false);
        canvas.draw_winui_toggle(286.0, 260.0, 40.0, 20.0, 1.0, true);

        let mug_center = canvas.buf[(80 * w + 52) as usize];
        assert_ne!(mug_center, 0xFF_FC_FC_FC);
        let toggle_dark_on = canvas.buf[(220 * w + 292) as usize];
        let toggle_dark_off = canvas.buf[(80 * w + 316) as usize];
        let toggle_light_on = canvas.buf[(270 * w + 292) as usize];
        assert_ne!(toggle_dark_on, toggle_dark_off);
        assert_ne!(toggle_light_on, toggle_dark_on);
    }

    #[test]
    fn test_scenario_registry_settings_roundtrip() {
        let expected_prefs = [
            LangPref::System,
            LangPref::Tr,
            LangPref::En,
            LangPref::De,
            LangPref::Fr,
            LangPref::Es,
            LangPref::It,
            LangPref::Pt,
            LangPref::Ru,
            LangPref::Zh,
            LangPref::Ja,
            LangPref::Ko,
        ];
        for (i, &pref) in expected_prefs.iter().enumerate() {
            assert_eq!(lang_pref_from_dword(i as u32), pref);
        }
        assert_eq!(lang_pref_from_dword(99), LangPref::System);

        let primary_map = [
            (0x1F, Lang::Tr),
            (0x09, Lang::En),
            (0x07, Lang::De),
            (0x0C, Lang::Fr),
            (0x0A, Lang::Es),
            (0x10, Lang::It),
            (0x16, Lang::Pt),
            (0x19, Lang::Ru),
            (0x04, Lang::Zh),
            (0x11, Lang::Ja),
            (0x12, Lang::Ko),
            (0x00, Lang::En),
        ];
        for (pid, expected_lang) in primary_map {
            assert_eq!(lang_from_primary_lang_id(pid), expected_lang);
        }

        assert_eq!(lang_grid_pos(LangPref::System), (0.0, 0.0));
        assert_eq!(lang_grid_pos(LangPref::De), (3.0, 0.0));
        assert_eq!(lang_grid_pos(LangPref::Ru), (0.0, 2.0));
        assert_eq!(lang_grid_pos(LangPref::Ko), (3.0, 2.0));

        assert_eq!(resolve_lang(LangPref::System, Lang::Ja), Lang::Ja);
        assert_eq!(resolve_lang(LangPref::De, Lang::Tr), Lang::De);
        assert_eq!(resolve_lang(LangPref::Ko, Lang::En), Lang::Ko);

        assert_eq!(theme_pref_from_dword(0), ThemePref::System);
        assert_eq!(theme_pref_from_dword(1), ThemePref::Light);
        assert_eq!(theme_pref_from_dword(2), ThemePref::Dark);
        assert_eq!(theme_pref_from_dword(99), ThemePref::System);
        assert!(resolve_is_light_theme(ThemePref::Light, false));
        assert!(!resolve_is_light_theme(ThemePref::Dark, true));
        assert!(resolve_is_light_theme(ThemePref::System, true));
        assert!(!resolve_is_light_theme(ThemePref::System, false));

        let _lock = STATE_TEST_LOCK.lock().unwrap();
        unsafe {
            let _guard = RegRestoreGuard::capture();

            write_reg_dword(REG_CLOSE_TO_TRAY, 0);
            write_reg_dword(REG_KEEP_DISPLAY_ON, 0);
            write_reg_dword(REG_LANGUAGE, 3); // De
            write_reg_dword(REG_THEME, 1); // Light
            load_app_settings();
            let lp_de = STATE.lang_pref;
            let l_de = STATE.lang;
            let tp_l = STATE.theme_pref;
            let is_l = STATE.is_light_theme;
            assert!(!STATE.close_to_tray);
            assert!(!STATE.keep_display_on);
            assert_eq!(lp_de, LangPref::De);
            assert_eq!(l_de, Lang::De);
            assert_eq!(tp_l, ThemePref::Light);
            assert!(is_l);
            assert!((STATE.anim_close_opt - 1.0).abs() < f32::EPSILON);
            assert!((STATE.anim_theme_opt - 1.0).abs() < f32::EPSILON);
            assert!((STATE.anim_disp_toggle - 0.0).abs() < f32::EPSILON);
            assert!((STATE.anim_lang_col - 3.0).abs() < f32::EPSILON);
            assert!((STATE.anim_lang_row - 0.0).abs() < f32::EPSILON);

            write_reg_dword(REG_THEME, 2); // Dark
            load_app_settings();
            let tp_d = STATE.theme_pref;
            let is_d = STATE.is_light_theme;
            assert_eq!(tp_d, ThemePref::Dark);
            assert!(!is_d);
            assert!((STATE.anim_theme_opt - 2.0).abs() < f32::EPSILON);

            for (dword, pref, lang, exp_col, exp_row) in [
                (10, LangPref::Ja, Lang::Ja, 2.0f32, 2.0f32),
                (8, LangPref::Ru, Lang::Ru, 0.0f32, 2.0f32),
                (2, LangPref::En, Lang::En, 2.0f32, 0.0f32),
                (1, LangPref::Tr, Lang::Tr, 1.0f32, 0.0f32),
            ] {
                write_reg_dword(REG_LANGUAGE, dword);
                load_app_settings();
                let cur_pref = STATE.lang_pref;
                let cur_lang = STATE.lang;
                assert_eq!(cur_pref, pref);
                assert_eq!(cur_lang, lang);
                assert!((STATE.anim_lang_col - exp_col).abs() < f32::EPSILON);
                assert!((STATE.anim_lang_row - exp_row).abs() < f32::EPSILON);
            }

            write_reg_dword(REG_CLOSE_TO_TRAY, 1);
            write_reg_dword(REG_KEEP_DISPLAY_ON, 1);
            write_reg_dword(REG_LANGUAGE, 0);
            write_reg_dword(REG_THEME, 0);
            write_reg_dword(REG_FIRST_RUN_DONE, 1);
            load_app_settings();
            assert!(!STATE.show_first_run);
            write_reg_dword(REG_FIRST_RUN_DONE, 0);
            load_app_settings();
            let lp_sys = STATE.lang_pref;
            let l_sys = STATE.lang;
            let tp_sys = STATE.theme_pref;
            let is_sys = STATE.is_light_theme;
            assert!(STATE.close_to_tray);
            assert!(STATE.keep_display_on);
            assert!(STATE.show_first_run);
            assert_eq!(lp_sys, LangPref::System);
            assert_eq!(l_sys, system_detected_lang());
            assert_eq!(tp_sys, ThemePref::System);
            assert_eq!(is_sys, system_uses_light_theme());
            assert!((STATE.anim_close_opt - 0.0).abs() < f32::EPSILON);
            assert!((STATE.anim_theme_opt - 0.0).abs() < f32::EPSILON);
            assert!((STATE.anim_disp_toggle - 1.0).abs() < f32::EPSILON);
            assert!((STATE.anim_lang_col - 0.0).abs() < f32::EPSILON);
            assert!((STATE.anim_lang_row - 0.0).abs() < f32::EPSILON);
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
