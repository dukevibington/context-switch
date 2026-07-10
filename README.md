# ContextSwitch 🚀

An ultra-fast, lightweight desktop workspace and window layout manager built on **Tauri v2** and **Rust**. 

ContextSwitch runs quietly in the background as a system utility daemon, allowing you to instantly capture the geometry of all your active applications and restore them later with a single action or global hotkey.

---

## ✨ Core Features

*   **Desktop Layout Capture & Restore**: Automatically records exact window coordinates ($X$, $Y$), sizes ($Width$, $Height$), and states (normal, minimized, or maximized) for all running applications.
*   **Z-Order Focus Reconstruction**: Restores applications in the exact **reverse Z-order** of capture, ensuring your overlapping windows and focus layers are stacked perfectly.
*   **Stitched Multi-Monitor Previews**: Identifies all connected displays and dynamically generates stitched virtual monitor preview thumbnails of your saved workspaces.
*   **Global HUD Dashboard Overlay**: A borderless utility dashboard overlay that toggles instantly using a universal global hotkey. It takes topmost precedence over borderless apps and games.
*   **Customizable Global Hotkeys**: Bind any combination (e.g. `Alt+Space` or `Ctrl+Shift+H`) to show/hide the HUD, fully persistable in a local database.
*   **Launch on System Startup**: Toggle startup launch inside the dashboard. Because the app starts completely hidden (`"visible": false`), it runs silently in the background when your system boots, waiting for your hotkey.
*   **Sleek Multi-Theme Interface**: A beautiful glassmorphic panel equipped with multiple premium themes (Default, Kinetic Dark, Fluent Clarity Light Mode, Lumina Precision) standardized on the modern **Geist** font family.
*   **Local-First Architecture**: Zero cloud dependencies. Workspaces, window states, and configurations are stored in an atomic local SQLite database.

---

## 📸 How it Works (GIF Demo)

*(Place a GIF recording here, e.g. `media/context_switch_demo.gif`)*

1. **Capture**: Arrange your IDE, browser, and documentation. Open ContextSwitch, click **Capture Current Workspace**, and name it.
2. **Restore**: Switch workspaces dynamically by clicking **Restore** (ambient repositioning) or **Restore + Close** (closes other applications for a completely clean layout).

---

## 🛠️ Local Development & Build Setup

### Prerequisites
*   **Node.js**: LTS version (v18+)
*   **Rust**: Stable toolchain (`rustup`)
*   **OS Build Tools**: Windows C++ build tools (via Visual Studio Installer)

### Installation & Run
1. Clone the repository and install dependencies:
   ```bash
   npm install
   ```
2. Start the application in development mode:
   ```bash
   npm run tauri dev
   ```

### Local Production Build
To compile the standalone release binary for your active platform:
```bash
npm run tauri build
```
The compiled executable will be located under `src-tauri/target/release/`.

---

## 📦 Automated GitHub Releases (CI/CD)

This repository includes a GitHub Action workflow to automatically compile the application and create release binaries, gated strictly through Pull Request merges.

### How to release a new version:
1. Create a development branch and implement your changes.
2. Increment the version number in `package.json` and `src-tauri/tauri.conf.json` (e.g. `1.0.0` to `1.0.1`).
3. Commit your changes and open a **Pull Request** targeting the `main` branch.
4. Once the PR is reviewed and **merged into `main`**, the release workflow triggers automatically:
   - It reads the version directly from `package.json` (e.g. `v1.0.1`).
   - It boots a Windows runner to compile the release bundles.
   - It uploads the executables (such as `.exe` and `.msi` installers) to a new **Release Draft** on GitHub, ready for you to review and publish.

---

## 📄 License
ContextSwitch is open-source and released under the MIT License. Developed by **dukevibington.com**.
