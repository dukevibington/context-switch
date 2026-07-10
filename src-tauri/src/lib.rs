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
        Ok(manager) => manager,
        Err(e) => {
            eprintln!("Failed to initialize ContextSwitch SQLite DB: {:?}", e);
            return;
        }
    };

    let engine = create_engine();

    while let Ok(job) = rx.recv() {
        match job {
            BackgroundJob::Capture { id, name, responder } => {
                let res = (|| -> Result<Workspace, String> {
                    let windows = engine.capture_windows()?;
                    let workspace = Workspace {
                        id,
                        name,
                        created_at: chrono::Utc::now().timestamp(),
                        windows,
                    };
                    db.save_workspace(&workspace).map_err(|e| e.to_string())?;
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
                    engine.restore_workspace(&workspace, close_others)?;
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
                    db.delete_workspace_by_id(&id).map_err(|e| e.to_string())?;
                    
                    // Clean up thumbnail
                    let mut thumb_path = database::get_default_db_path();
                    thumb_path.pop();
                    thumb_path.push("thumbnails");
                    thumb_path.push(format!("{}.jpg", id));
                    if thumb_path.exists() {
                        let _ = std::fs::remove_file(thumb_path);
                    }
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

                    let windows = engine.capture_windows()?;
                    let workspace = Workspace {
                        id,
                        name: existing.name,
                        created_at: chrono::Utc::now().timestamp(),
                        windows,
                    };
                    db.save_workspace(&workspace).map_err(|e| e.to_string())?;
                    Ok(workspace)
                })();
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
            commands::open_thumbnail_in_system_viewer
        ])
        .plugin(tauri_plugin_global_shortcut::Builder::new().with_handler(|app, shortcut, event| {
            use tauri_plugin_global_shortcut::{Code, Modifiers, ShortcutState};
            if event.state() == ShortcutState::Pressed {
                if shortcut.key == Code::Space && shortcut.mods.contains(Modifiers::ALT) {
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
                                        SetWindowPos(
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
            }
        }).build())
        .setup(|app| {
            use tauri_plugin_global_shortcut::{Code, Modifiers, Shortcut, GlobalShortcutExt};
            let shortcut = Shortcut::new(Some(Modifiers::ALT), Code::Space);
            let _ = app.global_shortcut().register(shortcut);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}



