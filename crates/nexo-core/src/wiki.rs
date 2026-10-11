//! Guardián de las rutas que escribe el modelo: las herramientas de `compile` solo pueden
//! leer y escribir dentro de `wiki/`.

use std::path::{Component, Path, PathBuf};

/// Ruta segura dentro de `wiki/` para las herramientas: relativa, sin `..` ni carpetas ocultas.
pub fn wiki_path(wiki: &Path, path: &str, must_be_md: bool) -> Result<PathBuf, String> {
    let clean = path.trim().trim_start_matches("wiki/");
    let p = Path::new(clean);
    let ok = !clean.is_empty()
        && p.components()
            .all(|c| matches!(c, Component::Normal(s) if !s.to_string_lossy().starts_with('.')));
    if !ok {
        return Err(format!("Ruta no válida: {path}"));
    }
    if must_be_md && !clean.to_lowercase().ends_with(".md") {
        return Err("Solo se pueden escribir notas .md".into());
    }
    let file = wiki.join(p);
    if !stays_inside(wiki, &file) {
        return Err(format!("Ruta fuera de la wiki: {path}"));
    }
    Ok(file)
}

/// `false` si un enlace simbólico saca `file` de `wiki`. Se resuelve la parte de la ruta que
/// ya existe: lo que falta lo crea `write_note` y no puede ser un enlace.
fn stays_inside(wiki: &Path, file: &Path) -> bool {
    let Ok(base) = wiki.canonicalize() else {
        // Sin wiki todavía no hay enlaces que seguir.
        return true;
    };
    let mut existing = file;
    // `symlink_metadata` y no `exists`: un enlace roto también cuenta, porque escribir en
    // él crea su destino.
    while existing.symlink_metadata().is_err() {
        match existing.parent() {
            Some(parent) => existing = parent,
            None => return false,
        }
    }
    existing
        .canonicalize()
        .is_ok_and(|real| real.starts_with(&base))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn rutas_de_herramientas_seguras() {
        let wiki = Path::new("/v/wiki");
        assert!(wiki_path(wiki, "rag/_index.md", true).is_ok());
        assert!(wiki_path(wiki, "wiki/rag/nota.md", true).is_ok());
        assert!(wiki_path(wiki, "../raw/x.md", true).is_err());
        assert!(wiki_path(wiki, "/etc/passwd.md", true).is_err());
        assert!(wiki_path(wiki, ".obsidian/app.md", true).is_err());
        assert!(wiki_path(wiki, "rag/nota.txt", true).is_err());
    }

    /// `vault/wiki/` vacía y, fuera del vault, `ajeno/secreto.md`.
    #[cfg(unix)]
    fn temp(name: &str) -> (PathBuf, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("nexo-herramientas-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let wiki = root.join("vault/wiki");
        let outside = root.join("ajeno");
        fs::create_dir_all(&wiki).unwrap();
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secreto.md"), "ajeno").unwrap();
        (wiki, outside)
    }

    #[cfg(unix)]
    #[test]
    fn wiki_path_rechaza_los_enlaces_que_salen_del_vault() {
        use std::os::unix::fs::symlink;
        let (wiki, outside) = temp("ruta");
        symlink(&outside, wiki.join("carpeta")).unwrap();
        symlink(outside.join("secreto.md"), wiki.join("archivo.md")).unwrap();
        symlink(outside.join("nueva.md"), wiki.join("roto.md")).unwrap();

        for path in [
            "carpeta/nota.md",
            "carpeta/sub/nota.md",
            "archivo.md",
            "roto.md",
        ] {
            assert!(wiki_path(&wiki, path, true).is_err(), "aceptó {path}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn wiki_path_acepta_los_enlaces_que_se_quedan_dentro() {
        use std::os::unix::fs::symlink;
        let (wiki, _) = temp("dentro");
        fs::create_dir_all(wiki.join("rag")).unwrap();
        symlink(wiki.join("rag"), wiki.join("alias")).unwrap();

        assert!(wiki_path(&wiki, "alias/nota.md", true).is_ok());
        assert!(wiki_path(&wiki, "tema/nuevo/nota.md", true).is_ok());
    }
}
