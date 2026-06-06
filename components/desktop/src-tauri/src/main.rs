#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bridge;
mod explorer;

use bridge::{
    bridge_poll, bridge_restart, bridge_send, bridge_start, bridge_status, bridge_stop, BridgeState,
};
use explorer::fs_list_directory;

fn main() {
    tauri::Builder::default()
        .manage(BridgeState::default())
        .invoke_handler(tauri::generate_handler![
            bridge_status,
            bridge_start,
            bridge_send,
            bridge_poll,
            bridge_stop,
            bridge_restart,
            fs_list_directory,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Vegvisir Desktop");
}
