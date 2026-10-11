//! La parte de nexo que no depende de Tauri: lo que se puede compilar, probar y usar desde
//! otro binario (un servidor MCP, por ejemplo) sin WebKitGTK. La app de escritorio
//! (`src-tauri`, crate `app`) depende de este crate; aquí nunca entra `tauri`.

pub mod fichas;
pub mod graph;
pub mod vault;
pub mod vaultgit;
pub mod wiki;
