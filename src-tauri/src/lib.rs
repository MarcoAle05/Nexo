mod agents;
mod agy;
mod browser;
mod claude;
mod claude_code;
mod connections;
mod fichas;
mod graph;
mod markitdown;
mod mcp;
mod nexo;
mod notas;
mod playwright;
mod registro;
pub mod render;
mod skills;
mod sources;
mod usage;
mod vault;
mod workspace;
mod workspace_run;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(nexo::ChatState::default())
        .manage(claude_code::ClaudeCodeState::default())
        .manage(agy::AgyState::default())
        .manage(usage::UsageState::default())
        .manage(registro::RegistroState::default())
        .manage(workspace::WorkspaceCatalogs::default())
        .manage(workspace_run::WorkspaceRuns::default())
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
            graph::note_view,
            nexo::key_status,
            nexo::set_api_key,
            nexo::chat_send,
            nexo::chat_reset,
            nexo::ask,
            nexo::compile,
            markitdown::convert_sources,
            connections::connections_status,
            agy::add_web_source,
            agy::summarize_source,
            agy::agy_jobs,
            agy::agy_clear_finished,
            sources::rename_source,
            sources::source_detail,
            claude_code::claude_code_start,
            claude_code::claude_code_send,
            claude_code::claude_code_stop,
            claude_code::claude_code_reset,
            browser::browser_status,
            browser::browser_import,
            usage::usage_claude,
            usage::usage_agy,
            notas::notes_tree,
            notas::note_read,
            notas::note_create,
            notas::note_save,
            notas::topic_create,
            notas::topic_rename,
            notas::notes_delete,
            notas::desk_load,
            notas::desk_save,
            notas::desks_load,
            notas::desks_save,
            render::render_info,
            skills::skills_list,
            skills::skills_prepare,
            skills::skills_remove,
            agents::agents_list,
            agents::agents_remove,
            registro::agent_log,
            registro::agent_context,
            workspace::workspace_list,
            workspace::workspace_open,
            workspace::workspace_create,
            workspace::workspace_forget,
            workspace::workspace_state,
            workspace::workspace_save_ui,
            workspace::workspace_set_extras,
            workspace::workspace_tree,
            workspace::workspace_read,
            workspace::workspace_agents,
            workspace::workspace_extras,
            workspace_run::workspace_start,
            workspace_run::workspace_send,
            workspace_run::workspace_stop,
            workspace_run::workspace_reset,
            workspace_run::workspace_detach,
            workspace_run::workspace_running
        ])
        .setup(|app| {
            render::attach(app);
            graph::watch(app.handle().clone());
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .on_window_event(|_, event| {
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                render::shutdown();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_, event| {
            if let tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit = event {
                render::shutdown();
            }
        });
}
