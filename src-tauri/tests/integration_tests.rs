use context_switch_daemon::database::DatabaseManager;
use context_switch_daemon::engine::{create_engine, WindowState, Workspace};

#[test]
fn test_serialization() {
    let ws = Workspace {
        id: "test-uuid-1234".to_string(),
        name: "Test Workspace".to_string(),
        created_at: 1620000000,
        windows: vec![WindowState {
            id: None,
            app_name: "notepad.exe".to_string(),
            process_id: 1234,
            window_title: "Untitled - Notepad".to_string(),
            x: 100,
            y: 150,
            width: 800,
            height: 600,
            is_minimized: false,
            is_maximized: false,
        }],
    };

    let serialized = serde_json::to_string(&ws).expect("Failed to serialize");
    let deserialized: Workspace = serde_json::from_str(&serialized).expect("Failed to deserialize");
    
    assert_eq!(deserialized.id, ws.id);
    assert_eq!(deserialized.name, ws.name);
    assert_eq!(deserialized.windows.len(), 1);
    assert_eq!(deserialized.windows[0].app_name, "notepad.exe");
    assert_eq!(deserialized.windows[0].x, 100);
    assert_eq!(deserialized.windows[0].is_minimized, false);
}

#[test]
fn test_database_persistence() {
    let mut db = DatabaseManager::new_in_memory().expect("Failed to create in-memory DB");
    
    let ws = Workspace {
        id: "test-uuid-5678".to_string(),
        name: "Coding Layout".to_string(),
        created_at: 1625000000,
        windows: vec![
            WindowState {
                id: None,
                app_name: "code.exe".to_string(),
                process_id: 5678,
                window_title: "VS Code".to_string(),
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
                is_minimized: false,
                is_maximized: true,
            },
            WindowState {
                id: None,
                app_name: "chrome.exe".to_string(),
                process_id: 9012,
                window_title: "Chrome Browser".to_string(),
                x: 100,
                y: 100,
                width: 800,
                height: 600,
                is_minimized: true,
                is_maximized: false,
            }
        ],
    };

    // Save
    db.save_workspace(&ws).expect("Failed to save workspace");

    // Retrieve
    let retrieved_opt = db.get_workspace_by_id("test-uuid-5678").expect("Failed to query workspace");
    assert!(retrieved_opt.is_some());
    let retrieved = retrieved_opt.unwrap();
    
    assert_eq!(retrieved.id, ws.id);
    assert_eq!(retrieved.name, ws.name);
    assert_eq!(retrieved.created_at, ws.created_at);
    assert_eq!(retrieved.windows.len(), 2);

    // Assert sorting and fields
    let code_win = retrieved.windows.iter().find(|w| w.app_name == "code.exe").unwrap();
    assert_eq!(code_win.x, 0);
    assert_eq!(code_win.is_maximized, true);
    assert_eq!(code_win.is_minimized, false);

    let chrome_win = retrieved.windows.iter().find(|w| w.app_name == "chrome.exe").unwrap();
    assert_eq!(chrome_win.is_minimized, true);
    assert_eq!(chrome_win.is_maximized, false);

    // Test list
    let list = db.list_workspaces().expect("Failed to list workspaces");
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "test-uuid-5678");
}

#[test]
fn test_engine_initialization() {
    let engine = create_engine();
    
    #[cfg(target_os = "windows")]
    {
        let res = engine.capture_windows();
        assert!(res.is_ok(), "Engine scanning failed: {:?}", res.err());
        let windows = res.unwrap();
        println!("Detected {} active windows on Windows host", windows.len());
        // In some testing environments, there could be no visible windows,
        // but normally we should check if they parse correctly if windows exist.
        for win in &windows {
            assert!(win.width > 0);
            assert!(win.height > 0);
            assert!(!win.app_name.is_empty());
        }
    }
}
