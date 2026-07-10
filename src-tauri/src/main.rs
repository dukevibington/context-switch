#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    context_switch_daemon::run();
}
