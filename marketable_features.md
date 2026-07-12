# Marketable Features of ContextSwitch

ContextSwitch is an ultra-fast, lightweight background daemon and window state manager built on **Tauri 2.0** and **Rust**. It empowers developers, creators, gamers, and power users to seamlessly toggle between different desktop states—capturing coordinates, positions, and operational contexts of active applications, and restoring them with a single action.

Below is a detailed breakdown of the marketable features that make ContextSwitch a premium addition to any productivity suite.

---

## 🚀 Key Marketable Features

### 1. Instant Desktop State Capture & Restore
Never lose your train of thought or waste time repositioning windows when switching tasks. ContextSwitch captures the exact geometry of your active applications.
*   **Complete Layout Capture**: Automatically records coordinates ($X$, $Y$), dimensions ($Width$, $Height$), and window states (maximized, minimized, or normal).
*   **Window Focus Z-Order Reconstruction**: When restoring, applications are reactivated in the exact **reverse Z-order** of capture. This guarantees that window stacking, overlapping layers, and the active foreground focus are perfectly reconstructed.
*   **Smart OS Exclusions**: Employs low-level OS filters to skip system clutter such as background processes, taskbars, tooltips, flyout menu overlays, and Alt+Tab interfaces (e.g., filtering Windows class names like `Progman`, `WorkerW`, `shell_traywnd`, and `multitaskingviewframe`).
*   **Folder-Only File Explorer Captures**: Intelligently matches real file manager folders (e.g., `CabinetWClass` windows on Windows) while discarding explorer.exe helper widgets.

---

### 2. PC Gaming Platform Integrations
Unlike generic window managers that fail when launching games or opening client-managed apps, ContextSwitch has specialized launchers built natively for major PC gaming clients:
*   🎮 **Steam Integration**: Scans local Steam directory `.acf` appmanifests to identify installed games and launches them natively via the `steam://rungameid/<id>` protocol. This ensures games launch with Steam overlay support and accurate time tracking.
*   🕹️ **Epic Games Store Integration**: Parses Epic's local manifest `.item` JSON logs to retrieve deep-linking parameters, launching games using Epic’s URI scheme: `com.epicgames.launcher://apps/<namespace>:<item_id>:<app_name>?action=launch`.
*   ⚔️ **Riot Games Integration**: Resolves installation metadata from `.product_settings.yaml` and executes Riot Client commands (such as `--launch-product` and `--launch-patchline`) to boot Riot games like *Valorant* or *League of Legends* without manual navigation.

---

### 3. Stitched Multi-Monitor Visual Previews
ContextSwitch provides high-fidelity visual representations of your workspaces.
*   **Multi-Monitor Bounding Canvas**: Identifies the coordinates of all connected screens and dynamically creates a stitched, black-background virtual desktop canvas.
*   **Instant Screen Stitches**: Captures high-definition monitor images (via the `xcap` crate) and places them side-by-side using screen offsets.
*   **System Viewer Integration**: Click on any workspace preview card to launch the captured visual within the default OS image viewer.

---

### 4. Global HUD Dashboard Overlay
An unobtrusive UI that is always one keystroke away.
*   ⚡ **Alt + Space Hotkey**: Instantly toggle the dashboard visibility with a universal shortcut handler.
*   👑 **HWND_TOPMOST Precedence**: Custom Win32 hooks force the dashboard to take display precedence over all active windows—even borderless full-screen games.
*   📸 **Self-Hiding Capture**: Automatically hides the dashboard before taking workspace screenshots to ensure your workspace thumbnails are clean and context-focused.

---

### 5. Desktop Cleaning Modes
Choose how to handle applications that are not part of your restored workspace:
*   🧹 **Restore & Close Other Apps (Destructive)**: Automatically closes all unrelated open windows (`WM_CLOSE` trigger), giving you a blank slate for your new task.
*   📉 **Restore & Minimize (Non-Destructive)**: Minimizes all unrelated applications to the taskbar, keeping them active but out of sight.
*   🌿 **Standard Restore (Ambient)**: Repositions workspace windows while leaving other applications untouched in the background.

---

### 6. Local-First Architecture
Keep your data completely private and secure on your own machine.
*   🗃️ **SQLite Persistence**: Workspaces and window configurations are saved to a local SQLite database (`rusqlite`) in standard platform appdata paths (e.g. `%LOCALAPPDATA%` on Windows, `~/Library/Application Support` on macOS).
*   🛡️ **Atomic Transactions**: Utilizes database transactions to ensure workspace captures and window layout overwrites are saved safely and atomically with zero corruption risk.

---

## 📊 Technical Comparison & Capabilities

| Feature Capability | ContextSwitch | Standard OS Virtual Desktops | Basic Window Managers |
| :--- | :---: | :---: | :---: |
| **Save Geometry (Size/Position)** | **Yes** | No | Yes |
| **Cross-Boot Persistence** | **Yes** (Runs across reboots) | No | No |
| **Gaming Client Hooking** | **Yes** (Steam/Epic/Riot) | No | No |
| **Multi-Monitor Previews** | **Yes** (Stitched Canvas) | No | No |
| **Destructive Desktop Clean** | **Yes** (Close others) | No | No |
| **Z-Order Focus Restore** | **Yes** | No | No |
| **Global Shortcut Overlay** | **Yes** | Yes (Built-in) | No |
| **SQLite Local Backing** | **Yes** | No | No |

---

## 🛠️ Performance Metrics & footprint

*   **Runtime Engine**: Rust-backed background daemon for low memory overhead and rapid FFI calls.
*   **Real-time Metrics**: Built-in resource diagnostics panel showing RAM consumption, total saved workspaces, and active window counts.
*   **Simulated Footprint**: $\approx 12\text{ MB}$ RAM consumption at idle.
