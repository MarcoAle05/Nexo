mod agents;
mod agy;
mod avisos;
mod browser;
mod claude;
mod claude_code;
mod connections;
pub mod control;
mod fichas;
mod graph;
mod markitdown;
mod mcp;
mod nexo;
mod notas;
mod orquestacion;
mod playwright;
mod registro;
pub mod render;
mod sesiones;
mod skills;
mod sources;
mod terminales;
mod usage;
mod vault;
mod workspace;
mod worktree;

use std::sync::Arc;

use tauri::Manager;

/// Prepara las terminales de los workspaces: el servidor de control, los archivos de arranque
/// del shell con el `claude` de nexo y el guardado periódico. Si algo falla, nexo abre igual y
/// las terminales dicen por qué no abren.
fn prepare_terminals(app: &tauri::AppHandle) -> Result<(), String> {
    let run = terminales::run_dir();
    let ctl = control::listen(app.clone(), &run).unwrap_or_else(|e| {
        log::warn!("No se pudo abrir el socket de control: {e}");
        control::socket_paths(&run).0
    });
    let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let files = terminales::install_shell(&data, &exe, &run, &ctl)?;
    let terms = terminales::AppTerms {
        store: data.join("terminales"),
        files,
        shell: terminales::user_shell(),
        oyente: Arc::new(terminales::AppOyente(app.clone())),
    };
    *app.state::<terminales::TermSetup>()
        .0
        .lock()
        .map_err(|e| e.to_string())? = Some(Arc::new(terms));
    terminales::keep_saving(app.clone());
    Ok(())
}

/// Al cerrar nexo: guarda las terminales (para recuperarlas), las cuelga y quita el socket.
fn close_terminals(app: &tauri::AppHandle) {
    let Ok(terms) = terminales::app_terms(app) else {
        return;
    };
    app.state::<terminales::Terminales>().shutdown(&terms.store);
    control::unlisten(&terms.files.run);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .manage(nexo::ChatState::default())
        .manage(claude_code::ClaudeCodeState::default())
        .manage(agy::AgyState::default())
        .manage(usage::UsageState::default())
        .manage(registro::RegistroState::default())
        .manage(terminales::Terminales::default())
        .manage(terminales::TermSetup::default())
        .manage(sesiones::Sesiones::default())
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
            workspace::workspace_close,
            workspace::workspace_state,
            workspace::workspace_save_ui,
            workspace::workspace_tree,
            workspace::workspace_read,
            sesiones::workspace_attention,
            sesiones::workspace_watch,
            terminales::terminal_list,
            terminales::terminal_open,
            terminales::terminal_attach,
            terminales::terminal_input,
            terminales::terminal_resize,
            terminales::terminal_busy,
            terminales::terminal_busy_in,
            terminales::terminal_close,
            worktree::worktree_list,
            worktree::worktree_form,
            worktree::worktree_plan,
            worktree::worktree_create,
            worktree::worktree_setup
        ])
        .setup(|app| {
            render::attach(app);
            graph::watch(app.handle().clone());
            if let Err(e) = prepare_terminals(app.handle()) {
                log::warn!("No se pudieron preparar las terminales: {e}");
            }
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
        .run(|app, event| {
            if let tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit = event {
                close_terminals(app);
                render::shutdown();
            }
        });
}
