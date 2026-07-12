use rusqlite::{params, Connection, Result};
use std::path::PathBuf;
use super::engine::{WindowState, Workspace};

pub struct DatabaseManager {
    conn: Connection,
}

impl DatabaseManager {
    /// Connect to a database file at the specified path and initialize tables.
    pub fn new(path: PathBuf) -> Result<Self> {
        let conn = Connection::open(path)?;
        let mut db = Self { conn };
        db.init_tables()?;
        Ok(db)
    }

    /// Open an in-memory SQLite database (ideal for unit testing).
    pub fn new_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let mut db = Self { conn };
        db.init_tables()?;
        Ok(db)
    }

    /// Configures SQLite parameters and initializes tables.
    fn init_tables(&mut self) -> Result<()> {
        // Enable foreign key constraints
        self.conn.execute("PRAGMA foreign_keys = ON;", [])?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS workspaces (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                created_at INTEGER NOT NULL
            );",
            [],
        )?;

        let _ = self.conn.execute(
            "ALTER TABLE workspaces ADD COLUMN is_favorite INTEGER NOT NULL DEFAULT 0;",
            [],
        );

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS window_states (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                workspace_id TEXT NOT NULL,
                app_name TEXT NOT NULL,
                process_id INTEGER NOT NULL,
                window_title TEXT NOT NULL,
                x INTEGER NOT NULL,
                y INTEGER NOT NULL,
                width INTEGER NOT NULL,
                height INTEGER NOT NULL,
                is_minimized INTEGER NOT NULL,
                is_maximized INTEGER NOT NULL,
                FOREIGN KEY(workspace_id) REFERENCES workspaces(id) ON DELETE CASCADE
            );",
            [],
        )?;

        self.conn.execute(
            "CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            );",
            [],
        )?;

        // Seed default hotkey if not present
        let hotkey_check: Result<i32> = self.conn.query_row(
            "SELECT 1 FROM settings WHERE key = 'hotkey';",
            [],
            |_| Ok(1),
        );
        if hotkey_check.is_err() {
            let _ = self.conn.execute(
                "INSERT INTO settings (key, value) VALUES ('hotkey', 'alt+space');",
                [],
            );
        }

        Ok(())
    }

    /// Persists a Workspace and all its child WindowStates in a single atomic transaction.
    pub fn save_workspace(&mut self, workspace: &Workspace) -> Result<()> {
        let tx = self.conn.transaction()?;

        // Save workspace metadata (UPSERT)
        tx.execute(
            "INSERT OR REPLACE INTO workspaces (id, name, created_at, is_favorite) VALUES (?1, ?2, ?3, ?4);",
            params![
                workspace.id,
                workspace.name,
                workspace.created_at,
                if workspace.is_favorite { 1 } else { 0 }
            ],
        )?;

        // Clear existing windows to overwrite state cleanly
        tx.execute(
            "DELETE FROM window_states WHERE workspace_id = ?1;",
            params![workspace.id],
        )?;

        // Insert fresh window layouts
        let mut stmt = tx.prepare(
            "INSERT INTO window_states (
                workspace_id, app_name, process_id, window_title, x, y, width, height, is_minimized, is_maximized
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10);",
        )?;

        for win in &workspace.windows {
            stmt.execute(params![
                workspace.id,
                win.app_name,
                win.process_id,
                win.window_title,
                win.x,
                win.y,
                win.width,
                win.height,
                if win.is_minimized { 1 } else { 0 },
                if win.is_maximized { 1 } else { 0 }
            ])?;
        }

        stmt.finalize()?;
        tx.commit()?;
        Ok(())
    }

    /// Retrieves a Workspace by its unique UUID ID.
    pub fn get_workspace_by_id(&self, id: &str) -> Result<Option<Workspace>> {
        let mut stmt = self.conn.prepare("SELECT name, created_at, is_favorite FROM workspaces WHERE id = ?1;")?;
        
        let ws_opt = stmt.query_row(params![id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i32>(2)?))
        });

        let (name, created_at, is_fav_val) = match ws_opt {
            Ok(data) => data,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e),
        };

        // Query the child window configurations
        let mut win_stmt = self.conn.prepare(
            "SELECT id, app_name, process_id, window_title, x, y, width, height, is_minimized, is_maximized 
             FROM window_states WHERE workspace_id = ?1;"
        )?;

        let windows_iter = win_stmt.query_map(params![id], |row| {
            Ok(WindowState {
                id: Some(row.get::<_, i32>(0)?),
                app_name: row.get::<_, String>(1)?,
                process_id: row.get::<_, u32>(2)?,
                window_title: row.get::<_, String>(3)?,
                x: row.get::<_, i32>(4)?,
                y: row.get::<_, i32>(5)?,
                width: row.get::<_, i32>(6)?,
                height: row.get::<_, i32>(7)?,
                is_minimized: row.get::<_, i32>(8)? != 0,
                is_maximized: row.get::<_, i32>(9)? != 0,
            })
        })?;

        let mut windows = Vec::new();
        for win in windows_iter {
            windows.push(win?);
        }

        Ok(Some(Workspace {
            id: id.to_string(),
            name,
            created_at,
            is_favorite: is_fav_val != 0,
            windows,
        }))
    }

    /// Returns a list of all stored Workspaces sorted by creation date.
    pub fn list_workspaces(&self) -> Result<Vec<Workspace>> {
        let mut stmt = self.conn.prepare("SELECT id FROM workspaces ORDER BY created_at DESC;")?;
        let ids_iter = stmt.query_map([], |row| row.get::<_, String>(0))?;

        let mut workspaces = Vec::new();
        for id_res in ids_iter {
            let id = id_res?;
            if let Ok(Some(ws)) = self.get_workspace_by_id(&id) {
                workspaces.push(ws);
            }
        }

        Ok(workspaces)
    }

    /// Deletes a Workspace and all its associated WindowStates from the database.
    pub fn delete_workspace_by_id(&mut self, id: &str) -> Result<()> {
        self.conn.execute("DELETE FROM workspaces WHERE id = ?1;", [id])?;
        Ok(())
    }

    /// Toggles the favorite status of a Workspace and returns the new state.
    pub fn toggle_workspace_favorite(&mut self, id: &str) -> Result<bool> {
        let current: i32 = self.conn.query_row(
            "SELECT is_favorite FROM workspaces WHERE id = ?1;",
            [id],
            |row| row.get(0),
        )?;
        let new_state = if current == 0 { 1 } else { 0 };
        self.conn.execute(
            "UPDATE workspaces SET is_favorite = ?1 WHERE id = ?2;",
            params![new_state, id],
        )?;
        Ok(new_state != 0)
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare("SELECT value FROM settings WHERE key = ?1;")?;
        let mut rows = stmt.query(params![key])?;
        if let Some(row) = rows.next()? {
            let val: String = row.get(0)?;
            Ok(Some(val))
        } else {
            Ok(None)
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO settings (key, value) VALUES (?1, ?2);",
            params![key, value],
        )?;
        Ok(())
    }
}

/// Helper to get the standard OS application data folder for ContextSwitch.
pub fn get_default_db_path() -> PathBuf {
    let mut path = if cfg!(target_os = "windows") {
        if let Ok(app_data) = std::env::var("LOCALAPPDATA") {
            PathBuf::from(app_data)
        } else {
            PathBuf::from("C:\\temp")
        }
    } else if cfg!(target_os = "macos") {
        if let Ok(home) = std::env::var("HOME") {
            let mut p = PathBuf::from(home);
            p.push("Library");
            p.push("Application Support");
            p
        } else {
            PathBuf::from("/tmp")
        }
    } else {
        PathBuf::from(".")
    };

    path.push("ContextSwitch");
    let _ = std::fs::create_dir_all(&path);
    path.push("context_switch_data.db");
    path
}
