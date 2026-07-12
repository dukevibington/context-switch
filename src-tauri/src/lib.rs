pub mod database;
pub mod engine;

use std::sync::mpsc::{channel, Sender};
use std::thread;
use tauri::Manager;

use database::DatabaseManager;
use engine::{create_engine, Workspace};

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct DaemonStats {
    pub memory_bytes: u64,
    pub active_windows: usize,
}

enum BackgroundJob {
    Capture {
        id: String,
        name: String,
        responder: Sender<Result<Workspace, String>>,
    },
    Restore {
        id: String,
        close_others: bool,
        responder: Sender<Result<(), String>>,
    },
    List {
        responder: Sender<Result<Vec<Workspace>, String>>,
    },
    GetStats {
        responder: Sender<Result<DaemonStats, String>>,
    },
    Delete {
        id: String,
        responder: Sender<Result<(), String>>,
    },
    Update {
        id: String,
        responder: Sender<Result<Workspace, String>>,
    },
    ToggleFavorite {
        id: String,
        responder: Sender<Result<bool, String>>,
    },
    GetSetting {
        key: String,
        responder: Sender<Result<Option<String>, String>>,
    },
    SetSetting {
        key: String,
        value: String,
        responder: Sender<Result<(), String>>,
    },
}

pub struct AppState {
    job_tx: Sender<BackgroundJob>,
}

/// Dynamic Process Memory Query using native APIs
fn get_memory_usage() -> u64 {
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;

        unsafe {
            let mut counters: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
            counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
            let process = GetCurrentProcess();
            if GetProcessMemoryInfo(process, &mut counters, counters.cb) != 0 {
                counters.WorkingSetSize as u64
            } else {
                0
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Fallback for macOS and non-windows compilation simulation
        12 * 1024 * 1024
    }
}

/// Asynchronous channel-based worker runner
fn worker_thread_loop(rx: std::sync::mpsc::Receiver<BackgroundJob>) {
    let db_path = database::get_default_db_path();
    let mut db = match DatabaseManager::new(db_path) {
        Ok(manager) => {
            println!("[Daemon] SQLite database connection established successfully.");
            manager
        }
        Err(e) => {
            eprintln!("Failed to initialize ContextSwitch SQLite DB: {:?}", e);
            return;
        }
    };

    let engine = create_engine();
    println!("[Daemon] Active Window Spy Engine loaded.");
    if let Ok(workspaces) = db.list_workspaces() {
        println!("[Daemon] Loaded database with {} saved workspaces.", workspaces.len());
    }

    while let Ok(job) = rx.recv() {
        match job {
            BackgroundJob::Capture { id, name, responder } => {
                let res = (|| -> Result<Workspace, String> {
                    let windows = engine.capture_windows()?;
                    let workspace = Workspace {
                        id,
                        name,
                        created_at: chrono::Utc::now().timestamp(),
                        is_favorite: false,
                        windows,
                    };
                    println!("[Daemon] Capturing workspace '{}' with {} active window(s)...", workspace.name, workspace.windows.len());
                    db.save_workspace(&workspace).map_err(|e| e.to_string())?;
                    println!("[Daemon] Workspace '{}' ({}) captured and saved successfully.", workspace.name, workspace.id);
                    Ok(workspace)
                })();
                let _ = responder.send(res);
            }
            BackgroundJob::Restore { id, close_others, responder } => {
                let res = (|| -> Result<(), String> {
                    let workspace = db
                        .get_workspace_by_id(&id)
                        .map_err(|e| e.to_string())?
                        .ok_or_else(|| "Workspace not found".to_string())?;
                    println!("[Daemon] Restoring workspace '{}' (close_others={})...", workspace.name, close_others);
                    engine.restore_workspace(&workspace, close_others)?;
                    println!("[Daemon] Workspace '{}' restored successfully.", workspace.name);
                    Ok(())
                })();
                let _ = responder.send(res);
            }
            BackgroundJob::List { responder } => {
                let res = db.list_workspaces().map_err(|e| e.to_string());
                let _ = responder.send(res);
            }
            BackgroundJob::GetStats { responder } => {
                let active_count = match engine.capture_windows() {
                    Ok(w) => w.len(),
                    Err(_) => 0,
                };
                let stats = DaemonStats {
                    memory_bytes: get_memory_usage(),
                    active_windows: active_count,
                };
                let _ = responder.send(Ok(stats));
            }
            BackgroundJob::Delete { id, responder } => {
                let res = (|| -> Result<(), String> {
                    let name = db.get_workspace_by_id(&id)
                        .ok()
                        .flatten()
                        .map(|w| w.name)
                        .unwrap_or_else(|| id.clone());
                    println!("[Daemon] Deleting workspace '{}'...", name);
                    db.delete_workspace_by_id(&id).map_err(|e| e.to_string())?;
                    
                    // Clean up thumbnail
                    let mut thumb_path = database::get_default_db_path();
                    thumb_path.pop();
                    thumb_path.push("thumbnails");
                    thumb_path.push(format!("{}.jpg", id));
                    if thumb_path.exists() {
                        let _ = std::fs::remove_file(thumb_path);
                    }
                    println!("[Daemon] Workspace '{}' deleted successfully.", name);
                    Ok(())
                })();
                let _ = responder.send(res);
            }
            BackgroundJob::Update { id, responder } => {
                let res = (|| -> Result<Workspace, String> {
                    let existing = db
                        .get_workspace_by_id(&id)
                        .map_err(|e| e.to_string())?
                        .ok_or_else(|| "Workspace not found".to_string())?;
                    println!("[Daemon] Updating workspace '{}'...", existing.name);
                    let windows = engine.capture_windows()?;
                    let workspace = Workspace {
                        id,
                        name: existing.name.clone(),
                        created_at: chrono::Utc::now().timestamp(),
                        is_favorite: existing.is_favorite,
                        windows,
                    };
                    db.save_workspace(&workspace).map_err(|e| e.to_string())?;
                    println!("[Daemon] Workspace '{}' updated successfully with {} active window(s).", workspace.name, workspace.windows.len());
                    Ok(workspace)
                })();
                let _ = responder.send(res);
            }
            BackgroundJob::ToggleFavorite { id, responder } => {
                let name = db.get_workspace_by_id(&id)
                    .ok()
                    .flatten()
                    .map(|w| w.name)
                    .unwrap_or_else(|| id.clone());
                println!("[Daemon] Toggling favorite status for workspace '{}'...", name);
                let res = db.toggle_workspace_favorite(&id).map_err(|e| e.to_string());
                if let Ok(fav) = &res {
                    println!("[Daemon] Workspace '{}' favorite set to {}.", name, fav);
                }
                let _ = responder.send(res);
            }
            BackgroundJob::GetSetting { key, responder } => {
                let res = db.get_setting(&key).map_err(|e| e.to_string());
                let _ = responder.send(res);
            }
            BackgroundJob::SetSetting { key, value, responder } => {
                let res = db.set_setting(&key, &value).map_err(|e| e.to_string());
                let _ = responder.send(res);
            }
        }
    }
}

fn capture_and_save_screenshot(workspace_id: &str) -> Result<(), String> {
    use xcap::Monitor;
    
    let monitors = Monitor::all().map_err(|e| e.to_string())?;
    if monitors.is_empty() {
        return Err("No monitors found".to_string());
    }
    
    // Find virtual desktop bounding box
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    
    for monitor in &monitors {
        let x = monitor.x().unwrap_or(0);
        let y = monitor.y().unwrap_or(0);
        let w = monitor.width().unwrap_or(0) as i32;
        let h = monitor.height().unwrap_or(0) as i32;
        
        if x < min_x { min_x = x; }
        if y < min_y { min_y = y; }
        if x + w > max_x { max_x = x + w; }
        if y + h > max_y { max_y = y + h; }
    }
    
    let total_width = (max_x - min_x) as u32;
    let total_height = (max_y - min_y) as u32;
    
    if total_width == 0 || total_height == 0 {
        return Err("Invalid virtual screen dimensions".to_string());
    }
    
    // Create black background canvas
    let mut stitched = image::ImageBuffer::from_pixel(
        total_width,
        total_height,
        image::Rgba([0, 0, 0, 255]),
    );
    
    // Stitch monitors
    for monitor in &monitors {
        if let Ok(img) = monitor.capture_image() {
            let offset_x = (monitor.x().unwrap_or(0) - min_x) as u32;
            let offset_y = (monitor.y().unwrap_or(0) - min_y) as u32;
            image::imageops::replace(&mut stitched, &img, offset_x as i64, offset_y as i64);
        }
    }
    
    // Resolve thumbnails folder
    let mut thumb_path = database::get_default_db_path();
    thumb_path.pop(); // ContextSwitch
    thumb_path.push("thumbnails");
    let _ = std::fs::create_dir_all(&thumb_path);
    thumb_path.push(format!("{}.jpg", workspace_id));
    
    // Convert RgbaImage to RgbImage since Jpeg doesn't support alpha channel
    let rgb_image = image::DynamicImage::ImageRgba8(stitched).into_rgb8();
    
    // Save image
    rgb_image
        .save_with_format(&thumb_path, image::ImageFormat::Jpeg)
        .map_err(|e| e.to_string())?;
        
    Ok(())
}

// IPC COMMAND EXPORTS FOR TAURI FRONTEND
pub mod commands {
    use super::{AppState, BackgroundJob, DaemonStats, Workspace};
    use std::sync::mpsc::channel;
    use tauri::State;

    #[tauri::command]
    pub async fn capture_current_workspace(
        name: String,
        capture_screenshot: bool,
        window: tauri::Window,
        state: State<'_, AppState>,
    ) -> Result<Workspace, String> {
        let workspace_id = uuid::Uuid::new_v4().to_string();

        if capture_screenshot {
            // Hide dashboard
            let _ = window.hide();
            // Sleep 150ms to let Windows repaint
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            // Capture
            if let Err(e) = super::capture_and_save_screenshot(&workspace_id) {
                eprintln!("Failed to capture workspace screenshot: {:?}", e);
            }
        } else {
            let _ = window.hide();
        }

        let (tx, rx) = channel();
        state
            .job_tx
            .send(BackgroundJob::Capture { id: workspace_id, name, responder: tx })
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    #[tauri::command]
    pub async fn restore_workspace_by_id(
        id: String,
        close_others: bool,
        state: State<'_, AppState>,
    ) -> Result<(), String> {
        let (tx, rx) = channel();
        state
            .job_tx
            .send(BackgroundJob::Restore { id, close_others, responder: tx })
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    #[tauri::command]
    pub async fn list_workspaces(state: State<'_, AppState>) -> Result<Vec<Workspace>, String> {
        let (tx, rx) = channel();
        state
            .job_tx
            .send(BackgroundJob::List { responder: tx })
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    #[tauri::command]
    pub async fn get_daemon_stats(state: State<'_, AppState>) -> Result<DaemonStats, String> {
        let (tx, rx) = channel();
        state
            .job_tx
            .send(BackgroundJob::GetStats { responder: tx })
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    #[tauri::command]
    pub async fn delete_workspace_by_id(
        id: String,
        state: State<'_, AppState>,
    ) -> Result<(), String> {
        let (tx, rx) = channel();
        state
            .job_tx
            .send(BackgroundJob::Delete { id, responder: tx })
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    #[tauri::command]
    pub fn get_workspace_thumbnail_path(id: String) -> Option<String> {
        let mut path = super::database::get_default_db_path();
        path.pop();
        path.push("thumbnails");
        path.push(format!("{}.jpg", id));
        if path.exists() {
            Some(path.to_string_lossy().to_string())
        } else {
            None
        }
    }

    #[tauri::command]
    pub fn open_thumbnail_in_system_viewer(id: String) -> Result<(), String> {
        let mut path = super::database::get_default_db_path();
        path.pop();
        path.push("thumbnails");
        path.push(format!("{}.jpg", id));
        
        if path.exists() {
            #[cfg(target_os = "windows")]
            {
                std::process::Command::new("cmd")
                    .args(["/C", "start", "", &path.to_string_lossy()])
                    .spawn()
                    .map_err(|e| e.to_string())?;
            }
            #[cfg(not(target_os = "windows"))]
            {
                #[cfg(target_os = "macos")]
                let cmd = "open";
                #[cfg(target_os = "linux")]
                let cmd = "xdg-open";
                
                std::process::Command::new(cmd)
                    .arg(&path)
                    .spawn()
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    #[tauri::command]
    pub async fn update_workspace_by_id(
        id: String,
        capture_screenshot: bool,
        window: tauri::Window,
        state: State<'_, AppState>,
    ) -> Result<Workspace, String> {
        if capture_screenshot {
            // Hide dashboard
            let _ = window.hide();
            // Sleep 150ms to let Windows repaint
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            // Capture and overwrite thumbnail
            if let Err(e) = super::capture_and_save_screenshot(&id) {
                eprintln!("Failed to update workspace screenshot: {:?}", e);
            }
            // Show and focus dashboard (since update shouldn't close the app)
            let _ = window.show();
            let _ = window.set_focus();
        }

        let (tx, rx) = channel();
        state
            .job_tx
            .send(BackgroundJob::Update { id, responder: tx })
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    #[tauri::command]
    pub async fn toggle_workspace_favorite(
        id: String,
        state: State<'_, AppState>,
    ) -> Result<bool, String> {
        let (tx, rx) = channel();
        state
            .job_tx
            .send(BackgroundJob::ToggleFavorite { id, responder: tx })
            .map_err(|e| e.to_string())?;
        rx.recv().map_err(|e| e.to_string())?
    }

    #[tauri::command]
    pub async fn update_hotkey(
        new_hotkey: String,
        app: tauri::AppHandle,
        state: State<'_, AppState>,
    ) -> Result<(), String> {
        let new_shortcut = new_hotkey.parse::<tauri_plugin_global_shortcut::Shortcut>()
            .map_err(|e| format!("Invalid shortcut: {}", e))?;

        let (tx_get, rx_get) = channel();
        state
            .job_tx
            .send(BackgroundJob::GetSetting { key: "hotkey".to_string(), responder: tx_get })
            .map_err(|e| e.to_string())?;
        let old_hotkey = rx_get.recv().map_err(|e| e.to_string())??
            .unwrap_or_else(|| "alt+space".to_string());

        use tauri_plugin_global_shortcut::GlobalShortcutExt;
        if let Ok(old_shortcut) = old_hotkey.parse::<tauri_plugin_global_shortcut::Shortcut>() {
            let _ = app.global_shortcut().unregister(old_shortcut);
        }

        app.global_shortcut().register(new_shortcut)
            .map_err(|e| format!("Failed to register hotkey: {}", e))?;

        let (tx_set, rx_set) = channel();
        state
            .job_tx
            .send(BackgroundJob::SetSetting { key: "hotkey".to_string(), value: new_hotkey, responder: tx_set })
            .map_err(|e| e.to_string())?;
        rx_set.recv().map_err(|e| e.to_string())??;

        Ok(())
    }

    #[tauri::command]
    pub async fn get_current_hotkey(state: State<'_, AppState>) -> Result<String, String> {
        let (tx, rx) = channel();
        state
            .job_tx
            .send(BackgroundJob::GetSetting { key: "hotkey".to_string(), responder: tx })
            .map_err(|e| e.to_string())?;
        let hotkey = rx.recv().map_err(|e| e.to_string())??
            .unwrap_or_else(|| "alt+space".to_string());
        Ok(hotkey)
    }
}

#[cfg(target_os = "windows")]
fn add_to_path() {
    use winreg::enums::*;
    use winreg::RegKey;

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let exe_dir_str = exe_dir.to_string_lossy().to_string();
            let hkcu = RegKey::predef(HKEY_CURRENT_USER);
            if let Ok((env_key, _)) = hkcu.create_subkey("Environment") {
                let current_path: String = env_key.get_value("Path").unwrap_or_default();
                
                // Avoid double insertion
                let has_path = current_path.split(';').any(|p| {
                    let cleaned_p = p.trim().trim_end_matches('\\');
                    let cleaned_exe = exe_dir_str.trim().trim_end_matches('\\');
                    cleaned_p.eq_ignore_ascii_case(cleaned_exe)
                });

                if !has_path {
                    let new_path = if current_path.is_empty() {
                        exe_dir_str.clone()
                    } else {
                        format!("{};{}", current_path, exe_dir_str)
                    };
                    if env_key.set_value("Path", &new_path).is_ok() {
                        println!("[Daemon] Added ContextSwitch to user PATH environment variable: {}", exe_dir_str);
                        
                        // Broadcast environment update to standard explorer shell
                        unsafe {
                            use windows_sys::Win32::UI::WindowsAndMessaging::{
                                SendMessageTimeoutW, HWND_BROADCAST, WM_SETTINGCHANGE, SMTO_ABORTIFHUNG
                            };
                            let mut result = 0;
                            let param = "Environment\0".encode_utf16().collect::<Vec<u16>>();
                            let _ = SendMessageTimeoutW(
                                HWND_BROADCAST,
                                WM_SETTINGCHANGE,
                                0,
                                param.as_ptr() as _,
                                SMTO_ABORTIFHUNG,
                                5000,
                                &mut result
                            );
                        }
                    }
                }
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let (job_tx, job_rx) = channel();

    // Spawn background worker thread
    thread::spawn(move || {
        worker_thread_loop(job_rx);
    });

    tauri::Builder::default()
        .manage(AppState { job_tx })
        .invoke_handler(tauri::generate_handler![
            commands::capture_current_workspace,
            commands::restore_workspace_by_id,
            commands::list_workspaces,
            commands::get_daemon_stats,
            commands::delete_workspace_by_id,
            commands::update_workspace_by_id,
            commands::get_workspace_thumbnail_path,
            commands::open_thumbnail_in_system_viewer,
            commands::toggle_workspace_favorite,
            commands::update_hotkey,
            commands::get_current_hotkey
        ])
        .plugin(tauri_plugin_global_shortcut::Builder::new().with_handler(|app, _shortcut, event| {
            use tauri_plugin_global_shortcut::ShortcutState;
            if event.state() == ShortcutState::Pressed {
                // Any registered global shortcut triggers toggle behavior
                if let Some(window) = app.get_webview_window("main") {
                    let is_visible = window.is_visible().unwrap_or(false);
                    if is_visible {
                        let _ = window.hide();
                    } else {
                        let _ = window.show();
                        let _ = window.set_focus();

                        // Explicitly force HWND_TOPMOST in Win32 to ensure overlay precedence over borderless games
                        #[cfg(target_os = "windows")]
                        {
                            if let Ok(hwnd) = window.hwnd() {
                                unsafe {
                                    use windows_sys::Win32::UI::WindowsAndMessaging::{
                                        SetWindowPos, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, SWP_SHOWWINDOW
                                    };
                                    let _ = SetWindowPos(
                                        hwnd.0 as _,
                                        HWND_TOPMOST,
                                        0, 0, 0, 0,
                                        SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }).build())
        .plugin(tauri_plugin_window_state::Builder::default()
            .with_state_flags(tauri_plugin_window_state::StateFlags::all() & !tauri_plugin_window_state::StateFlags::VISIBLE)
            .build()
        )
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .setup(|app| {
            let (tx, rx) = channel();
            let _ = app.state::<AppState>().job_tx.send(BackgroundJob::GetSetting {
                key: "hotkey".to_string(),
                responder: tx,
            });
            
            let hotkey_str = rx.recv()
                .ok()
                .and_then(|r| r.ok())
                .flatten()
                .unwrap_or_else(|| "alt+space".to_string());

            println!("[Daemon] Registering global hotkey: {}", hotkey_str);

            use tauri_plugin_global_shortcut::GlobalShortcutExt;
            if let Ok(shortcut) = hotkey_str.parse::<tauri_plugin_global_shortcut::Shortcut>() {
                if let Err(e) = app.global_shortcut().register(shortcut) {
                    eprintln!("Failed to register startup hotkey '{}': {:?}", hotkey_str, e);
                }
            }

            #[cfg(target_os = "windows")]
            {
                add_to_path();
            }

            // Create system tray icon with Open and Quit items
            use tauri::menu::{MenuBuilder, MenuItem};
            use tauri::tray::{TrayIconBuilder, TrayIconEvent, MouseButton, MouseButtonState};

            let open_i = MenuItem::with_id(app, "open", "Open ContextSwitch", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = MenuBuilder::new(app)
                .item(&open_i)
                .separator()
                .item(&quit_i)
                .build()?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    match event.id.0.as_str() {
                        "open" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    match event {
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } => {
                            let app = tray.app_handle();
                            if let Some(window) = app.get_webview_window("main") {
                                let is_visible = window.is_visible().unwrap_or(false);
                                if is_visible {
                                    let _ = window.hide();
                                } else {
                                    let _ = window.show();
                                    let _ = window.set_focus();
                                }
                            }
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            // First-launch detection: show the dashboard on initial install
            let (first_tx, first_rx) = channel();
            let _ = app.state::<AppState>().job_tx.send(BackgroundJob::GetSetting {
                key: "launched_before".to_string(),
                responder: first_tx,
            });
            let launched_before = first_rx.recv()
                .ok()
                .and_then(|r| r.ok())
                .flatten()
                .is_some();

            if !launched_before {
                // First time running — show the window so the user sees something
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
                // Mark as launched so future startups stay hidden
                let (set_tx, _set_rx) = channel();
                let _ = app.state::<AppState>().job_tx.send(BackgroundJob::SetSetting {
                    key: "launched_before".to_string(),
                    value: "true".to_string(),
                    responder: set_tx,
                });
                println!("[Daemon] First launch detected — showing dashboard.");
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}



