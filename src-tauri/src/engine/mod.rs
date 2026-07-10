use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    pub id: Option<i32>, // Optional database primary key id
    pub app_name: String,
    pub process_id: u32,
    pub window_title: String,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub is_minimized: bool,
    pub is_maximized: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String, // UUID v4
    pub name: String,
    pub created_at: i64,
    pub windows: Vec<WindowState>,
}

pub trait WindowSpyEngine {
    /// Captures the state of all active, visible user application windows
    fn capture_windows(&self) -> Result<Vec<WindowState>, String>;
    
    /// Restores coordinates and layout of the saved workspace windows
    fn restore_workspace(&self, workspace: &Workspace, close_others: bool) -> Result<(), String>;
}

#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub mod dummy {
    use super::{WindowSpyEngine, WindowState, Workspace};

    pub struct DummyWindowSpy;

    impl DummyWindowSpy {
        pub fn new() -> Self {
            Self
        }
    }

    impl WindowSpyEngine for DummyWindowSpy {
        fn capture_windows(&self) -> Result<Vec<WindowState>, String> {
            Ok(vec![])
        }

        fn restore_workspace(&self, _workspace: &Workspace, _close_others: bool) -> Result<(), String> {
            Ok(())
        }
    }
}

/// Factory function to retrieve the active Window Spy Engine for the target OS
pub fn create_engine() -> Box<dyn WindowSpyEngine + Send + Sync> {
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsWindowSpy::new())
    }
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacosWindowSpy::new())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Box::new(dummy::DummyWindowSpy::new())
    }
}
