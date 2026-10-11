//! Commits automáticos del vault: deja en git lo que escribe la compilación para poder
//! revisarlo y deshacerlo. Las reglas están en `docs/spec-commits-automaticos.md`.
//!
//! Llama a `git` como programa (sin dependencias nuevas). Nunca crea repositorios, nunca
//! empuja y nunca devuelve un error que tumbe la compilación: todo termina en un [`Outcome`].

use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Lo que pasó al intentar guardar en git.
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Se creó un commit; lleva el hash corto.
    Committed(String),
    /// No había nada que guardar.
    NothingToCommit,
    /// No se intentó (sin repositorio o sin git): no es un fallo.
    Skipped(&'static str),
    /// Git falló; el texto es la primera línea de su error.
    Failed(String),
}

impl Outcome {
    /// Línea para la terminal de nexo; `None` si no hay nada que contar.
    pub fn message(&self) -> Option<String> {
        match self {
            Outcome::Committed(hash) => Some(format!("guardado en git {hash}")),
            Outcome::Failed(error) => Some(format!("git: {error}")),
            Outcome::NothingToCommit | Outcome::Skipped(_) => None,
        }
    }
}

pub struct VaultGit {
    root: PathBuf,
    program: String,
    timeout: Duration,
    env: Vec<(String, String)>,
}

struct Run {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

impl VaultGit {
    pub fn new(root: &Path) -> Self {
        VaultGit {
            root: root.to_path_buf(),
            program: "git".into(),
            timeout: Duration::from_secs(30),
            env: Vec::new(),
        }
    }

    /// Para las pruebas: otro programa, otro plazo o variables de entorno extra.
    #[cfg(test)]
    fn with(mut self, program: &str, timeout: Duration, env: &[(&str, &str)]) -> Self {
        self.program = program.into();
        self.timeout = timeout;
        self.env = env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        self
    }

    /// Foto previa a una compilación: guarda lo que haya sin guardar en `wiki/`
    /// (y solo ahí) con el mensaje `nexo: antes de compilar`.
    pub fn snapshot(&self) -> Outcome {
        if let Some(skip) = self.unavailable() {
            return skip;
        }
        if !self.root.join("wiki").is_dir() {
            return Outcome::NothingToCommit;
        }
        self.stage_and_commit(&["wiki".to_string()], "nexo: antes de compilar")
    }

    /// Guarda en un commit exactamente `paths` (relativas al vault) con `message`.
    /// Las rutas ignoradas por `.gitignore` o que no existen se omiten.
    pub fn commit(&self, paths: &[String], message: &str) -> Outcome {
        if let Some(skip) = self.unavailable() {
            return skip;
        }
        for path in paths {
            if !is_vault_relative(path) {
                return Outcome::Failed(format!("ruta no válida: {path}"));
            }
        }
        let mut keep = Vec::new();
        for path in paths {
            if !self.root.join(path).exists() || keep.contains(path) {
                continue;
            }
            match self.git(&["check-ignore", "-q", "--", path]) {
                Ok(run) if run.code == Some(0) => {} // ignorada
                Ok(run) if run.code == Some(1) => keep.push(path.clone()),
                Ok(run) => return Outcome::Failed(first_line(&run.stderr)),
                Err(error) => return Outcome::Failed(error),
            }
        }
        if keep.is_empty() {
            return Outcome::NothingToCommit;
        }
        self.stage_and_commit(&keep, &clean_message(message))
    }

    fn unavailable(&self) -> Option<Outcome> {
        // Solo cuenta un repositorio en la propia raíz del vault: si el vault está dentro de
        // otro repositorio, no se escribe en él.
        if !self.root.join(".git").exists() {
            return Some(Outcome::Skipped("el vault no es un repositorio git"));
        }
        match self.git(&["--version"]) {
            Ok(_) => None,
            Err(_) => Some(Outcome::Skipped("git no está disponible")),
        }
    }

    fn stage_and_commit(&self, specs: &[String], message: &str) -> Outcome {
        let with_specs = |head: &[&str]| -> Vec<String> {
            head.iter()
                .map(|s| s.to_string())
                .chain(std::iter::once("--".to_string()))
                .chain(specs.iter().cloned())
                .collect()
        };
        let run = |args: Vec<String>| {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            self.git(&args)
        };
        match run(with_specs(&["add", "-A"])) {
            Ok(r) if r.code == Some(0) => {}
            Ok(r) => return Outcome::Failed(first_line(&r.stderr)),
            Err(error) => return Outcome::Failed(error),
        }
        match run(with_specs(&["diff", "--cached", "--quiet"])) {
            Ok(r) if r.code == Some(0) => return Outcome::NothingToCommit,
            Ok(r) if r.code == Some(1) => {}
            Ok(r) => return Outcome::Failed(first_line(&r.stderr)),
            Err(error) => return Outcome::Failed(error),
        }
        let committed = run(with_specs(&["commit", "-q", "-m", message]));
        match committed {
            Ok(r) if r.code == Some(0) => {}
            other => {
                // Deja el índice como estaba: lo que se añadió para el commit se quita.
                let _ = run(with_specs(&["reset", "-q"]));
                return Outcome::Failed(match other {
                    Ok(r) => first_line(&format!("{}{}", r.stderr, r.stdout)),
                    Err(error) => error,
                });
            }
        }
        match self.git(&["rev-parse", "--short", "HEAD"]) {
            Ok(r) if r.code == Some(0) => Outcome::Committed(r.stdout.trim().to_string()),
            _ => Outcome::Committed("?".into()),
        }
    }

    /// Ejecuta git en el vault con un plazo. Si se pasa, no lo mata (podría dejar
    /// `.git/index.lock`): deja que termine solo y avisa.
    fn git(&self, args: &[&str]) -> Result<Run, String> {
        let mut command = Command::new(&self.program);
        command
            // Una firma GPG que pida contraseña dejaría la compilación esperando.
            .args(["-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(&self.root)
            .env("GIT_TERMINAL_PROMPT", "0")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in &self.env {
            command.env(key, value);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("no se pudo ejecutar git: {e}"))?;
        let read = |pipe: Option<Box<dyn Read + Send>>| {
            std::thread::spawn(move || {
                let mut text = String::new();
                if let Some(mut pipe) = pipe {
                    let _ = pipe.read_to_string(&mut text);
                }
                text
            })
        };
        let out = read(child.stdout.take().map(|p| Box::new(p) as _));
        let err = read(child.stderr.take().map(|p| Box::new(p) as _));
        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return Ok(Run {
                        code: status.code(),
                        stdout: out.join().unwrap_or_default(),
                        stderr: err.join().unwrap_or_default(),
                    });
                }
                Ok(None) if start.elapsed() < self.timeout => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Ok(None) => {
                    std::thread::spawn(move || {
                        let _ = child.wait();
                    });
                    return Err(format!(
                        "git no respondió en {} s; se deja terminar",
                        self.timeout.as_secs().max(1)
                    ));
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    }
}

/// Mensaje del commit de una fuente compilada.
pub fn source_message(source: &str) -> String {
    format!("nexo: compila raw/{source}")
}

/// Ruta relativa al vault, sin `..`, sin raíz y que no entre en `.git`.
fn is_vault_relative(path: &str) -> bool {
    let p = Path::new(path);
    !path.is_empty()
        && p.components().all(|c| matches!(c, Component::Normal(_)))
        && p.components().next() != Some(Component::Normal(".git".as_ref()))
}

/// Una sola línea y sin caracteres de control (el nombre de la fuente lo elige el usuario).
fn clean_message(message: &str) -> String {
    message
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .trim()
        .to_string()
}

fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("error desconocido")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Entorno que aísla las pruebas de la configuración de git de quien las ejecuta.
    const HERMETIC: [(&str, &str); 2] = [
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
    ];

    fn sh(dir: &Path, args: &[&str]) -> String {
        let out = Command::new("git")
            .args(args)
            .current_dir(dir)
            .envs(HERMETIC)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim_end().to_string()
    }

    /// Vault de prueba con un repositorio, una identidad y un commit base.
    fn vault(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("nexo-vaultgit-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("wiki")).unwrap();
        fs::create_dir_all(root.join("output")).unwrap();
        fs::create_dir_all(root.join(".nexo")).unwrap();
        sh(&root, &["init", "-q", "-b", "main"]);
        sh(&root, &["config", "user.name", "Prueba"]);
        sh(&root, &["config", "user.email", "prueba@example.com"]);
        fs::write(root.join("wiki/base.md"), "base\n").unwrap();
        fs::write(root.join("output/q.md"), "q\n").unwrap();
        sh(&root, &["add", "-A"]);
        sh(&root, &["commit", "-qm", "base"]);
        root
    }

    fn git(root: &Path) -> VaultGit {
        VaultGit::new(root).with("git", Duration::from_secs(20), &HERMETIC)
    }

    fn commits(root: &Path) -> usize {
        sh(root, &["rev-list", "--count", "HEAD"]).parse().unwrap()
    }

    fn files_of_last_commit(root: &Path) -> Vec<String> {
        let mut v: Vec<String> = sh(root, &["show", "--name-only", "--format=", "HEAD"])
            .lines()
            .map(String::from)
            .collect();
        v.sort();
        v
    }

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn sin_repositorio_no_hace_nada_ni_lo_crea() {
        let root = std::env::temp_dir().join(format!("nexo-vaultgit-sin-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("wiki")).unwrap();
        fs::write(root.join("wiki/a.md"), "a").unwrap();
        let g = git(&root);
        assert!(matches!(g.snapshot(), Outcome::Skipped(_)));
        assert!(matches!(
            g.commit(&paths(&["wiki/a.md"]), "x"),
            Outcome::Skipped(_)
        ));
        assert!(!root.join(".git").exists());
    }

    #[test]
    fn un_vault_dentro_de_otro_repositorio_se_deja_en_paz() {
        let padre = vault("padre");
        let hijo = padre.join("subvault");
        fs::create_dir_all(hijo.join("wiki")).unwrap();
        fs::write(hijo.join("wiki/a.md"), "a").unwrap();
        let antes = commits(&padre);
        let g = git(&hijo);
        assert!(matches!(g.snapshot(), Outcome::Skipped(_)));
        assert!(matches!(
            g.commit(&paths(&["wiki/a.md"]), "x"),
            Outcome::Skipped(_)
        ));
        assert_eq!(commits(&padre), antes);
    }

    #[test]
    fn sin_git_instalado_no_es_un_fallo() {
        let root = vault("sin-git");
        let g = VaultGit::new(&root).with("git-que-no-existe", Duration::from_secs(5), &[]);
        assert!(matches!(g.snapshot(), Outcome::Skipped(_)));
        assert!(matches!(
            g.commit(&paths(&["wiki/base.md"]), "x"),
            Outcome::Skipped(_)
        ));
    }

    #[test]
    fn el_commit_lleva_solo_las_rutas_indicadas() {
        let root = vault("solo-rutas");
        fs::write(root.join("wiki/nueva.md"), "nueva\n").unwrap();
        fs::write(root.join("wiki/ajena.md"), "a medias\n").unwrap();
        fs::write(root.join("wiki/base.md"), "base editada\n").unwrap();
        fs::write(root.join(".nexo/compiled.json"), "{}").unwrap();
        let antes = commits(&root);
        let r = git(&root).commit(
            &paths(&["wiki/nueva.md", ".nexo/compiled.json"]),
            &source_message("a.pdf"),
        );
        assert!(
            matches!(r, Outcome::Committed(ref h) if !h.is_empty() && h != "?"),
            "{r:?}"
        );
        assert_eq!(commits(&root), antes + 1);
        assert_eq!(
            files_of_last_commit(&root),
            [".nexo/compiled.json", "wiki/nueva.md"]
        );
        assert_eq!(
            sh(&root, &["log", "-1", "--format=%s"]),
            "nexo: compila raw/a.pdf"
        );
        // Lo demás sigue como estaba: sin guardar.
        let estado = sh(&root, &["status", "--porcelain"]);
        assert!(estado.contains("?? wiki/ajena.md"), "{estado}");
        assert!(estado.contains(" M wiki/base.md"), "{estado}");
    }

    #[test]
    fn no_toca_lo_que_el_usuario_ya_tenia_en_el_stage() {
        let root = vault("stage-ajeno");
        fs::write(root.join("wiki/mia.md"), "mía\n").unwrap();
        sh(&root, &["add", "wiki/mia.md"]);
        fs::write(root.join("wiki/nueva.md"), "nueva\n").unwrap();
        let r = git(&root).commit(&paths(&["wiki/nueva.md"]), "nexo: compila raw/x");
        assert!(matches!(r, Outcome::Committed(_)), "{r:?}");
        assert_eq!(files_of_last_commit(&root), ["wiki/nueva.md"]);
        assert!(sh(&root, &["status", "--porcelain"]).contains("A  wiki/mia.md"));
    }

    #[test]
    fn sin_cambios_no_crea_commits_vacios() {
        let root = vault("vacio");
        let antes = commits(&root);
        let g = git(&root);
        assert_eq!(
            g.commit(&paths(&["wiki/base.md"]), "x"),
            Outcome::NothingToCommit
        );
        assert_eq!(g.snapshot(), Outcome::NothingToCommit);
        assert_eq!(commits(&root), antes);
    }

    #[test]
    fn la_foto_previa_guarda_solo_wiki() {
        let root = vault("foto");
        fs::write(root.join("wiki/base.md"), "editada a mano\n").unwrap();
        fs::write(root.join("wiki/nueva.md"), "nueva\n").unwrap();
        fs::write(root.join("output/q.md"), "cambio fuera de wiki\n").unwrap();
        let r = git(&root).snapshot();
        assert!(matches!(r, Outcome::Committed(_)), "{r:?}");
        assert_eq!(
            files_of_last_commit(&root),
            ["wiki/base.md", "wiki/nueva.md"]
        );
        assert_eq!(
            sh(&root, &["log", "-1", "--format=%s"]),
            "nexo: antes de compilar"
        );
        assert!(sh(&root, &["status", "--porcelain"]).contains(" M output/q.md"));
    }

    #[test]
    fn la_foto_previa_recoge_tambien_lo_borrado() {
        let root = vault("foto-borrado");
        fs::remove_file(root.join("wiki/base.md")).unwrap();
        let r = git(&root).snapshot();
        assert!(matches!(r, Outcome::Committed(_)), "{r:?}");
        assert_eq!(files_of_last_commit(&root), ["wiki/base.md"]);
    }

    #[test]
    fn rechaza_rutas_que_salen_del_vault_o_entran_en_git() {
        let root = vault("rutas");
        fs::write(root.join("wiki/nueva.md"), "n").unwrap();
        let antes = commits(&root);
        for mala in [
            "../fuera.md",
            "/etc/passwd",
            ".git/config",
            "wiki/../../x.md",
            "",
        ] {
            let r = git(&root).commit(&paths(&["wiki/nueva.md", mala]), "x");
            assert!(matches!(r, Outcome::Failed(_)), "{mala:?} → {r:?}");
        }
        assert_eq!(commits(&root), antes);
        assert!(sh(&root, &["status", "--porcelain"]).contains("?? wiki/nueva.md"));
    }

    #[test]
    fn omite_las_rutas_ignoradas_y_las_que_no_existen() {
        let root = vault("ignoradas");
        fs::write(root.join(".gitignore"), "secreto.md\n").unwrap();
        sh(&root, &["add", ".gitignore"]);
        sh(&root, &["commit", "-qm", "gitignore"]);
        fs::write(root.join("wiki/secreto.md"), "s").unwrap();
        fs::write(root.join("wiki/nueva.md"), "n").unwrap();
        let r = git(&root).commit(
            &paths(&[
                "wiki/secreto.md",
                "wiki/no-existe.md",
                "wiki/nueva.md",
                "wiki/nueva.md",
            ]),
            "nexo: compila raw/x",
        );
        assert!(matches!(r, Outcome::Committed(_)), "{r:?}");
        assert_eq!(files_of_last_commit(&root), ["wiki/nueva.md"]);
    }

    #[test]
    fn sin_identidad_falla_sin_romper_y_deja_el_indice_limpio() {
        let root = vault("identidad");
        sh(&root, &["config", "--unset", "user.name"]);
        sh(&root, &["config", "--unset", "user.email"]);
        sh(&root, &["config", "user.useConfigOnly", "true"]);
        fs::write(root.join("wiki/nueva.md"), "n").unwrap();
        let antes = commits(&root);
        let r = git(&root).commit(&paths(&["wiki/nueva.md"]), "x");
        assert!(
            matches!(r, Outcome::Failed(ref e) if !e.is_empty()),
            "{r:?}"
        );
        assert!(r.message().unwrap().starts_with("git: "));
        assert_eq!(commits(&root), antes);
        assert_eq!(sh(&root, &["diff", "--cached", "--name-only"]), "");
    }

    #[test]
    fn una_firma_gpg_configurada_no_bloquea() {
        let root = vault("gpg");
        sh(&root, &["config", "commit.gpgsign", "true"]);
        sh(&root, &["config", "gpg.program", "/bin/false"]);
        fs::write(root.join("wiki/nueva.md"), "n").unwrap();
        let r = git(&root).commit(&paths(&["wiki/nueva.md"]), "x");
        assert!(matches!(r, Outcome::Committed(_)), "{r:?}");
    }

    #[test]
    fn un_remoto_roto_no_afecta_porque_nunca_se_empuja() {
        let root = vault("remoto");
        sh(
            &root,
            &["remote", "add", "origin", "/ruta/que/no/existe.git"],
        );
        fs::write(root.join("wiki/nueva.md"), "n").unwrap();
        let r = git(&root).commit(&paths(&["wiki/nueva.md"]), "x");
        assert!(matches!(r, Outcome::Committed(_)), "{r:?}");
    }

    #[cfg(unix)]
    #[test]
    fn si_git_tarda_demasiado_avisa_y_sigue() {
        use std::os::unix::fs::PermissionsExt;
        let root = vault("plazo");
        let hook = root.join(".git/hooks/pre-commit");
        fs::write(&hook, "#!/bin/sh\nsleep 2\n").unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(root.join("wiki/nueva.md"), "n").unwrap();
        let g = VaultGit::new(&root).with("git", Duration::from_millis(400), &HERMETIC);
        let t = Instant::now();
        let r = g.commit(&paths(&["wiki/nueva.md"]), "x");
        assert!(
            t.elapsed() < Duration::from_millis(1800),
            "esperó demasiado"
        );
        assert!(
            matches!(r, Outcome::Failed(ref e) if e.contains("no respondió")),
            "{r:?}"
        );
    }

    #[test]
    fn el_mensaje_va_en_una_sola_linea() {
        assert_eq!(source_message("a/b.pdf"), "nexo: compila raw/a/b.pdf");
        assert_eq!(
            clean_message("nexo: compila raw/a\nb\u{7}.md"),
            "nexo: compila raw/a b .md"
        );
    }

    #[test]
    fn mensajes_para_la_terminal() {
        assert_eq!(
            Outcome::Committed("abc1234".into()).message().unwrap(),
            "guardado en git abc1234"
        );
        assert!(Outcome::NothingToCommit.message().is_none());
        assert!(Outcome::Skipped("x").message().is_none());
    }
}
