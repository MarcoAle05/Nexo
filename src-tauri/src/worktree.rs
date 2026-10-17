//! Worktrees de git desde un workspace: el «+» junto al nombre crea otra copia de trabajo del
//! mismo repo, con su propia rama, en `~/nexo/workspaces/<repo>/<nombre>`, y la abre como un
//! workspace más (con su orquestador). El nombre del workspace abre la lista de los worktrees
//! del repo para saltar entre ellos.
//!
//! Es la lógica de Orca (1.4.216, leída de su paquete): el nombre se limpia y, si no se
//! escribe, se propone uno de una lista que no se repite (Orca usa animales marinos; nexo,
//! estrellas y constelaciones); la rama es `<usuario de GitHub>/<nombre>`; parte de la rama
//! principal del remoto (`origin/HEAD`, o `origin/main`, `origin/master`, `main`, `master`)
//! después de un `git fetch`; si la rama o la carpeta ya existen se prueba `nombre-2`,
//! `nombre-3`… Se crea con `git worktree add --no-track -b`, se apunta la base en
//! `branch.<rama>.base` y se pone `push.autoSetupRemote`. Después se copian los archivos
//! ignorados que liste `.worktreeinclude` (un `.env`, por ejemplo), se enlazan las carpetas
//! ignoradas de `worktree.sharedDirectories` de `orca.yaml` y se ejecuta su `scripts.setup` en
//! segundo plano, con `ORCA_ROOT_PATH` y `ORCA_WORKTREE_PATH` (la salida va a
//! `.nexo/setup.log`). El workspace nuevo arranca con los mismos extras encendidos que el de
//! origen.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager};
use yaml_rust2::{Yaml, YamlLoader};

use crate::workspace::{self, NEXO_DIR, WsInfo};

/// Los worktrees van en `~/nexo/workspaces/<repo>/<nombre>` (Orca: `~/orca/workspaces`).
const WORKTREES_DIR: &str = "nexo/workspaces";
/// Los nombres propuestos que ya se usaron, por repo (en la configuración de nexo).
const NAMES_FILE: &str = "worktree-nombres.json";
/// Intentos como mucho para encontrar un nombre libre (`nombre`, `nombre-2`…), como Orca.
const MAX_TRIES: usize = 100;
const NAME_MAX: usize = 60;
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
const BASES_MAX: usize = 300;
const INCLUDE_FILE: &str = ".worktreeinclude";
const INCLUDE_MAX: usize = 1000;
const SHARED_MAX: usize = 100;
/// Lo que se copia de `.worktreeinclude` como mucho, en total (el tope de Orca).
const COPY_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const COPY_MAX_ENTRIES: u64 = 50_000;
/// `orca.yaml` y `.worktreeinclude` más grandes que esto no se leen (como Orca).
const TEXT_MAX: u64 = 256 * 1024;
/// Lo que se enseña de la salida del setup: el final.
const LOG_TAIL: u64 = 256 * 1024;
const SETUP_RUNNER: &str = "setup-runner.sh";
const SETUP_LOG: &str = "setup.log";
const SETUP_STATE: &str = "setup.json";

/// Nombres que se proponen: estrellas y constelaciones, a juego con la galaxia.
const STARS: &[&str] = &[
    "orion",
    "vega",
    "lyra",
    "sirius",
    "altair",
    "deneb",
    "rigel",
    "antares",
    "polaris",
    "capella",
    "arcturus",
    "aldebaran",
    "betelgeuse",
    "spica",
    "procyon",
    "castor",
    "pollux",
    "regulus",
    "canopus",
    "achernar",
    "mira",
    "electra",
    "maia",
    "alcyone",
    "mizar",
    "alcor",
    "bellatrix",
    "mintaka",
    "alnilam",
    "saiph",
    "fomalhaut",
    "andromeda",
    "cassiopeia",
    "perseus",
    "draco",
    "cygnus",
    "aquila",
    "pegasus",
    "hydra",
    "phoenix",
    "carina",
    "vela",
    "auriga",
    "bootes",
    "corvus",
    "lynx",
    "fornax",
    "pyxis",
    "norma",
    "pavo",
    "tucana",
    "musca",
    "indus",
    "lepus",
    "columba",
    "crux",
    "centaurus",
    "lacerta",
    "vulpecula",
    "sagitta",
];

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------- procesos

struct Out {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// Ejecuta un programa y recoge lo que escribe. Con plazo, si se pasa lo mata (solo para lo
/// que no deja nada a medias: `git fetch`, la consulta a `gh`, el `PATH` del shell).
fn run(
    mut command: Command,
    input: Option<Vec<u8>>,
    timeout: Option<Duration>,
) -> Result<Out, String> {
    let program = command.get_program().to_string_lossy().into_owned();
    command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("No se pudo ejecutar {program}: {e}"))?;
    if let (Some(data), Some(mut stdin)) = (input, child.stdin.take()) {
        std::thread::spawn(move || {
            let _ = stdin.write_all(&data);
        });
    }
    let read = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            String::from_utf8_lossy(&bytes).into_owned()
        })
    };
    let out = read(child.stdout.take().map(|p| Box::new(p) as _));
    let err = read(child.stderr.take().map(|p| Box::new(p) as _));
    let status = match timeout {
        None => child.wait().map_err(|e| e.to_string())?,
        Some(limit) => {
            let start = Instant::now();
            loop {
                if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                    break status;
                }
                if start.elapsed() > limit {
                    let _ = child.kill();
                    let _ = child.wait();
                    // Los lectores pueden seguir esperando a un nieto que tiene la tubería: no
                    // se esperan.
                    return Err(format!("{program} tardó demasiado y se paró."));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    };
    Ok(Out {
        code: status.code(),
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
}

fn git_command(dir: &Path) -> Command {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(dir)
        // Una petición de usuario y contraseña dejaría la creación esperando.
        .env("GIT_TERMINAL_PROMPT", "0");
    command
}

fn git_error(out: &Out) -> String {
    let text = out.stderr.trim();
    if text.is_empty() {
        format!("git terminó con el código {}", out.code.unwrap_or(-1))
    } else {
        text.chars().take(600).collect()
    }
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = git_command(dir);
    command.args(args);
    let out = run(command, None, None)?;
    if out.code == Some(0) {
        Ok(out.stdout)
    } else {
        Err(git_error(&out))
    }
}

fn git_ok(dir: &Path, args: &[&str]) -> bool {
    git(dir, args).is_ok()
}

fn ref_exists(dir: &Path, name: &str) -> bool {
    git_ok(
        dir,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{name}^{{commit}}"),
        ],
    )
}

// ---------------------------------------------------------------- el repo y sus worktrees

#[derive(Clone, Debug, PartialEq)]
struct Tree {
    path: PathBuf,
    head: String,
    /// La rama, sin `refs/heads/` (ninguna si está suelto).
    branch: Option<String>,
    bare: bool,
    /// Git lo conoce pero su carpeta ya no está.
    prunable: bool,
}

/// `git worktree list --porcelain -z`: campos acabados en NUL y un campo vacío entre
/// worktrees. El primero es siempre el principal.
fn parse_worktrees(text: &str) -> Vec<Tree> {
    let mut trees = Vec::new();
    let mut current: Option<Tree> = None;
    for field in text.split('\0') {
        if field.is_empty() {
            trees.extend(current.take());
            continue;
        }
        let (key, value) = field.split_once(' ').unwrap_or((field, ""));
        match key {
            "worktree" => {
                trees.extend(current.take());
                current = Some(Tree {
                    path: PathBuf::from(value),
                    head: String::new(),
                    branch: None,
                    bare: false,
                    prunable: false,
                });
            }
            "HEAD" => {
                if let Some(t) = current.as_mut() {
                    t.head = value.to_string();
                }
            }
            "branch" => {
                if let Some(t) = current.as_mut() {
                    t.branch = Some(value.trim_start_matches("refs/heads/").to_string());
                }
            }
            "bare" => {
                if let Some(t) = current.as_mut() {
                    t.bare = true;
                }
            }
            "prunable" => {
                if let Some(t) = current.as_mut() {
                    t.prunable = true;
                }
            }
            _ => {}
        }
    }
    trees.extend(current);
    trees
}

#[derive(Clone, Debug)]
struct Repo {
    /// El checkout principal (o el repo desnudo).
    main: PathBuf,
    /// El nombre del repo: el de su carpeta principal, sin `.git`.
    name: String,
    /// `git rev-parse --git-common-dir`: lo comparten todos sus worktrees.
    common: PathBuf,
    trees: Vec<Tree>,
}

fn same_path(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(x), Ok(y)) if x == y)
}

/// El repo de un workspace, si la carpeta es la raíz de un checkout de git. Una carpeta que
/// solo está dentro de un repo (una subcarpeta, o todo `~` en un repo de dotfiles) no cuenta.
fn repo_of(root: &Path) -> Option<Repo> {
    let top = git(root, &["rev-parse", "--show-toplevel"]).ok()?;
    if !same_path(Path::new(top.trim()), root) {
        return None;
    }
    let common = git(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .ok()?;
    let common = PathBuf::from(common.trim());
    let trees = parse_worktrees(&git(root, &["worktree", "list", "--porcelain", "-z"]).ok()?);
    let main = trees.first()?.path.clone();
    let name = main
        .file_name()
        .map(|n| n.to_string_lossy().trim_end_matches(".git").to_string())
        .filter(|n| !n.is_empty() && n != "." && n != "..")
        .unwrap_or_else(|| "repo".into());
    Some(Repo {
        main,
        name,
        common: common.canonicalize().unwrap_or(common),
        trees,
    })
}

fn require_repo(root: &Path) -> Result<Repo, String> {
    repo_of(root).ok_or_else(|| "Este workspace no es la raíz de un repositorio de git.".into())
}

fn dest_parent(app: &AppHandle, repo: &Repo) -> Result<PathBuf, String> {
    let home = app.path().home_dir().map_err(|e| e.to_string())?;
    Ok(home.join(WORKTREES_DIR).join(&repo.name))
}

fn home(app: &AppHandle) -> PathBuf {
    app.path().home_dir().unwrap_or_else(|_| PathBuf::from("/"))
}

// ---------------------------------------------------------------- nombres

/// El nombre de la carpeta (y del final de la rama), como lo limpia Orca: lo que no sea letra,
/// número, `.`, `_` o `-` pasa a `-`, sin guiones repetidos, sin `..` y sin `.`/`-` en los
/// extremos.
fn sanitize_name(raw: &str) -> Result<String, String> {
    let mut out = String::new();
    for c in raw.trim().chars() {
        let keep = c.is_alphanumeric() || matches!(c, '.' | '_' | '-');
        let c = if keep { c } else { '-' };
        if (c == '-' && out.ends_with('-')) || (c == '.' && out.ends_with('.')) {
            continue;
        }
        out.push(c);
    }
    let out: String = out
        .trim_matches(|c| c == '.' || c == '-')
        .chars()
        .take(NAME_MAX)
        .collect();
    let out = out.trim_end_matches(['.', '-']).to_string();
    if out.is_empty() {
        return Err("Escribe un nombre con letras o números.".into());
    }
    Ok(out)
}

/// Un nombre de la lista (`orion`) o uno de sus siguientes (`orion-2`).
fn pool_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    let base = match lower.rsplit_once('-') {
        Some((base, n)) if n.parse::<u32>().is_ok_and(|n| n >= 2) => base,
        _ => lower.as_str(),
    };
    STARS.contains(&base)
}

fn names_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|dir| dir.join(NAMES_FILE))
}

fn read_names(app: &AppHandle) -> HashMap<String, Vec<String>> {
    names_path(app)
        .and_then(|p| fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn used_names(app: &AppHandle, repo: &Repo) -> HashSet<String> {
    read_names(app)
        .remove(&repo.common.display().to_string())
        .unwrap_or_default()
        .into_iter()
        .collect()
}

/// Un nombre de la lista ya usado no se vuelve a proponer, aunque se borre su worktree.
fn mark_used(app: &AppHandle, repo: &Repo, name: &str) {
    if !pool_name(name) {
        return;
    }
    let Some(path) = names_path(app) else { return };
    let mut all = read_names(app);
    let list = all.entry(repo.common.display().to_string()).or_default();
    let name = name.to_lowercase();
    if !list.contains(&name) {
        list.push(name);
    }
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(&all) {
        let _ = fs::write(path, json);
    }
}

// ---------------------------------------------------------------- la rama

/// `git config user.email` y compañía pueden traer `12345+usuario@…`: se queda el usuario, y
/// solo si vale como trozo de una rama.
fn clean_user(raw: &str) -> Option<String> {
    let text = raw.trim();
    let text = text.split('@').next().unwrap_or("");
    let text = match text.split_once('+') {
        Some((n, rest)) if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) => rest,
        _ => text,
    };
    let valid = !text.is_empty()
        && text.len() <= 255
        && text.starts_with(|c: char| c.is_ascii_alphanumeric())
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        && !text.contains("..")
        && !text.ends_with('.')
        && !text.ends_with(".lock");
    valid.then(|| text.to_string())
}

/// El login de GitHub según `gh`, una vez por sesión de nexo (pregunta a la red).
fn gh_login(home: &Path) -> Option<String> {
    static LOGIN: Mutex<Option<Option<String>>> = Mutex::new(None);
    let mut cached = LOGIN.lock().ok()?;
    if let Some(login) = cached.as_ref() {
        return login.clone();
    }
    let login = workspace::find_program("gh", home).and_then(|gh| {
        let mut command = Command::new(gh);
        command.args(["api", "user", "--jq", ".login"]);
        run(command, None, Some(Duration::from_secs(6)))
            .ok()
            .filter(|o| o.code == Some(0))
            .and_then(|o| clean_user(&o.stdout))
    });
    *cached = Some(login.clone());
    login
}

/// Lo que va delante del nombre en la rama: el usuario de GitHub, como en Orca
/// (`github.user`, `user.username` o, si el repo está en GitHub, el login de `gh`). Sin
/// usuario, la rama se llama como el worktree.
fn branch_prefix(dir: &Path, home: &Path) -> String {
    for key in ["github.user", "user.username"] {
        if let Some(user) = git(dir, &["config", "--get", key])
            .ok()
            .and_then(|v| clean_user(&v))
        {
            return user;
        }
    }
    let on_github = git(dir, &["config", "--get-regexp", r"^remote\..*\.url$"])
        .is_ok_and(|urls| urls.contains("github.com"));
    if on_github {
        gh_login(home).unwrap_or_default()
    } else {
        String::new()
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Base {
    name: String,
    remote: bool,
}

/// Las ramas de las que se puede partir: primero las del remoto, luego las locales.
fn bases(dir: &Path) -> Vec<Base> {
    let Ok(text) = git(
        dir,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(symref)",
            "refs/heads",
            "refs/remotes",
        ],
    ) else {
        return Vec::new();
    };
    let mut remote = Vec::new();
    let mut local = Vec::new();
    for line in text.lines() {
        let (name, symref) = line.split_once('\0').unwrap_or((line, ""));
        if !symref.is_empty() {
            continue; // origin/HEAD
        }
        if let Some(n) = name.strip_prefix("refs/remotes/") {
            remote.push(Base {
                name: n.to_string(),
                remote: true,
            });
        } else if let Some(n) = name.strip_prefix("refs/heads/") {
            local.push(Base {
                name: n.to_string(),
                remote: false,
            });
        }
    }
    remote.extend(local);
    remote.truncate(BASES_MAX);
    remote
}

/// La rama de la que parte un worktree si no se elige otra: la principal del remoto, como en
/// Orca.
fn default_base(dir: &Path) -> Option<String> {
    if let Ok(head) = git(
        dir,
        &["symbolic-ref", "--quiet", "refs/remotes/origin/HEAD"],
    ) {
        let head = head.trim();
        if !head.is_empty() && ref_exists(dir, head) {
            return Some(head.trim_start_matches("refs/remotes/").to_string());
        }
    }
    [
        ("refs/remotes/origin/main", "origin/main"),
        ("refs/remotes/origin/master", "origin/master"),
        ("refs/heads/main", "main"),
        ("refs/heads/master", "master"),
    ]
    .into_iter()
    .find(|(r, _)| ref_exists(dir, r))
    .map(|(_, name)| name.to_string())
}

/// La rama de partida que pide la interfaz tiene que ser una de las de la lista.
fn pick_base(dir: &Path, wanted: &str) -> Result<Base, String> {
    let wanted = wanted.trim();
    let name = if wanted.is_empty() {
        default_base(dir).ok_or(
            "No se encontró la rama principal del repo: elige de qué rama parte el worktree.",
        )?
    } else {
        wanted.to_string()
    };
    bases(dir)
        .into_iter()
        .find(|b| b.name == name)
        .ok_or_else(|| format!("La rama «{name}» ya no existe."))
}

/// `origin/main` → (`origin`, `main`), con el remoto más largo que encaje.
fn remote_of(dir: &Path, base: &str) -> Option<(String, String)> {
    let remotes = git(dir, &["remote"]).ok()?;
    remotes
        .lines()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .filter_map(|r| {
            base.strip_prefix(r)
                .and_then(|rest| rest.strip_prefix('/'))
                .map(|branch| (r.to_string(), branch.to_string()))
        })
        .max_by_key(|(r, _)| r.len())
}

fn branch_taken(dir: &Path, branch: &str) -> bool {
    if ref_exists(dir, &format!("refs/heads/{branch}")) {
        return true;
    }
    git(
        dir,
        &[
            "for-each-ref",
            "--count=1",
            "--format=%(refname)",
            &format!("refs/remotes/*/{branch}"),
        ],
    )
    .is_ok_and(|out| !out.trim().is_empty())
}

/// La rama de un nombre si está libre (ni la rama, aquí o en el remoto, ni la carpeta).
fn free_branch(dir: &Path, prefix: &str, name: &str, dest: &Path) -> Option<String> {
    let with_prefix = (!prefix.is_empty()).then(|| format!("{prefix}/{name}"));
    // Si el prefijo no vale para git, la rama se llama como el worktree (lo mismo hace Orca).
    let branch = with_prefix
        .filter(|b| git_ok(dir, &["check-ref-format", "--branch", b]))
        .unwrap_or_else(|| name.to_string());
    if !git_ok(dir, &["check-ref-format", "--branch", &branch]) {
        return None;
    }
    if branch_taken(dir, &branch) || fs::symlink_metadata(dest.join(name)).is_ok() {
        return None;
    }
    Some(branch)
}

/// El nombre que se usará de verdad, con su rama: el que se escribió (limpio) o, si está
/// cogido, el siguiente libre (`-2`, `-3`…); sin nombre, el primero de la lista que no se
/// haya usado en el repo.
fn resolve_name(
    dir: &Path,
    raw: &str,
    prefix: &str,
    dest: &Path,
    used: &HashSet<String>,
) -> Result<(String, String), String> {
    if raw.trim().is_empty() {
        for tier in 1..=MAX_TRIES {
            for star in STARS {
                let name = if tier == 1 {
                    star.to_string()
                } else {
                    format!("{star}-{tier}")
                };
                if used.contains(&name) {
                    continue;
                }
                if let Some(branch) = free_branch(dir, prefix, &name, dest) {
                    return Ok((name, branch));
                }
            }
        }
        return Err("No queda ningún nombre libre: escribe uno.".into());
    }
    let wanted = sanitize_name(raw)?;
    for n in 1..=MAX_TRIES {
        let name = if n == 1 {
            wanted.clone()
        } else {
            format!("{wanted}-{n}")
        };
        if pool_name(&name) && used.contains(&name.to_lowercase()) {
            continue;
        }
        if let Some(branch) = free_branch(dir, prefix, &name, dest) {
            return Ok((name, branch));
        }
    }
    Err(format!(
        "No hay un nombre libre para «{wanted}»: escribe otro."
    ))
}

// ---------------------------------------------------------------- orca.yaml y .worktreeinclude

#[derive(Default, Debug, PartialEq)]
struct OrcaYaml {
    setup: Option<String>,
    shared: Vec<String>,
}

/// Una ruta relativa de una carpeta del repo, como la acepta Orca (sin `..`, `.`, `.git`,
/// segmentos vacíos ni rutas absolutas).
fn clean_rel(raw: &str) -> Option<String> {
    let text = raw.trim().replace('\\', "/");
    let text = text
        .strip_prefix("./")
        .unwrap_or(&text)
        .trim_end_matches('/');
    let parts: Vec<&str> = text.split('/').collect();
    let bad = text.is_empty()
        || text.starts_with('/')
        || text.as_bytes().get(1) == Some(&b':')
        || parts
            .iter()
            .any(|p| p.is_empty() || *p == "." || *p == ".." || *p == ".git");
    (!bad).then(|| text.to_string())
}

fn parse_orca_yaml(text: &str) -> Option<OrcaYaml> {
    if text.len() as u64 > TEXT_MAX {
        return None;
    }
    let doc = YamlLoader::load_from_str(text).ok()?.into_iter().next()?;
    let setup = doc["scripts"]["setup"]
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from);
    let mut shared: Vec<String> = Vec::new();
    if let Yaml::Array(items) = &doc["worktree"]["sharedDirectories"] {
        for dir in items
            .iter()
            .take(SHARED_MAX)
            .filter_map(Yaml::as_str)
            .filter_map(clean_rel)
        {
            if !shared.contains(&dir) {
                shared.push(dir);
            }
        }
    }
    Some(OrcaYaml { setup, shared })
}

fn read_small(path: &Path) -> Option<String> {
    let meta = fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > TEXT_MAX {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn orca_yaml_at(dir: &Path) -> OrcaYaml {
    read_small(&dir.join("orca.yaml"))
        .and_then(|t| parse_orca_yaml(&t))
        .unwrap_or_default()
}

/// `orca.yaml` tal como está en una rama (lo que tendrá el worktree al crearlo).
fn orca_yaml_in(dir: &Path, base: &str) -> OrcaYaml {
    git(dir, &["show", &format!("{base}:orca.yaml")])
        .ok()
        .and_then(|t| parse_orca_yaml(&t))
        .unwrap_or_default()
}

/// Las líneas de `.worktreeinclude` que se pueden copiar: rutas literales (sin `!`, `*` ni
/// `?`), relativas y dentro del repo, sin repetir; las demás se saltan, como en Orca.
fn parse_include(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('!') || line.contains('*') || line.contains('?') {
            continue;
        }
        let Some(path) = clean_rel(line) else {
            continue;
        };
        if !out.contains(&path) {
            out.push(path);
        }
        if out.len() >= INCLUDE_MAX {
            break;
        }
    }
    out
}

/// De esas rutas, las que git ignora (`git check-ignore`): lo que está en git ya lo trae el
/// worktree.
fn ignored(dir: &Path, paths: &[String]) -> HashSet<String> {
    if paths.is_empty() {
        return HashSet::new();
    }
    let mut command = git_command(dir);
    command.args([
        "-c",
        "core.quotePath=false",
        "check-ignore",
        "-z",
        "--stdin",
    ]);
    let mut input = paths.join("\0");
    input.push('\0');
    match run(
        command,
        Some(input.into_bytes()),
        Some(Duration::from_secs(15)),
    ) {
        // 1: ninguna está ignorada.
        Ok(out) if matches!(out.code, Some(0) | Some(1)) => out
            .stdout
            .split('\0')
            .filter(|p| !p.is_empty())
            .map(String::from)
            .collect(),
        _ => HashSet::new(),
    }
}

/// Lo que se copia de `.worktreeinclude`: rutas que existen en el repo y que git ignora.
fn include_paths(main: &Path) -> Vec<String> {
    let Some(text) = read_small(&main.join(INCLUDE_FILE)) else {
        return Vec::new();
    };
    let candidates: Vec<String> = parse_include(&text)
        .into_iter()
        .filter(|p| fs::symlink_metadata(main.join(p)).is_ok())
        .collect();
    let ignored = ignored(main, &candidates);
    let mut out: Vec<String> = candidates
        .into_iter()
        .filter(|p| ignored.contains(p))
        .collect();
    out.sort();
    out
}

/// Las carpetas de `sharedDirectories` que se enlazan: carpetas que existen y que git ignora.
fn shared_dirs(main: &Path, listed: &[String]) -> Vec<String> {
    let candidates: Vec<String> = listed
        .iter()
        .filter(|p| main.join(p).is_dir())
        .cloned()
        .collect();
    let ignored = ignored(main, &candidates);
    let mut out: Vec<String> = candidates
        .into_iter()
        .filter(|p| ignored.contains(p))
        .collect();
    out.sort();
    out
}

/// El destino de una copia o un enlace, si su carpeta queda dentro del worktree (una carpeta
/// del repo que sea un enlace a otro sitio no se sigue).
fn target_inside(root: &Path, rel: &str) -> Option<PathBuf> {
    let target = root.join(rel);
    let parent = target.parent()?;
    fs::create_dir_all(parent).ok()?;
    let real = parent.canonicalize().ok()?;
    let root = root.canonicalize().ok()?;
    let name = target.file_name()?;
    real.starts_with(&root).then(|| real.join(name))
}

/// Cuánto ocupa algo, sin seguir enlaces; para en cuanto pasa del tope.
fn measure(path: &Path, max_entries: u64) -> (u64, u64) {
    let mut bytes = 0;
    let mut entries = 0;
    let mut stack = vec![path.to_path_buf()];
    while let Some(p) = stack.pop() {
        let Ok(meta) = fs::symlink_metadata(&p) else {
            continue;
        };
        entries += 1;
        if entries > max_entries {
            break;
        }
        if meta.is_dir() {
            if let Ok(read) = fs::read_dir(&p) {
                stack.extend(read.flatten().map(|e| e.path()));
            }
        } else if meta.is_file() {
            bytes += meta.len();
        }
    }
    (bytes, entries)
}

fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(src)?;
    if meta.is_dir() {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)?.flatten() {
            copy_tree(&entry.path(), &dst.join(entry.file_name()))?;
        }
        Ok(())
    } else if meta.file_type().is_symlink() {
        #[cfg(unix)]
        std::os::unix::fs::symlink(fs::read_link(src)?, dst)?;
        Ok(())
    } else {
        fs::copy(src, dst).map(|_| ())
    }
}

/// Copia lo de `.worktreeinclude` (sin pisar lo que ya haya; un enlace del repo se copia con
/// lo que tiene dentro, como en Orca). Devuelve los avisos de lo que no se copió.
fn copy_included(main: &Path, folder: &Path, paths: &[String]) -> Vec<String> {
    let mut warnings = Vec::new();
    let mut bytes_left = COPY_MAX_BYTES;
    let mut entries_left = COPY_MAX_ENTRIES;
    for rel in paths {
        let source = main.join(rel);
        let Some(target) = target_inside(folder, rel) else {
            warnings.push(format!(
                "No se copió «{rel}»: su carpeta sale del worktree."
            ));
            continue;
        };
        if fs::symlink_metadata(&target).is_ok() {
            continue;
        }
        let real = fs::canonicalize(&source).unwrap_or(source);
        let (bytes, entries) = measure(&real, entries_left + 1);
        if bytes > bytes_left || entries > entries_left {
            warnings.push(format!(
                "No se copió «{rel}»: pasa de 2 GB o de 50 000 archivos en total."
            ));
            continue;
        }
        bytes_left -= bytes;
        entries_left -= entries;
        if let Err(e) = copy_tree(&real, &target) {
            warnings.push(format!("No se pudo copiar «{rel}»: {e}"));
        }
    }
    warnings
}

/// Enlaza las carpetas compartidas del repo dentro del worktree.
fn link_shared(main: &Path, folder: &Path, dirs: &[String]) -> Vec<String> {
    let mut warnings = Vec::new();
    for rel in dirs {
        let Some(target) = target_inside(folder, rel) else {
            warnings.push(format!(
                "No se enlazó «{rel}»: su carpeta sale del worktree."
            ));
            continue;
        };
        if fs::symlink_metadata(&target).is_ok() {
            continue;
        }
        #[cfg(unix)]
        if let Err(e) = std::os::unix::fs::symlink(main.join(rel), &target) {
            warnings.push(format!("No se pudo enlazar «{rel}»: {e}"));
        }
        #[cfg(not(unix))]
        warnings.push(format!(
            "No se enlazó «{rel}»: solo se hace en Linux y macOS."
        ));
    }
    warnings
}

// ---------------------------------------------------------------- el setup

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SetupState {
    /// `running`, `ok` o `error` (y, al leerlo, `interrupted` si nexo se cerró a medias).
    pub state: String,
    pub code: Option<i32>,
    pub started: i64,
    pub ended: Option<i64>,
    /// La primera línea del script, para reconocerlo.
    pub command: String,
}

#[derive(Serialize)]
pub struct SetupView {
    #[serde(flatten)]
    state: SetupState,
    log: String,
    /// Solo se enseña el final de la salida.
    truncated: bool,
}

fn running() -> &'static Mutex<HashSet<PathBuf>> {
    static RUNNING: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    RUNNING.get_or_init(|| Mutex::new(HashSet::new()))
}

fn first_line(script: &str) -> String {
    let line = script
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let short: String = line.chars().take(80).collect();
    if short.len() < line.len() || script.trim().lines().count() > 1 {
        format!("{short} …")
    } else {
        short
    }
}

/// El script que se ejecuta, como lo escribe Orca.
fn runner_text(script: &str) -> String {
    format!("#!/usr/bin/env bash\nset -e\n{}\n", script.trim_end())
}

/// El `PATH` del shell del usuario: nexo se abre desde el escritorio, con uno corto, y un setup
/// suele llamar a `npm`, `pnpm`, `uv`… Se pregunta una vez a un shell de inicio de sesión.
fn login_path(home: &Path) -> String {
    static PATH: OnceLock<String> = OnceLock::new();
    PATH.get_or_init(|| {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let mut command = Command::new(shell);
        command.args(["-l", "-i", "-c", "printf '\\n__NEXO_PATH__%s' \"$PATH\""]);
        let found = run(command, None, Some(Duration::from_secs(5)))
            .ok()
            .and_then(|out| {
                out.stdout
                    .rsplit_once("__NEXO_PATH__")
                    .map(|(_, p)| p.trim().to_string())
            })
            .filter(|p| !p.is_empty());
        found.unwrap_or_else(|| {
            let mut paths: Vec<PathBuf> = std::env::var_os("PATH")
                .map(|p| std::env::split_paths(&p).collect())
                .unwrap_or_default();
            paths.extend([home.join(".local/bin"), home.join(".cargo/bin")]);
            std::env::join_paths(paths)
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
    })
    .clone()
}

fn write_state(dir: &Path, state: &SetupState) {
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let tmp = dir.join(format!("{SETUP_STATE}.tmp"));
        if fs::write(&tmp, json).is_ok() {
            let _ = fs::rename(&tmp, dir.join(SETUP_STATE));
        }
    }
}

/// Ejecuta `scripts.setup` en la carpeta del worktree, en segundo plano. La salida (la normal y
/// la de errores, juntas) va a `.nexo/setup.log` y el estado a `.nexo/setup.json`.
fn start_setup(main: &Path, folder: &Path, script: &str, path_env: String) -> Result<(), String> {
    let dir = folder.join(NEXO_DIR);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let runner = dir.join(SETUP_RUNNER);
    fs::write(&runner, runner_text(script)).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&runner, fs::Permissions::from_mode(0o755));
    }
    let log = fs::File::create(dir.join(SETUP_LOG)).map_err(|e| e.to_string())?;
    let mut state = SetupState {
        state: "running".into(),
        code: None,
        started: now_ms(),
        ended: None,
        command: first_line(script),
    };
    write_state(&dir, &state);
    let name = folder
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut command = Command::new("bash");
    command
        .arg(&runner)
        .current_dir(folder)
        .env("ORCA_ROOT_PATH", main)
        .env("ORCA_WORKTREE_PATH", folder)
        .env("ORCA_WORKSPACE_NAME", &name)
        .env("PATH", path_env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let log = Arc::new(Mutex::new(log));
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(e) => {
            if let Ok(mut log) = log.lock() {
                let _ = writeln!(log, "No se pudo ejecutar bash: {e}");
            }
            state.state = "error".into();
            state.ended = Some(now_ms());
            write_state(&dir, &state);
            return Err(format!("No se pudo ejecutar el setup: {e}"));
        }
    };
    if let Ok(mut set) = running().lock() {
        set.insert(folder.to_path_buf());
    }
    let pump = |pipe: Option<Box<dyn Read + Send>>, log: Arc<Mutex<fs::File>>| {
        std::thread::spawn(move || {
            let Some(mut pipe) = pipe else { return };
            let mut buf = [0u8; 8192];
            while let Ok(n) = pipe.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if let Ok(mut log) = log.lock() {
                    let _ = log.write_all(&buf[..n]);
                }
            }
        })
    };
    let out = pump(child.stdout.take().map(|p| Box::new(p) as _), log.clone());
    let err = pump(child.stderr.take().map(|p| Box::new(p) as _), log);
    let folder = folder.to_path_buf();
    std::thread::spawn(move || {
        let status = child.wait();
        let _ = out.join();
        let _ = err.join();
        state.code = status.as_ref().ok().and_then(|s| s.code());
        state.state = if status.is_ok_and(|s| s.success()) {
            "ok".into()
        } else {
            "error".into()
        };
        state.ended = Some(now_ms());
        write_state(&dir, &state);
        if let Ok(mut set) = running().lock() {
            set.remove(&folder);
        }
    });
    Ok(())
}

/// La salida de una terminal, para leerla como texto: sin secuencias de escape (colores,
/// cursor) y, en cada línea, solo lo que queda después del último retorno de carro (las
/// barras de progreso se reescriben así).
fn clean_log(raw: &str) -> String {
    let mut text = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            match chars.next() {
                Some('[') => {
                    for c in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' {
                            break;
                        }
                        if c == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        if c == '\n' || c == '\r' || c == '\t' || !c.is_control() {
            text.push(c);
        }
    }
    text.split('\n')
        .map(|line| {
            let line = line.strip_suffix('\r').unwrap_or(line);
            line.rsplit('\r').next().unwrap_or(line)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn tail(path: &Path, max: u64) -> (String, bool) {
    let Ok(mut file) = fs::File::open(path) else {
        return (String::new(), false);
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let skip = len.saturating_sub(max);
    if skip > 0 {
        use std::io::{Seek, SeekFrom};
        let _ = file.seek(SeekFrom::Start(skip));
    }
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if skip > 0 {
        // La primera línea puede estar cortada por la mitad.
        if let Some(at) = text.find('\n') {
            text.drain(..=at);
        }
    }
    (text, skip > 0)
}

fn read_setup(root: &Path) -> Option<SetupView> {
    let dir = root.join(NEXO_DIR);
    let mut state: SetupState =
        serde_json::from_str(&fs::read_to_string(dir.join(SETUP_STATE)).ok()?).ok()?;
    if state.state == "running" && !running().lock().is_ok_and(|set| set.contains(root)) {
        state.state = "interrupted".into();
    }
    let (log, truncated) = tail(&dir.join(SETUP_LOG), LOG_TAIL);
    Some(SetupView {
        state,
        log: clean_log(&log),
        truncated,
    })
}

// ---------------------------------------------------------------- crear

/// Lo que se va a hacer, para enseñarlo en el formulario antes de crear.
#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Plan {
    name: String,
    branch: String,
    folder: String,
    base: String,
    /// La primera línea de `scripts.setup` de `orca.yaml` en la rama de partida.
    setup: Option<String>,
    /// Lo que se copia (`.worktreeinclude`) y lo que se enlaza (`sharedDirectories`).
    include: Vec<String>,
    shared: Vec<String>,
}

fn plan_in(
    repo: &Repo,
    dir: &Path,
    raw: &str,
    base: &str,
    prefix: &str,
    dest: &Path,
    used: &HashSet<String>,
) -> Result<Plan, String> {
    let base = pick_base(dir, base)?;
    let (name, branch) = resolve_name(dir, raw, prefix, dest, used)?;
    let listed = orca_yaml_at(&repo.main).shared;
    Ok(Plan {
        folder: dest.join(&name).display().to_string(),
        setup: orca_yaml_in(dir, &base.name).setup.map(|s| first_line(&s)),
        include: include_paths(&repo.main),
        shared: shared_dirs(&repo.main, &listed),
        name,
        branch,
        base: base.name,
    })
}

struct Made {
    folder: PathBuf,
    name: String,
    branch: String,
    warnings: Vec<String>,
}

/// Crea el worktree: actualiza la rama de partida si es del remoto, busca un nombre libre,
/// `git worktree add`, la configuración de la rama y las copias y enlaces. No abre nada.
#[allow(clippy::too_many_arguments)]
fn create_in(
    repo: &Repo,
    dir: &Path,
    raw: &str,
    base: &str,
    prefix: &str,
    dest: &Path,
    used: &HashSet<String>,
    step: &dyn Fn(&str, String),
) -> Result<Made, String> {
    let mut warnings = Vec::new();
    let base = pick_base(dir, base)?;
    if base.remote
        && let Some((remote, branch)) = remote_of(dir, &base.name)
    {
        step("fetch", format!("Actualizando {}…", base.name));
        let mut command = git_command(dir);
        command.args(["fetch", "--quiet", "--no-tags", &remote, &branch]);
        let fetched = run(command, None, Some(FETCH_TIMEOUT));
        let problem = match fetched {
            Ok(out) if out.code == Some(0) => None,
            Ok(out) => Some(git_error(&out)),
            Err(e) => Some(e),
        };
        if let Some(problem) = problem {
            warnings.push(format!(
                "No se pudo actualizar {} ({problem}): el worktree parte de la copia que había.",
                base.name
            ));
        }
    }
    let (name, branch) = resolve_name(dir, raw, prefix, dest, used)?;
    fs::create_dir_all(dest).map_err(|e| format!("No se pudo crear {}: {e}", dest.display()))?;
    let folder = dest.join(&name);
    step("add", format!("Creando {name}…"));
    let folder_arg = folder.to_string_lossy().into_owned();
    git(
        dir,
        &[
            "worktree",
            "add",
            "--no-track",
            "-b",
            &branch,
            &folder_arg,
            &base.name,
        ],
    )
    .map_err(|e| format!("git no pudo crear el worktree: {e}"))?;
    let folder = folder.canonicalize().unwrap_or(folder);
    // La base de la rama (la usan Orca y otras herramientas para comparar) y que el primer
    // `git push` cree la rama en el remoto.
    let _ = git(
        &folder,
        &[
            "config",
            "--local",
            "--replace-all",
            &format!("branch.{branch}.base"),
            &base.name,
        ],
    );
    if git(&folder, &["config", "--get", "push.autoSetupRemote"]).is_err() {
        let _ = git(
            &folder,
            &["config", "--local", "push.autoSetupRemote", "true"],
        );
    }
    let include = include_paths(&repo.main);
    let shared = shared_dirs(&repo.main, &orca_yaml_at(&repo.main).shared);
    if !include.is_empty() || !shared.is_empty() {
        step("files", "Copiando archivos…".into());
    }
    warnings.extend(copy_included(&repo.main, &folder, &include));
    warnings.extend(link_shared(&repo.main, &folder, &shared));
    Ok(Made {
        folder,
        name,
        branch,
        warnings,
    })
}

// ---------------------------------------------------------------- comandos

#[derive(Serialize)]
pub struct WtItem {
    path: String,
    name: String,
    branch: String,
    main: bool,
    current: bool,
    missing: bool,
}

#[derive(Serialize)]
pub struct WtList {
    /// El nombre del repo, si el workspace es la raíz de uno.
    repo: Option<String>,
    items: Vec<WtItem>,
}

fn list_of(root: &Path) -> WtList {
    let Some(repo) = repo_of(root) else {
        return WtList {
            repo: None,
            items: Vec::new(),
        };
    };
    let items = repo
        .trees
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.bare)
        .map(|(i, t)| WtItem {
            path: t.path.display().to_string(),
            name: t
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            branch: t
                .branch
                .clone()
                .unwrap_or_else(|| t.head.chars().take(7).collect()),
            main: i == 0,
            current: same_path(&t.path, root),
            missing: t.prunable || !t.path.is_dir(),
        })
        .collect();
    WtList {
        repo: Some(repo.name),
        items,
    }
}

/// Los worktrees del repo del workspace (vacío si no es la raíz de un repo).
#[tauri::command]
pub async fn worktree_list(app: AppHandle, path: String) -> Result<WtList, String> {
    let root = workspace::ws_root(&app, &path)?;
    tauri::async_runtime::spawn_blocking(move || list_of(&root))
        .await
        .map_err(|e| e.to_string())
}

#[derive(Serialize)]
pub struct WtForm {
    repo: String,
    bases: Vec<Base>,
    base: Option<String>,
    /// La carpeta donde se crean (`~/nexo/workspaces/<repo>`) y la personal, para enseñar las
    /// rutas con `~`.
    parent: String,
    home: String,
    plan: Option<Plan>,
    error: Option<String>,
}

/// Lo que necesita el formulario al abrirse: las ramas, la de partida y el plan con el nombre
/// propuesto.
#[tauri::command]
pub async fn worktree_form(app: AppHandle, path: String) -> Result<WtForm, String> {
    let root = workspace::ws_root(&app, &path)?;
    tauri::async_runtime::spawn_blocking(move || {
        let repo = require_repo(&root)?;
        let dest = dest_parent(&app, &repo)?;
        let used = used_names(&app, &repo);
        let prefix = branch_prefix(&root, &home(&app));
        let base = default_base(&root);
        let plan = plan_in(&repo, &root, "", "", &prefix, &dest, &used);
        Ok(WtForm {
            repo: repo.name.clone(),
            bases: bases(&root),
            base,
            parent: dest.display().to_string(),
            home: home(&app).display().to_string(),
            error: plan.as_ref().err().cloned(),
            plan: plan.ok(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Lo que saldría con un nombre y una rama de partida (se pide mientras se escribe).
#[tauri::command]
pub async fn worktree_plan(
    app: AppHandle,
    path: String,
    name: String,
    base: String,
) -> Result<Plan, String> {
    let root = workspace::ws_root(&app, &path)?;
    tauri::async_runtime::spawn_blocking(move || {
        let repo = require_repo(&root)?;
        let dest = dest_parent(&app, &repo)?;
        let used = used_names(&app, &repo);
        let prefix = branch_prefix(&root, &home(&app));
        plan_in(&repo, &root, &name, &base, &prefix, &dest, &used)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct Created {
    info: WsInfo,
    branch: String,
    /// Se puso en marcha el setup de `orca.yaml`.
    setup: bool,
    warnings: Vec<String>,
}

/// Crea el worktree y lo deja como workspace actual (con los extras del de origen). El
/// progreso va por el canal: `{step, text}`.
#[tauri::command]
pub async fn worktree_create(
    app: AppHandle,
    path: String,
    name: String,
    base: String,
    setup: bool,
    channel: Channel<Value>,
) -> Result<Created, String> {
    let root = workspace::ws_root(&app, &path)?;
    tauri::async_runtime::spawn_blocking(move || {
        let repo = require_repo(&root)?;
        let dest = dest_parent(&app, &repo)?;
        let used = used_names(&app, &repo);
        let home = home(&app);
        let prefix = branch_prefix(&root, &home);
        let step = |kind: &str, text: String| {
            let _ = channel.send(json!({ "step": kind, "text": text }));
        };
        let made = create_in(&repo, &root, &name, &base, &prefix, &dest, &used, &step)?;
        mark_used(&app, &repo, &made.name);
        step("open", format!("Abriendo {}…", made.name));
        let extras = workspace::read_config(&root).extras;
        workspace::update_config(&made.folder, |c| c.extras = extras)?;
        let info = workspace::remember(&app, &made.folder)?;
        let mut warnings = made.warnings;
        let mut started = false;
        if setup && let Some(script) = orca_yaml_at(&made.folder).setup {
            match start_setup(&repo.main, &made.folder, &script, login_path(&home)) {
                Ok(()) => started = true,
                Err(e) => warnings.push(e),
            }
        }
        Ok(Created {
            info,
            branch: made.branch,
            setup: started,
            warnings,
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// La salida y el estado del setup del workspace, si tuvo uno.
#[tauri::command]
pub fn worktree_setup(app: AppHandle, path: String) -> Result<Option<SetupView>, String> {
    let root = workspace::ws_root(&app, &path)?;
    Ok(read_setup(&root))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("nexo-wt-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root.canonicalize().unwrap()
    }

    fn sh(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=Prueba",
                "-c",
                "user.email=prueba@example.com",
            ])
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Un repo con remoto (`origin`, un repo desnudo) y lo que usan las copias: un `.env`
    /// ignorado, una carpeta ignorada para compartir y un `orca.yaml` con setup.
    fn repo_with_remote(name: &str) -> (PathBuf, PathBuf) {
        let base = temp(name);
        let remote = base.join("remote.git");
        sh(
            &base,
            &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
        );
        let seed = base.join("seed");
        sh(&base, &["init", "-b", "main", seed.to_str().unwrap()]);
        fs::write(seed.join(".gitignore"), ".env\nnode_modules/\nnotas.txt\n").unwrap();
        fs::write(
            seed.join(".worktreeinclude"),
            "# lo que git no lleva\n.env\n./.env\nsecretos/*\n../fuera\nREADME.md\nno-existe\n",
        )
        .unwrap();
        fs::write(
            seed.join("orca.yaml"),
            "scripts:\n  setup: |\n    echo \"raiz=$ORCA_ROOT_PATH\" > setup-hecho.txt\n    echo listo\nworktree:\n  sharedDirectories:\n    - node_modules\n    - ../fuera\n",
        )
        .unwrap();
        fs::write(seed.join("README.md"), "hola\n").unwrap();
        sh(&seed, &["add", "."]);
        sh(&seed, &["commit", "-m", "primero"]);
        sh(
            &seed,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        sh(&seed, &["push", "-u", "origin", "main"]);
        let repo = base.join("proyecto");
        sh(
            &base,
            &["clone", remote.to_str().unwrap(), repo.to_str().unwrap()],
        );
        fs::write(repo.join(".env"), "CLAVE=1\n").unwrap();
        fs::create_dir_all(repo.join("node_modules/paquete")).unwrap();
        fs::write(repo.join("node_modules/paquete/index.js"), "x").unwrap();
        (base, repo.canonicalize().unwrap())
    }

    #[test]
    fn el_nombre_se_limpia_como_en_orca() {
        assert_eq!(sanitize_name("  Mi rama nueva! ").unwrap(), "Mi-rama-nueva");
        assert_eq!(sanitize_name("a/b\\c").unwrap(), "a-b-c");
        assert_eq!(sanitize_name("..oculta..").unwrap(), "oculta");
        assert_eq!(sanitize_name("v1...2").unwrap(), "v1.2");
        assert_eq!(sanitize_name("canción_ñ").unwrap(), "canción_ñ");
        assert_eq!(sanitize_name("--x--").unwrap(), "x");
        for bad in ["", "   ", "...", "///", "🚀"] {
            assert!(sanitize_name(bad).is_err(), "{bad:?} debería rechazarse");
        }
        assert_eq!(sanitize_name(&"a".repeat(100)).unwrap().len(), NAME_MAX);
    }

    #[test]
    fn el_usuario_de_la_rama_se_queda_en_lo_que_vale() {
        assert_eq!(clean_user("MarcoAle05\n").as_deref(), Some("MarcoAle05"));
        assert_eq!(
            clean_user("12345+marco@users.noreply.github.com").as_deref(),
            Some("marco")
        );
        assert_eq!(clean_user("ana@correo.es").as_deref(), Some("ana"));
        for bad in ["", "-ana", "ana maría", "a..b", "ana.", "x.lock"] {
            assert_eq!(clean_user(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn los_nombres_propuestos_son_de_la_lista_o_sus_siguientes() {
        assert!(pool_name("orion"));
        assert!(pool_name("Vega-3"));
        assert!(!pool_name("orion-1"));
        assert!(!pool_name("mi-rama"));
        let unique: HashSet<_> = STARS.iter().collect();
        assert_eq!(unique.len(), STARS.len(), "la lista no repite nombres");
        for star in STARS {
            assert_eq!(sanitize_name(star).unwrap(), *star);
        }
    }

    #[test]
    fn la_lista_de_worktrees_de_git_se_entiende() {
        let text = "worktree /repo\0HEAD 1111111111\0branch refs/heads/main\0\0worktree /otro dir\0HEAD 2222222\0detached\0\0worktree /viejo\0HEAD 333\0branch refs/heads/marco/vega\0prunable gitdir file points to non-existent location\0\0";
        let trees = parse_worktrees(text);
        assert_eq!(trees.len(), 3);
        assert_eq!(trees[0].path, PathBuf::from("/repo"));
        assert_eq!(trees[0].branch.as_deref(), Some("main"));
        assert_eq!(trees[1].path, PathBuf::from("/otro dir"));
        assert_eq!(trees[1].branch, None);
        assert_eq!(trees[1].head, "2222222");
        assert_eq!(trees[2].branch.as_deref(), Some("marco/vega"));
        assert!(trees[2].prunable);
    }

    #[test]
    fn worktreeinclude_solo_acepta_rutas_literales_dentro_del_repo() {
        let text = "# comentario\n\n.env\n./.env\nconfig\\local.json\ndist/\n*.key\n!no\na?b\n/etc/passwd\n../fuera\na//b\n.git/config\nC:/x\n";
        assert_eq!(
            parse_include(text),
            vec![".env", "config/local.json", "dist"]
        );
    }

    #[test]
    fn orca_yaml_da_el_setup_y_las_carpetas_compartidas() {
        let yaml = "scripts:\n  setup: |\n    pnpm install\n    pnpm build\n  archive: rm -rf x\nworktree:\n  sharedDirectories:\n    - node_modules/\n    - ./.venv\n    - ../fuera\n    - .git\n    - node_modules\n";
        let parsed = parse_orca_yaml(yaml).unwrap();
        assert_eq!(parsed.setup.as_deref(), Some("pnpm install\npnpm build"));
        assert_eq!(parsed.shared, vec!["node_modules", ".venv"]);
        assert_eq!(
            first_line(parsed.setup.as_deref().unwrap()),
            "pnpm install …"
        );
        assert_eq!(parse_orca_yaml("scripts: [").map(|p| p.setup), None);
        assert_eq!(parse_orca_yaml("otra: cosa\n"), Some(OrcaYaml::default()));
    }

    #[test]
    fn la_salida_del_setup_se_lee_como_texto() {
        let raw = "\u{1b}[32mok\u{1b}[0m\r\nbajando 10%\rbajando 50%\rbajando 100%\n\u{1b}]0;titulo\u{7}fin\u{8}\n";
        assert_eq!(clean_log(raw), "ok\nbajando 100%\nfin\n");
    }

    #[test]
    fn crea_el_worktree_con_su_rama_sus_copias_y_su_setup() {
        let (base, repo_dir) = repo_with_remote("crear");
        let repo = repo_of(&repo_dir).expect("es un repo");
        assert_eq!(repo.name, "proyecto");
        assert_eq!(default_base(&repo_dir).as_deref(), Some("origin/main"));
        assert!(
            bases(&repo_dir)
                .iter()
                .any(|b| b.name == "origin/main" && b.remote)
        );
        assert!(
            bases(&repo_dir)
                .iter()
                .any(|b| b.name == "main" && !b.remote)
        );
        // Una subcarpeta del repo no es la raíz: no ofrece worktrees.
        fs::create_dir_all(repo_dir.join("sub")).unwrap();
        assert!(repo_of(&repo_dir.join("sub")).is_none());

        let dest = base.join("workspaces").join(&repo.name);
        let used = HashSet::new();
        let plan = plan_in(&repo, &repo_dir, "", "", "marco", &dest, &used).unwrap();
        assert_eq!(plan.name, "orion");
        assert_eq!(plan.branch, "marco/orion");
        assert_eq!(plan.base, "origin/main");
        assert_eq!(
            plan.setup.as_deref(),
            Some("echo \"raiz=$ORCA_ROOT_PATH\" > setup-hecho.txt …")
        );
        assert_eq!(plan.include, vec![".env"]);
        assert_eq!(plan.shared, vec!["node_modules"]);
        assert!(pick_base(&repo_dir, "no-existe").is_err());

        let steps = Mutex::new(Vec::new());
        let step = |s: &str, _: String| steps.lock().unwrap().push(s.to_string());
        let made = create_in(
            &repo, &repo_dir, "Mi Rama", "", "marco", &dest, &used, &step,
        )
        .unwrap();
        assert_eq!(made.name, "Mi-Rama");
        assert_eq!(made.branch, "marco/Mi-Rama");
        assert!(made.warnings.is_empty(), "{:?}", made.warnings);
        assert_eq!(*steps.lock().unwrap(), vec!["fetch", "add", "files"]);
        let folder = dest.join("Mi-Rama");
        assert!(folder.join("README.md").is_file());
        assert_eq!(
            fs::read_to_string(folder.join(".env")).unwrap(),
            "CLAVE=1\n"
        );
        let link = fs::read_link(folder.join("node_modules")).unwrap();
        assert_eq!(link, repo_dir.join("node_modules"));
        assert_eq!(
            git(&folder, &["config", "--get", "branch.marco/Mi-Rama.base"])
                .unwrap()
                .trim(),
            "origin/main"
        );
        assert_eq!(
            git(&folder, &["rev-parse", "--abbrev-ref", "HEAD"])
                .unwrap()
                .trim(),
            "marco/Mi-Rama"
        );
        // Sin seguimiento: la rama nueva no apunta a origin/main.
        assert!(git(&folder, &["rev-parse", "--abbrev-ref", "@{upstream}"]).is_err());

        // El mismo nombre otra vez: la rama ya existe, así que se usa el siguiente.
        let again = create_in(
            &repo,
            &repo_dir,
            "Mi Rama",
            "main",
            "marco",
            &dest,
            &used,
            &|_, _| {},
        )
        .unwrap();
        assert_eq!(again.name, "Mi-Rama-2");
        // Un nombre propuesto ya usado no se vuelve a proponer.
        let used: HashSet<String> = ["orion".to_string()].into();
        let next = plan_in(&repo, &repo_dir, "", "", "", &dest, &used).unwrap();
        assert_eq!(next.name, "vega");
        assert_eq!(next.branch, "vega");

        let list = list_of(&repo_dir);
        assert_eq!(list.repo.as_deref(), Some("proyecto"));
        assert_eq!(list.items.len(), 3);
        assert!(list.items[0].main && list.items[0].current);
        assert!(
            list.items
                .iter()
                .any(|i| i.branch == "marco/Mi-Rama-2" && !i.current)
        );
        let from_child = list_of(&folder.canonicalize().unwrap());
        assert!(
            from_child
                .items
                .iter()
                .any(|i| i.name == "Mi-Rama" && i.current)
        );

        // El setup corre en la carpeta del worktree, con las variables de Orca.
        let script = orca_yaml_at(&folder).setup.unwrap();
        let path_env = std::env::var("PATH").unwrap_or_default();
        let folder = folder.canonicalize().unwrap();
        start_setup(&repo.main, &folder, &script, path_env).unwrap();
        let start = Instant::now();
        let view = loop {
            let view = read_setup(&folder).unwrap();
            if view.state.state != "running" || start.elapsed() > Duration::from_secs(10) {
                break view;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        assert_eq!(view.state.state, "ok", "{}", view.log);
        assert_eq!(view.state.code, Some(0));
        assert_eq!(view.log.trim(), "listo");
        assert_eq!(
            fs::read_to_string(folder.join("setup-hecho.txt"))
                .unwrap()
                .trim(),
            format!("raiz={}", repo.main.display())
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn un_setup_que_falla_queda_como_error_con_su_salida() {
        let dir = temp("setup-falla");
        let path_env = std::env::var("PATH").unwrap_or_default();
        start_setup(&dir, &dir, "echo antes\nfalse\necho nunca", path_env).unwrap();
        let start = Instant::now();
        let view = loop {
            let view = read_setup(&dir).unwrap();
            if view.state.state != "running" || start.elapsed() > Duration::from_secs(10) {
                break view;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        assert_eq!(view.state.state, "error");
        assert_eq!(view.state.code, Some(1));
        assert_eq!(view.log.trim(), "antes");
        assert_eq!(view.state.command, "echo antes …");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn un_setup_a_medias_al_cerrar_nexo_sale_interrumpido() {
        let dir = temp("setup-medias");
        let nexo = dir.join(NEXO_DIR);
        fs::create_dir_all(&nexo).unwrap();
        write_state(
            &nexo,
            &SetupState {
                state: "running".into(),
                code: None,
                started: 1,
                ended: None,
                command: "pnpm install".into(),
            },
        );
        assert_eq!(read_setup(&dir).unwrap().state.state, "interrupted");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn las_copias_no_salen_del_worktree() {
        let dir = temp("fuera");
        let main = dir.join("main");
        let wt = dir.join("wt");
        let outside = dir.join("outside");
        for d in [&main, &wt, &outside] {
            fs::create_dir_all(d).unwrap();
        }
        fs::create_dir_all(main.join("conf")).unwrap();
        fs::write(main.join("conf/secreto"), "x").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, wt.join("conf")).unwrap();
        let warnings = copy_included(&main, &wt, &["conf/secreto".into()]);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(!outside.join("secreto").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
