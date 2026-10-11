//! Guardián de rutas del vault: lo que llega de fuera no puede salir de su carpeta.

use std::path::{Path, PathBuf};

/// Resuelve `rel` dentro de `base` y rechaza cualquier ruta que se salga de ella.
pub fn resolve_inside(base: &Path, rel: &str) -> Result<PathBuf, String> {
    let base = base.canonicalize().map_err(|e| e.to_string())?;
    let path = base
        .join(rel)
        .canonicalize()
        .map_err(|_| format!("No existe: {rel}"))?;
    if path.starts_with(&base) && path != base {
        Ok(path)
    } else {
        Err("Ruta fuera del vault.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// `vault/raw/` con `nota.md` y `sub/`, la carpeta vecina `vault/raw-otro/` y, fuera
    /// del vault, `ajeno/secreto.md`.
    fn temp(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("nexo-vault-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("vault/raw/sub")).unwrap();
        fs::create_dir_all(root.join("vault/raw-otro")).unwrap();
        fs::create_dir_all(root.join("ajeno")).unwrap();
        fs::write(root.join("vault/raw/nota.md"), "dentro").unwrap();
        fs::write(root.join("vault/raw-otro/nota.md"), "vecina").unwrap();
        fs::write(root.join("ajeno/secreto.md"), "ajeno").unwrap();
        root
    }

    const OUTSIDE: &str = "Ruta fuera del vault.";

    #[test]
    fn resolve_inside_acepta_lo_que_hay_dentro() {
        let root = temp("dentro");
        let base = root.join("vault/raw");
        let real = base.canonicalize().unwrap();

        assert_eq!(resolve_inside(&base, "nota.md"), Ok(real.join("nota.md")));
        assert_eq!(resolve_inside(&base, "sub"), Ok(real.join("sub")));
        // Un rodeo que vuelve a entrar sigue dentro.
        assert_eq!(
            resolve_inside(&base, "sub/../nota.md"),
            Ok(real.join("nota.md"))
        );
    }

    #[test]
    fn resolve_inside_rechaza_lo_que_sale() {
        let root = temp("fuera");
        let base = root.join("vault/raw");

        for rel in [
            "..",
            "../../ajeno/secreto.md",
            "sub/../../../ajeno/secreto.md",
            // Empieza igual que la base, pero es otra carpeta.
            "../raw-otro/nota.md",
        ] {
            assert_eq!(resolve_inside(&base, rel), Err(OUTSIDE.into()), "{rel}");
        }
        // Una ruta absoluta sustituye a la base al unirlas.
        let absolute = root.join("ajeno/secreto.md");
        assert_eq!(
            resolve_inside(&base, absolute.to_str().unwrap()),
            Err(OUTSIDE.into())
        );
    }

    #[test]
    fn resolve_inside_rechaza_la_propia_base() {
        let root = temp("base");
        let base = root.join("vault/raw");

        for rel in ["", ".", "sub/.."] {
            assert_eq!(resolve_inside(&base, rel), Err(OUTSIDE.into()), "{rel:?}");
        }
    }

    #[test]
    fn resolve_inside_rechaza_lo_que_no_existe() {
        let root = temp("falta");
        let base = root.join("vault/raw");

        assert_eq!(
            resolve_inside(&base, "falta.md"),
            Err("No existe: falta.md".into())
        );
        assert!(resolve_inside(&root.join("vault/no-hay"), "nota.md").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_inside_sigue_los_enlaces_simbolicos() {
        use std::os::unix::fs::symlink;
        let root = temp("enlaces");
        let base = root.join("vault/raw");
        let real = base.canonicalize().unwrap();
        symlink(root.join("ajeno"), base.join("carpeta")).unwrap();
        symlink(root.join("ajeno/secreto.md"), base.join("archivo.md")).unwrap();
        symlink(root.join("ajeno/falta.md"), base.join("roto.md")).unwrap();
        symlink(base.join("sub"), base.join("alias")).unwrap();

        for rel in ["carpeta", "carpeta/secreto.md", "archivo.md"] {
            assert_eq!(resolve_inside(&base, rel), Err(OUTSIDE.into()), "{rel}");
        }
        assert_eq!(
            resolve_inside(&base, "roto.md"),
            Err("No existe: roto.md".into())
        );
        // El que apunta dentro vale, y devuelve la ruta real.
        assert_eq!(resolve_inside(&base, "alias"), Ok(real.join("sub")));
    }
}
