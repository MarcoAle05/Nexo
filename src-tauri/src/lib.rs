mod claude;
mod claude_code;
mod connections;
mod graph;
mod markitdown;
mod nexo;
mod sources;
mod vault;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(nexo::ChatState::default())
        .manage(claude_code::ClaudeCodeState::default())
        .invoke_handler(tauri::generate_handler![
            vault::get_vault,
            vault::detect_vaults,
            vault::set_vault,
            sources::list_sources,
            sources::add_sources,
            sources::open_source,
            sources::remove_source,
            graph::read_graph,
            graph::open_note,
            nexo::key_status,
            nexo::set_api_key,
            nexo::chat_send,
            nexo::chat_reset,
            nexo::ask,
            nexo::compile,
            markitdown::convert_sources,
            connections::connections_status,
            claude_code::claude_code_start,
            claude_code::claude_code_write,
            claude_code::claude_code_resize,
            claude_code::claude_code_stop
        ])
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while building tauri application");
}
