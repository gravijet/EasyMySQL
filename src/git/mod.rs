// Git fuer den Projektordner: Stand, Aenderungen ansehen, vormerken, Commit, Branches, Verlauf, Pull/Push,
// Klonen. Arbeitet mit dem git-Programm (eigenes MinGit unter Windows oder das des Systems). Fuer GitHub
// wird das Token nur fuer den einen Aufruf mitgegeben und nirgends in der Git-Konfiguration gespeichert.

pub mod github;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Stand einer Datei (wie `git status --porcelain`)
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Change {
    Modified,
    Added,
    Deleted,
    Renamed,
    Untracked,
    Conflict,
}

impl Change {
    pub fn letter(self) -> &'static str {
        match self {
            Change::Modified => "M",
            Change::Added => "A",
            Change::Deleted => "D",
            Change::Renamed => "R",
            Change::Untracked => "U",
            Change::Conflict => "!",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Change::Modified => crate::i18n::tr("geändert", "modified"),
            Change::Added => crate::i18n::tr("neu", "added"),
            Change::Deleted => crate::i18n::tr("gelöscht", "deleted"),
            Change::Renamed => crate::i18n::tr("umbenannt", "renamed"),
            Change::Untracked => crate::i18n::tr("nicht verfolgt", "untracked"),
            Change::Conflict => crate::i18n::tr("Konflikt", "conflict"),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileStatus {
    /// Pfad relativ zum Repository
    pub path: String,
    /// Vorgemerkt (Index)
    pub staged: Option<Change>,
    /// Im Arbeitsordner
    pub unstaged: Option<Change>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub branch: String,
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub files: Vec<FileStatus>,
    /// Noch kein Commit
    pub fresh: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Commit {
    pub hash: String,
    pub author: String,
    pub when: String,
    pub subject: String,
}

/// git-Programm: eigenes oder vom System
pub fn git_exe() -> Option<PathBuf> {
    crate::dev::tools().git.as_ref().map(|g| g.launcher.clone())
}

/// Einstellungen fuer Git (aus settings.rs uebernommen)
#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// Name und E-Mail fuer Commits (leer = aus der Git-Konfiguration bzw. von GitHub)
    pub user_name: String,
    pub user_email: String,
    /// Pull: "ff-only", "rebase" oder "merge"
    pub pull_mode: String,
    /// Nach dem Commit gleich pushen
    pub push_after_commit: bool,
    /// Alle Aenderungen vormerken, wenn beim Commit nichts vorgemerkt ist
    pub auto_stage: bool,
    /// Stand alle so viele Sekunden im Hintergrund abrufen (0 = aus)
    pub auto_fetch_secs: u32,
    /// Standardname des ersten Branches
    pub default_branch: String,
    /// Neue GitHub-Repositorys privat anlegen
    pub private_repos: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config { user_name: String::new(), user_email: String::new(), pull_mode: "ff-only".into(), push_after_commit: false, auto_stage: true, auto_fetch_secs: 0, default_branch: "main".into(), private_repos: true }
    }
}

static CONFIG: std::sync::RwLock<Option<Config>> = std::sync::RwLock::new(None);

pub fn config() -> Config {
    CONFIG.read().unwrap().clone().unwrap_or_default()
}

pub fn set_config(c: Config) {
    *CONFIG.write().unwrap() = Some(c);
}

/// Git-Befehl im Ordner `dir` vorbereiten: ohne Rueckfragen im Terminal, mit Name/E-Mail aus den Einstellungen
fn cmd(dir: &Path) -> Result<Command, String> {
    let exe = git_exe().ok_or_else(|| crate::i18n::tr("Git ist nicht installiert.", "Git is not installed.").to_string())?;
    let mut c = Command::new(exe);
    c.current_dir(dir).env("GIT_TERMINAL_PROMPT", "0").env("GIT_OPTIONAL_LOCKS", "0").arg("-c").arg("core.quotepath=false").arg("-c").arg("color.ui=false");
    let cfg = config();
    if !cfg.user_name.trim().is_empty() {
        c.arg("-c").arg(format!("user.name={}", cfg.user_name.trim()));
    }
    if !cfg.user_email.trim().is_empty() {
        c.arg("-c").arg(format!("user.email={}", cfg.user_email.trim()));
    }
    if let Some(p) = crate::dev::process::tool_path(&crate::dev::tools()) {
        c.env("PATH", p);
    }
    crate::server::hidden(&mut c);
    Ok(c)
}

/// Fuer GitHub-Adressen: das Token als Kopfzeile mitgeben (nur fuer diesen Aufruf)
fn with_auth(c: &mut Command) {
    if let Some(token) = github::token() {
        let b64 = base64(format!("x-access-token:{token}").as_bytes());
        c.arg("-c").arg(format!("http.https://github.com/.extraheader=AUTHORIZATION: basic {b64}"));
    }
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Ausfuehren und stdout liefern; bei Fehler die Meldung von git
fn run(mut c: Command) -> Result<String, String> {
    let out = c.stdin(Stdio::null()).output().map_err(|e| format!("git: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Ok(stdout)
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { stdout.trim().to_string() } else { err })
    }
}

fn git<I, S>(dir: &Path, args: I) -> Result<String, String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<std::ffi::OsStr>,
{
    let mut c = cmd(dir)?;
    c.args(args);
    run(c)
}

/// Wurzel des Repositorys, zu dem `dir` gehoert
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    let mut d = Some(dir);
    while let Some(x) = d {
        if x.join(".git").exists() {
            return Some(x.to_path_buf());
        }
        d = x.parent();
    }
    None
}

/// `git status --porcelain=v1 -b -z` lesen
pub fn parse_status(out: &str) -> Status {
    let mut st = Status::default();
    let mut parts = out.split('\0').peekable();
    while let Some(p) = parts.next() {
        if p.is_empty() {
            continue;
        }
        if let Some(b) = p.strip_prefix("## ") {
            // "main...origin/main [ahead 1, behind 2]", "No commits yet on main", "HEAD (no branch)"
            let (head, track) = b.split_once(" [").map(|(h, t)| (h, Some(t.trim_end_matches(']')))).unwrap_or((b, None));
            if let Some(n) = head.strip_prefix("No commits yet on ").or_else(|| head.strip_prefix("Initial commit on ")) {
                st.fresh = true;
                st.branch = n.to_string();
            } else if let Some((local, up)) = head.split_once("...") {
                st.branch = local.to_string();
                st.upstream = Some(up.to_string());
            } else {
                st.branch = head.to_string();
            }
            for t in track.unwrap_or("").split(", ") {
                if let Some(n) = t.strip_prefix("ahead ") {
                    st.ahead = n.parse().unwrap_or(0);
                } else if let Some(n) = t.strip_prefix("behind ") {
                    st.behind = n.parse().unwrap_or(0);
                }
            }
            continue;
        }
        if p.len() < 4 {
            continue;
        }
        let x = p.as_bytes()[0] as char;
        let y = p.as_bytes()[1] as char;
        let path = p[3..].to_string();
        // Umbenennungen: danach folgt der alte Name
        if x == 'R' || x == 'C' {
            parts.next();
        }
        let kind = |c: char| match c {
            'M' | 'T' => Some(Change::Modified),
            'A' => Some(Change::Added),
            'D' => Some(Change::Deleted),
            'R' | 'C' => Some(Change::Renamed),
            _ => None,
        };
        let conflict = matches!((x, y), ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D'));
        let f = if x == '?' {
            FileStatus { path, staged: None, unstaged: Some(Change::Untracked) }
        } else if conflict {
            FileStatus { path, staged: None, unstaged: Some(Change::Conflict) }
        } else {
            FileStatus { path, staged: kind(x), unstaged: kind(y) }
        };
        st.files.push(f);
    }
    st.files.sort_by_key(|f| f.path.to_lowercase());
    st
}

pub fn status(root: &Path) -> Result<Status, String> {
    git(root, ["status", "--porcelain=v1", "-b", "-z", "--untracked-files=all"]).map(|o| parse_status(&o))
}

pub fn init(dir: &Path) -> Result<(), String> {
    let branch = config().default_branch;
    // -b gibt es erst ab Git 2.28; sonst danach umbenennen
    if git(dir, ["init", "-b", branch.as_str()]).is_err() {
        git(dir, ["init"])?;
        let _ = git(dir, ["symbolic-ref", "HEAD", &format!("refs/heads/{branch}")]);
    }
    Ok(())
}

/// Hat Git Name und E-Mail fuer Commits?
pub fn has_identity(dir: &Path) -> bool {
    git(dir, ["config", "user.email"]).is_ok_and(|v| !v.trim().is_empty())
}

/// Neues Repository mit allen Dateien im ersten Commit (ohne Name/E-Mail nur vorgemerkt)
pub fn init_with_commit(dir: &Path, message: &str) -> Result<(), String> {
    init(dir)?;
    git(dir, ["add", "-A"])?;
    if has_identity(dir) {
        git(dir, ["commit", "-q", "-m", message])?;
    }
    Ok(())
}

pub fn stage(root: &Path, paths: &[String]) -> Result<(), String> {
    let mut args = vec!["add".to_string(), "--".into()];
    args.extend(paths.iter().cloned());
    git(root, args).map(|_| ())
}

pub fn stage_all(root: &Path) -> Result<(), String> {
    git(root, ["add", "-A"]).map(|_| ())
}

pub fn unstage(root: &Path, paths: &[String], fresh: bool) -> Result<(), String> {
    // Vor dem ersten Commit gibt es kein HEAD: dann aus dem Index entfernen
    let mut args: Vec<String> = if fresh { vec!["rm".into(), "--cached".into(), "-q".into(), "--".into()] } else { vec!["restore".into(), "--staged".into(), "--".into()] };
    args.extend(paths.iter().cloned());
    git(root, args).map(|_| ())
}

/// Aenderungen einer Datei verwerfen (neue, nicht verfolgte Dateien werden geloescht)
pub fn discard(root: &Path, f: &FileStatus) -> Result<(), String> {
    if f.unstaged == Some(Change::Untracked) {
        return std::fs::remove_file(root.join(&f.path)).or_else(|_| std::fs::remove_dir_all(root.join(&f.path))).map_err(|e| e.to_string());
    }
    if f.staged.is_some() {
        git(root, ["restore", "--staged", "--worktree", "--source=HEAD", "--", &f.path]).map(|_| ())
    } else {
        git(root, ["restore", "--", &f.path]).map(|_| ())
    }
}

/// Unterschiede einer Datei (vorgemerkt bzw. im Arbeitsordner); neue Dateien ganz als hinzugefuegt
pub fn diff(root: &Path, f: &FileStatus, staged: bool) -> Result<String, String> {
    if !staged && f.unstaged == Some(Change::Untracked) {
        let text = std::fs::read_to_string(root.join(&f.path)).unwrap_or_default();
        let mut out = format!("--- /dev/null\n+++ b/{}\n@@ -0,0 +1,{} @@\n", f.path, text.lines().count());
        for l in text.lines() {
            out.push('+');
            out.push_str(l);
            out.push('\n');
        }
        return Ok(out);
    }
    let mut args = vec!["diff", "--no-ext-diff", "-U3"];
    if staged {
        args.push("--cached");
    }
    args.extend(["--", f.path.as_str()]);
    git(root, args)
}

pub fn commit(root: &Path, message: &str, amend: bool) -> Result<String, String> {
    if message.trim().is_empty() && !amend {
        return Err(crate::i18n::tr("Bitte eine Beschreibung eingeben.", "Please enter a message.").into());
    }
    let mut args = vec!["commit".to_string(), "-q".into()];
    if amend {
        args.push("--amend".into());
        if message.trim().is_empty() {
            args.push("--no-edit".into());
        }
    }
    if !message.trim().is_empty() {
        args.extend(["-m".into(), message.trim().to_string()]);
    }
    git(root, args).map_err(|e| {
        if e.contains("user.email") || e.contains("Please tell me who you are") {
            crate::i18n::tr("Git kennt Ihren Namen und Ihre E-Mail noch nicht. Bitte in den Einstellungen unter Git eintragen (oder bei GitHub anmelden).", "Git does not know your name and e-mail yet. Please enter them in the settings under Git (or sign in to GitHub).").to_string()
        } else {
            e
        }
    })?;
    git(root, ["log", "-1", "--pretty=%h %s"]).map(|s| s.trim().to_string())
}

pub fn branches(root: &Path) -> Result<(Vec<String>, Vec<String>), String> {
    let local = git(root, ["branch", "--format=%(refname:short)"])?.lines().map(str::to_string).filter(|l| !l.is_empty()).collect();
    let remote = git(root, ["branch", "-r", "--format=%(refname:short)"]).unwrap_or_default().lines().map(str::to_string).filter(|l| !l.is_empty() && !l.ends_with("/HEAD")).collect();
    Ok((local, remote))
}

pub fn switch(root: &Path, branch: &str, create: bool) -> Result<(), String> {
    if create {
        git(root, ["switch", "-c", branch]).map(|_| ())
    } else if let Some((_, local)) = branch.split_once('/').filter(|_| !branch.starts_with("refs/")) {
        // Entfernter Branch: lokale Kopie anlegen, die ihm folgt
        git(root, ["switch", local]).or_else(|_| git(root, ["switch", "--track", branch])).map(|_| ())
    } else {
        git(root, ["switch", branch]).map(|_| ())
    }
}

pub fn delete_branch(root: &Path, branch: &str, force: bool) -> Result<(), String> {
    git(root, ["branch", if force { "-D" } else { "-d" }, branch]).map(|_| ())
}

pub fn merge(root: &Path, branch: &str) -> Result<String, String> {
    git(root, ["merge", "--no-edit", branch])
}

pub fn log(root: &Path, n: usize) -> Result<Vec<Commit>, String> {
    let out = git(root, ["log", &format!("-n{n}"), "--pretty=format:%h%x1f%an%x1f%ar%x1f%s"])?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let mut it = l.split('\u{1f}');
            Some(Commit { hash: it.next()?.into(), author: it.next()?.into(), when: it.next()?.into(), subject: it.next().unwrap_or("").into() })
        })
        .collect())
}

/// Aenderungen eines Commits (fuer die Anzeige)
pub fn show(root: &Path, hash: &str) -> Result<String, String> {
    git(root, ["show", "--no-ext-diff", "--stat", "-p", hash])
}

pub fn remote_url(root: &Path) -> Option<String> {
    git(root, ["remote", "get-url", "origin"]).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub fn set_remote(root: &Path, url: &str) -> Result<(), String> {
    if remote_url(root).is_some() { git(root, ["remote", "set-url", "origin", url]) } else { git(root, ["remote", "add", "origin", url]) }.map(|_| ())
}

pub fn fetch(root: &Path) -> Result<(), String> {
    let mut c = cmd(root)?;
    with_auth(&mut c);
    c.args(["fetch", "--prune", "-q"]);
    run(c).map(|_| ())
}

pub fn pull(root: &Path) -> Result<String, String> {
    let mut c = cmd(root)?;
    with_auth(&mut c);
    c.arg("pull");
    match config().pull_mode.as_str() {
        "rebase" => c.arg("--rebase"),
        "merge" => c.arg("--no-rebase"),
        _ => c.arg("--ff-only"),
    };
    run(c)
}

/// Push; ohne Upstream mit -u origin <branch>
pub fn push(root: &Path, st: &Status, force: bool) -> Result<String, String> {
    let mut c = cmd(root)?;
    with_auth(&mut c);
    c.arg("push");
    if force {
        c.arg("--force-with-lease");
    }
    if st.upstream.is_none() {
        c.args(["-u", "origin", st.branch.as_str()]);
    }
    let out = c.stdin(Stdio::null()).output().map_err(|e| format!("git: {e}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if out.status.success() { Ok(text) } else { Err(text.trim().to_string()) }
}

pub fn clone(url: &str, into: &Path) -> Result<(), String> {
    let parent = into.parent().ok_or("clone")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut c = cmd(parent)?;
    with_auth(&mut c);
    c.arg("clone").arg(url).arg(into);
    run(c).map(|_| ())
}

/// "owner/repo" aus einer GitHub-Adresse
pub fn github_repo(url: &str) -> Option<(String, String)> {
    let rest = url
        .strip_prefix("https://github.com/")
        .or_else(|| url.strip_prefix("http://github.com/"))
        .or_else(|| url.strip_prefix("git@github.com:"))
        .or_else(|| url.strip_prefix("ssh://git@github.com/"))?;
    let rest = rest.trim_end_matches('/').trim_end_matches(".git");
    let (o, r) = rest.split_once('/')?;
    (!o.is_empty() && !r.is_empty() && !r.contains('/')).then(|| (o.to_string(), r.to_string()))
}

/// Ordnername fuer eine geklonte Adresse
pub fn clone_dir_name(url: &str) -> String {
    url.trim_end_matches('/').trim_end_matches(".git").rsplit(['/', ':']).next().unwrap_or("repo").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn porcelain_status_is_parsed() {
        let out = "## main...origin/main [ahead 2, behind 1]\0 M src/a.rs\0M  b.txt\0R  neu.txt\0alt.txt\0?? x y.md\0UU k.txt\0AM c.txt\0";
        let st = parse_status(out);
        assert_eq!((st.branch.as_str(), st.upstream.as_deref(), st.ahead, st.behind), ("main", Some("origin/main"), 2, 1));
        let get = |p: &str| st.files.iter().find(|f| f.path == p).cloned().unwrap();
        assert_eq!(get("src/a.rs").unstaged, Some(Change::Modified));
        assert_eq!(get("b.txt").staged, Some(Change::Modified));
        assert_eq!(get("neu.txt").staged, Some(Change::Renamed));
        assert!(!st.files.iter().any(|f| f.path == "alt.txt"));
        assert_eq!(get("x y.md").unstaged, Some(Change::Untracked));
        assert_eq!(get("k.txt").unstaged, Some(Change::Conflict));
        assert_eq!((get("c.txt").staged, get("c.txt").unstaged), (Some(Change::Added), Some(Change::Modified)));
        let fresh = parse_status("## No commits yet on main\0");
        assert!(fresh.fresh && fresh.branch == "main");
    }

    #[test]
    fn github_addresses() {
        assert_eq!(github_repo("https://github.com/gravijet/EasyMySQL.git"), Some(("gravijet".into(), "EasyMySQL".into())));
        assert_eq!(github_repo("git@github.com:a/b.git"), Some(("a".into(), "b".into())));
        assert_eq!(github_repo("https://gitlab.com/a/b"), None);
        assert_eq!(clone_dir_name("https://github.com/a/mein-repo.git"), "mein-repo");
        assert_eq!(base64(b"x-access-token:abc"), "eC1hY2Nlc3MtdG9rZW46YWJj");
    }

    /// Mit dem git des Systems: anlegen, aendern, vormerken, Commit, Branch
    #[test]
    fn local_repository_workflow() {
        if crate::dev::which("git").is_none() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("easymysql-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        set_config(Config { user_name: "Test".into(), user_email: "test@example.org".into(), ..Default::default() });
        init(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "eins\n").unwrap();
        let st = status(&dir).unwrap();
        assert!(st.fresh && st.files[0].unstaged == Some(Change::Untracked));
        stage(&dir, &["a.txt".into()]).unwrap();
        assert_eq!(status(&dir).unwrap().files[0].staged, Some(Change::Added));
        unstage(&dir, &["a.txt".into()], true).unwrap();
        stage_all(&dir).unwrap();
        let c = commit(&dir, "Erster Stand", false).unwrap();
        assert!(c.ends_with("Erster Stand"), "{c}");
        std::fs::write(dir.join("a.txt"), "zwei\n").unwrap();
        let st = status(&dir).unwrap();
        let d = diff(&dir, &st.files[0], false).unwrap();
        assert!(d.contains("-eins") && d.contains("+zwei"), "{d}");
        discard(&dir, &st.files[0]).unwrap();
        assert!(status(&dir).unwrap().files.is_empty());
        switch(&dir, "feature", true).unwrap();
        let (local, _) = branches(&dir).unwrap();
        assert!(local.contains(&"feature".to_string()) && local.contains(&"main".to_string()), "{local:?}");
        assert_eq!(log(&dir, 5).unwrap()[0].subject, "Erster Stand");
        set_config(Config::default());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
