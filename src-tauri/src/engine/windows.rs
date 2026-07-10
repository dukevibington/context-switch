use windows_sys::Win32::Foundation::{BOOL, CloseHandle, FALSE, HWND, LPARAM, RECT, TRUE};
use windows_sys::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindow, GetWindowRect, GetWindowTextLengthW, GetWindowTextW,
    GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow, SetWindowPos, ShowWindow,
    GW_OWNER, GWL_EXSTYLE, GWL_STYLE, SWP_SHOWWINDOW, SW_MAXIMIZE, SW_MINIMIZE,
    SW_RESTORE, WS_EX_TOOLWINDOW, WS_MAXIMIZE, WS_MINIMIZE,
};

// Handle pointer width differences for GetWindowLongPtrW
#[cfg(target_pointer_width = "64")]
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW;
#[cfg(target_pointer_width = "32")]
use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongW as GetWindowLongPtrW;

use super::{WindowSpyEngine, WindowState, Workspace};
use serde;
use serde_json;
use std::collections::HashMap;

pub struct WindowsWindowSpy;

impl WindowsWindowSpy {
    pub fn new() -> Self {
        Self
    }
}

// Struct to store handle along with WindowState for restoration mapping
struct ActiveWindowInfo {
    hwnd: HWND,
    state: WindowState,
}

impl WindowSpyEngine for WindowsWindowSpy {
    fn capture_windows(&self) -> Result<Vec<WindowState>, String> {
        let active_windows = capture_active_windows_raw(true)?;
        // Map to WindowState vec
        let states = active_windows
            .into_iter()
            .map(|info| info.state)
            .collect();
        Ok(states)
    }

    fn restore_workspace(&self, workspace: &Workspace) -> Result<(), String> {
        let active_windows = capture_active_windows_raw(false)?;
        let our_pid = std::process::id();
        let our_path = std::env::current_exe()
            .ok()
            .map(|p| p.to_string_lossy().to_string().to_lowercase());
        let our_basename = our_path.as_ref().map(|p| get_basename(p).to_lowercase());

        let mut groups: HashMap<String, (Vec<&WindowState>, Vec<&ActiveWindowInfo>)> = HashMap::new();

        // Group saved windows by basename
        for saved_win in &workspace.windows {
            if saved_win.app_name == "Unknown" {
                continue;
            }
            let base = get_basename(&saved_win.app_name).to_lowercase();
            if let Some(ref our_base) = our_basename {
                if base == *our_base {
                    continue;
                }
            }
            if base.contains("context-switch") {
                continue;
            }
            groups.entry(base).or_default().0.push(saved_win);
        }

        // Group active windows by basename
        for act in &active_windows {
            if act.state.process_id == our_pid {
                continue;
            }
            let base = get_basename(&act.state.app_name).to_lowercase();
            if let Some(ref our_base) = our_basename {
                if base == *our_base {
                    continue;
                }
            }
            if base.contains("context-switch") {
                continue;
            }
            groups.entry(base).or_default().1.push(act);
        }

        let mut match_map: HashMap<*const WindowState, &ActiveWindowInfo> = HashMap::new();
        let mut launches: Vec<&WindowState> = Vec::new();
        let mut minimizes: Vec<&ActiveWindowInfo> = Vec::new();

        for (_base, (saved_wins, act_wins)) in &groups {
            for (i, saved_win) in saved_wins.iter().enumerate() {
                if i < act_wins.len() {
                    match_map.insert(*saved_win as *const WindowState, act_wins[i]);
                } else {
                    launches.push(*saved_win);
                }
            }
            for act_win in act_wins.iter().skip(saved_wins.len()) {
                minimizes.push(*act_win);
            }
        }

        // 1. Minimize excess active windows
        for act in minimizes {
            unsafe {
                ShowWindow(act.hwnd, SW_MINIMIZE);
            }
        }

        // 2. Launch missing instances
        for saved_win in launches {
            launch_app(&saved_win.app_name);
        }

        // 3. Restore paired windows in REVERSE order to correctly reconstruct focus Z-order
        for saved_win in workspace.windows.iter().rev() {
            let saved_key = saved_win as *const WindowState;
            if let Some(act) = match_map.get(&saved_key) {
                unsafe {
                    // Activate and bring to top of Z-stack
                    SetForegroundWindow(act.hwnd);

                    if saved_win.is_maximized {
                        ShowWindow(act.hwnd, SW_RESTORE);
                        SetWindowPos(
                            act.hwnd,
                            0, // HWND_TOP
                            saved_win.x,
                            saved_win.y,
                            saved_win.width,
                            saved_win.height,
                            SWP_SHOWWINDOW,
                        );
                        ShowWindow(act.hwnd, SW_MAXIMIZE);
                    } else if saved_win.is_minimized {
                        ShowWindow(act.hwnd, SW_MINIMIZE);
                    } else {
                        ShowWindow(act.hwnd, SW_RESTORE);
                        let success = SetWindowPos(
                            act.hwnd,
                            0, // HWND_TOP
                            saved_win.x,
                            saved_win.y,
                            saved_win.width,
                            saved_win.height,
                            SWP_SHOWWINDOW,
                        );
                        if success == 0 {
                            eprintln!(
                                "Failed to restore size for window: {} (hwnd: {})",
                                saved_win.window_title, act.hwnd
                            );
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

struct EnumParams {
    list: Vec<ActiveWindowInfo>,
    skip_minimized: bool,
}

/// Helper function to scan active windows and collect their HWNDs alongside states
fn capture_active_windows_raw(skip_minimized: bool) -> Result<Vec<ActiveWindowInfo>, String> {
    let mut params = EnumParams {
        list: Vec::new(),
        skip_minimized,
    };
    let lparam = &mut params as *mut EnumParams as LPARAM;

    unsafe {
        let res = EnumWindows(Some(enum_windows_callback), lparam);
        if res == 0 {
            return Err("Failed to enumerate Windows desktop windows.".to_string());
        }
    }

    Ok(params.list)
}

/// Callback invoked by Win32 EnumWindows for each top-level window
unsafe extern "system" fn enum_windows_callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let params = &mut *(lparam as *mut EnumParams);

    // 1. Verify window visibility
    if IsWindowVisible(hwnd) == 0 {
        return TRUE;
    }

    // 2. Validate title length
    let title_len = GetWindowTextLengthW(hwnd);
    if title_len == 0 {
        return TRUE;
    }

    // 3. Skip Tool Windows (tooltips, overlays, floating menus)
    let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
    if (ex_style & WS_EX_TOOLWINDOW) != 0 {
        return TRUE;
    }

    // 4. Skip child windows/popups that have an owner
    let owner = GetWindow(hwnd, GW_OWNER);
    if owner != 0 {
        return TRUE;
    }

    // 5. Capture maximized and minimized states
    let style = GetWindowLongPtrW(hwnd, GWL_STYLE) as u32;
    let is_minimized = (style & WS_MINIMIZE) != 0;
    let is_maximized = (style & WS_MAXIMIZE) != 0;

    // Filter out minimized windows if requested (during Capture)
    if params.skip_minimized && is_minimized {
        return TRUE;
    }

    // 6. Extract Window Title text safely
    let mut title_buf = vec![0u16; (title_len + 1) as usize];
    let actual_len = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), title_buf.len() as i32);
    let window_title = String::from_utf16_lossy(&title_buf[..actual_len as usize]);

    // 7. Retrieve Process ID and corresponding executable/app name
    let mut process_id = 0u32;
    GetWindowThreadProcessId(hwnd, &mut process_id);
    let app_name = get_process_name_by_id(process_id);

    // 7b. Advanced Window Class & System Shell Filtering
    let class_name = get_window_class(hwnd);
    let class_lower = class_name.to_lowercase();
    
    // Skip desktop backgrounds and main taskbar wrappers
    if class_lower == "progman" || class_lower == "workerw" || class_lower == "shell_traywnd" || class_lower == "shell_secondarytraywnd" {
        return TRUE;
    }
    
    // Skip Win10/11 system UI core windows (Start menu overlays, settings flyouts) and Alt+Tab overlays
    if class_lower == "windows.ui.core.corewindow" || class_lower == "multitaskingviewframe" {
        return TRUE;
    }

    // Capture Real File Explorer windows only: if it is explorer.exe, it must be CabinetWClass (folder windows)
    if app_name.to_lowercase().contains("explorer.exe") && class_name != "CabinetWClass" {
        return TRUE;
    }

    // 8. Capture physical screen bounds/layout
    let mut rect: RECT = std::mem::zeroed();
    if GetWindowRect(hwnd, &mut rect) == 0 {
        return TRUE;
    }

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;

    // Filter out zero-size invalid windows
    if width <= 0 || height <= 0 {
        return TRUE;
    }

    params.list.push(ActiveWindowInfo {
        hwnd,
        state: WindowState {
            id: None,
            app_name,
            process_id,
            window_title,
            x: rect.left,
            y: rect.top,
            width,
            height,
            is_minimized,
            is_maximized,
        },
    });

    TRUE
}

/// Helper to get the base executable filename from a Win32 process ID
unsafe fn get_process_name_by_id(process_id: u32) -> String {
    let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, FALSE, process_id);
    if handle == 0 {
        return "Unknown".to_string();
    }

    let mut buf = [0u16; 1024];
    let mut size = buf.len() as u32;
    let res = QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size);
    CloseHandle(handle);

    if res != 0 {
        String::from_utf16_lossy(&buf[..size as usize])
    } else {
        "Unknown".to_string()
    }
}

/// Helper to get the window class name from a window handle
unsafe fn get_window_class(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let len = GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    if len > 0 {
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        "".to_string()
    }
}

/// Helper to extract the executable name (e.g. "explorer.exe") for comparisons
fn get_basename(path: &str) -> &str {
    path.split('\\').last().unwrap_or(path).split('/').last().unwrap_or(path)
}

fn extract_quoted_value(line: &str) -> Option<String> {
    let parts: Vec<&str> = line.split('"').collect();
    if parts.len() >= 4 {
        Some(parts[3].to_string())
    } else {
        None
    }
}

fn extract_yaml_quoted_value(line: &str) -> Option<String> {
    let parts: Vec<&str> = line.splitn(2, ':').collect();
    if parts.len() < 2 {
        return None;
    }
    let val = parts[1].trim();
    let val_stripped = val.trim_matches(|c| c == '"' || c == '\'');
    Some(val_stripped.to_string())
}

fn try_launch_steam(app_path: &str) -> Result<bool, String> {
    let path_lower = app_path.to_lowercase().replace('/', "\\");
    if let Some(idx) = path_lower.find("\\steamapps\\common\\") {
        let steamapps_path_str = &app_path[..idx + 10]; // length of "\steamapps"
        let steamapps_dir = std::path::Path::new(steamapps_path_str);
        
        let common_and_suffix = &path_lower[idx + 18..]; // length of "\steamapps\common\"
        let game_dir_name = common_and_suffix.split('\\').next().unwrap_or("");
        
        if !game_dir_name.is_empty() {
            if let Ok(entries) = std::fs::read_dir(steamapps_dir) {
                for entry in entries.flatten() {
                    let file_name = entry.file_name().to_string_lossy().to_string();
                    if file_name.starts_with("appmanifest_") && file_name.ends_with(".acf") {
                        if let Ok(content) = std::fs::read_to_string(entry.path()) {
                            let mut appid = None;
                            let mut installdir = None;
                            for line in content.lines() {
                                let line_trimmed = line.trim();
                                if line_trimmed.starts_with("\"appid\"") {
                                    appid = extract_quoted_value(line_trimmed);
                                } else if line_trimmed.starts_with("\"installdir\"") {
                                    installdir = extract_quoted_value(line_trimmed);
                                }
                            }
                            if let (Some(id), Some(dir)) = (appid, installdir) {
                                if dir.to_lowercase() == game_dir_name {
                                    println!("[Windows Engine] Launching Steam game appid={} via URI", id);
                                    let launch_uri = format!("steam://rungameid/{}", id);
                                    let _ = std::process::Command::new("cmd")
                                        .args(["/C", "start", "", &launch_uri])
                                        .spawn();
                                    return Ok(true);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(false)
}

#[derive(serde::Deserialize)]
struct EpicManifest {
    #[serde(rename = "InstallLocation")]
    install_location: String,
    #[serde(rename = "LaunchExecutable")]
    launch_executable: String,
    #[serde(rename = "CatalogNamespace")]
    catalog_namespace: Option<String>,
    #[serde(rename = "CatalogItemId")]
    catalog_item_id: Option<String>,
    #[serde(rename = "AppName")]
    app_name: String,
}

fn try_launch_epic(app_path: &str) -> Result<bool, String> {
    let manifest_dir = "C:\\ProgramData\\Epic\\EpicGamesLauncher\\Data\\Manifests";
    let path_norm = app_path.to_lowercase().replace('/', "\\");
    
    if let Ok(entries) = std::fs::read_dir(manifest_dir) {
        for entry in entries.flatten() {
            if entry.path().extension().map_or(false, |ext| ext == "item") {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    if let Ok(manifest) = serde_json::from_str::<EpicManifest>(&content) {
                        let install_norm = manifest.install_location.to_lowercase().replace('/', "\\");
                        let exec_norm = manifest.launch_executable.to_lowercase().replace('/', "\\");
                        
                        let mut full_exec_path = std::path::PathBuf::from(&install_norm);
                        full_exec_path.push(&exec_norm);
                        let full_exec_str = full_exec_path.to_string_lossy().to_string().to_lowercase();
                        
                        if path_norm == full_exec_str || (path_norm.starts_with(&install_norm) && path_norm.ends_with(&exec_norm)) {
                            let launch_uri = if let (Some(ns), Some(id)) = (&manifest.catalog_namespace, &manifest.catalog_item_id) {
                                format!(
                                    "com.epicgames.launcher://apps/{}%3A{}%3A{}?action=launch&silent=true",
                                    ns, id, manifest.app_name
                                )
                            } else {
                                format!(
                                    "com.epicgames.launcher://apps/{}?action=launch&silent=true",
                                    manifest.app_name
                                )
                            };
                            println!("[Windows Engine] Launching Epic game {} via URI: {}", manifest.app_name, launch_uri);
                            let _ = std::process::Command::new("cmd")
                                .args(["/C", "start", "", &launch_uri])
                                .spawn();
                            return Ok(true);
                        }
                    }
                }
            }
        }
    }
    Ok(false)
}

fn try_launch_riot(app_path: &str) -> Result<bool, String> {
    let app_path_norm = app_path.to_lowercase().replace('/', "\\");
    
    let mut client_path = "C:\\Riot Games\\Riot Client\\RiotClientServices.exe".to_string();
    let installs_json_path = "C:\\ProgramData\\Riot Games\\RiotClientInstalls.json";
    if let Ok(content) = std::fs::read_to_string(installs_json_path) {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(rc_default) = json.get("rc_default").and_then(|v| v.as_str()) {
                client_path = rc_default.replace('/', "\\");
            }
        }
    }
    
    let metadata_dir = "C:\\ProgramData\\Riot Games\\Metadata";
    if let Ok(entries) = std::fs::read_dir(metadata_dir) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                let dir_name = entry.file_name().to_string_lossy().to_string();
                let parts: Vec<&str> = dir_name.split('.').collect();
                if parts.len() >= 2 {
                    let product = parts[0];
                    let patchline = parts[1];
                    
                    let yaml_name = format!("{}.product_settings.yaml", dir_name);
                    let yaml_path = entry.path().join(yaml_name);
                    if yaml_path.exists() {
                        if let Ok(content) = std::fs::read_to_string(&yaml_path) {
                            for line in content.lines() {
                                let trimmed = line.trim();
                                if trimmed.starts_with("product_install_full_path:") {
                                    if let Some(val) = extract_yaml_quoted_value(trimmed) {
                                        let val_norm = val.to_lowercase().replace('/', "\\");
                                        if app_path_norm.starts_with(&val_norm) {
                                            println!("[Windows Engine] Launching Riot game {}.{} via client", product, patchline);
                                            let _ = std::process::Command::new(&client_path)
                                                .args([
                                                    &format!("--launch-product={}", product),
                                                    &format!("--launch-patchline={}", patchline),
                                                ])
                                                .spawn();
                                            return Ok(true);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(false)
}

fn launch_app(app_path: &str) {
    if let Ok(true) = try_launch_steam(app_path) {
        return;
    }
    if let Ok(true) = try_launch_epic(app_path) {
        return;
    }
    if let Ok(true) = try_launch_riot(app_path) {
        return;
    }
    
    // Fallback: spawn direct executable
    println!("[Windows Engine] Spawning direct executable: {}", app_path);
    let _ = std::process::Command::new(app_path).spawn();
}
