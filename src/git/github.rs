// GitHub: Anmeldung (persoenliches Token oder GitHub CLI), eigene Repositorys, neues Repository anlegen,
// Pull Requests, Issues und Actions-Laeufe. Das Token liegt in einer eigenen Datei im Programmdatenordner
// (unter Linux/macOS nur fuer den Benutzer lesbar), nicht in einstellungen.txt.

use serde_json::{Value, json};
use std::path::PathBuf;
use std::sync::RwLock;
use std::time::Duration;

const API: &str = "https://api.github.com";

/// Seite zum Anlegen eines Tokens mit den noetigen Rechten
pub const TOKEN_URL: &str = "https://github.com/settings/tokens/new?scopes=repo,workflow,read:user,user:email&description=EasyMySQL";

fn token_file() -> PathBuf {
    crate::server::app_data_dir().join("github-token")
}

static TOKEN: RwLock<Option<Option<String>>> = RwLock::new(None);

/// Gespeichertes Token, sonst das der GitHub CLI (falls dort angemeldet)
pub fn token() -> Option<String> {
    if let Some(t) = TOKEN.read().unwrap().clone() {
        return t;
    }
    let t = std::fs::read_to_string(token_file()).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).or_else(gh_token);
    *TOKEN.write().unwrap() = Some(t.clone());
    t
}

/// Token der GitHub CLI (`gh auth token`)
fn gh_token() -> Option<String> {
    let gh = crate::dev::tools().gh.clone()?;
    crate::dev::probe(&gh.launcher, &["auth", "token", "-h", "github.com"]).map(|s| s.trim().to_string()).filter(|s| s.starts_with("gh") || s.len() > 20)
}

/// Neues Token speichern (leer = abmelden)
pub fn set_token(t: &str) -> Result<(), String> {
    let t = t.trim();
    let f = token_file();
    if t.is_empty() {
        let _ = std::fs::remove_file(&f);
    } else {
        crate::workspace::atomic_write_private(&f, t).map_err(|e| e.to_string())?;
    }
    *TOKEN.write().unwrap() = None;
    Ok(())
}

/// Nach einer Anmeldung ueber die GitHub CLI neu nachsehen
pub fn reload() {
    *TOKEN.write().unwrap() = None;
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .https_only(true)
        .http_status_as_error(false)
        .user_agent(concat!("EasyMySQL/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn check(status: u16, body: &str) -> Result<Value, String> {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    if (200..300).contains(&status) {
        return Ok(v);
    }
    let msg = v["message"].as_str().unwrap_or("").to_string();
    let detail = v["errors"].as_array().and_then(|a| a.first()).and_then(|e| e["message"].as_str().map(str::to_string)).unwrap_or_default();
    Err(match status {
        401 => crate::i18n::tr("GitHub lehnt die Anmeldung ab (Token ungültig oder abgelaufen).", "GitHub rejected the sign-in (token invalid or expired).").to_string(),
        403 if msg.contains("rate limit") => crate::i18n::tr("GitHub: zu viele Anfragen, bitte später erneut versuchen.", "GitHub: too many requests, please try again later.").to_string(),
        _ => format!("GitHub {status}: {msg}{}", if detail.is_empty() { String::new() } else { format!(" ({detail})") }),
    })
}

fn get(path: &str) -> Result<Value, String> {
    let token = token().ok_or_else(|| crate::i18n::tr("Nicht bei GitHub angemeldet.", "Not signed in to GitHub.").to_string())?;
    let mut r = agent()
        .get(&format!("{API}{path}"))
        .header("Authorization", &format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .call()
        .map_err(|e| format!("GitHub: {e}"))?;
    let status = r.status().as_u16();
    let body = r.body_mut().with_config().limit(16 * 1024 * 1024).read_to_string().map_err(|e| e.to_string())?;
    check(status, &body)
}

fn send(method: &str, path: &str, body: Value) -> Result<Value, String> {
    let token = token().ok_or_else(|| crate::i18n::tr("Nicht bei GitHub angemeldet.", "Not signed in to GitHub.").to_string())?;
    let a = agent();
    let url = format!("{API}{path}");
    let text = body.to_string();
    let req = match method {
        "PUT" => a.put(&url),
        "PATCH" => a.patch(&url),
        _ => a.post(&url),
    };
    let mut r = req
        .header("Authorization", &format!("Bearer {token}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("Content-Type", "application/json")
        .send(text.as_bytes())
        .map_err(|e| format!("GitHub: {e}"))?;
    let status = r.status().as_u16();
    let body = r.body_mut().with_config().limit(16 * 1024 * 1024).read_to_string().map_err(|e| e.to_string())?;
    check(status, &body)
}

#[derive(Clone, Debug, PartialEq)]
pub struct User {
    pub login: String,
    pub name: String,
    pub email: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Repo {
    pub full_name: String,
    pub clone_url: String,
    pub html_url: String,
    pub private: bool,
    pub description: String,
    pub default_branch: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub number: u64,
    pub title: String,
    pub user: String,
    pub html_url: String,
    /// Pull Request: Ziel- und Quell-Branch; Issue: leer
    pub base: String,
    pub head: String,
    pub draft: bool,
    pub comments: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub name: String,
    pub branch: String,
    /// queued, in_progress, completed
    pub status: String,
    /// success, failure, cancelled, … (wenn fertig)
    pub conclusion: String,
    pub html_url: String,
    pub when: String,
}

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or("").to_string()
}

pub fn user() -> Result<User, String> {
    let v = get("/user")?;
    let mut email = s(&v, "email");
    if email.is_empty()
        && let Ok(list) = get("/user/emails")
    {
        email = list.as_array().and_then(|a| a.iter().find(|e| e["primary"].as_bool() == Some(true))).map(|e| s(e, "email")).unwrap_or_default();
    }
    if email.is_empty() {
        // Adresse von GitHub, die die eigene verbirgt
        email = format!("{}+{}@users.noreply.github.com", v["id"].as_u64().unwrap_or(0), s(&v, "login"));
    }
    Ok(User { login: s(&v, "login"), name: v["name"].as_str().map(str::to_string).unwrap_or_else(|| s(&v, "login")), email })
}

fn repo_of(v: &Value) -> Repo {
    Repo { full_name: s(v, "full_name"), clone_url: s(v, "clone_url"), html_url: s(v, "html_url"), private: v["private"].as_bool().unwrap_or(false), description: s(v, "description"), default_branch: s(v, "default_branch") }
}

pub fn repos() -> Result<Vec<Repo>, String> {
    let v = get("/user/repos?per_page=100&sort=updated&affiliation=owner,collaborator,organization_member")?;
    Ok(v.as_array().map(|a| a.iter().map(repo_of).collect()).unwrap_or_default())
}

pub fn repo(owner: &str, name: &str) -> Result<Repo, String> {
    get(&format!("/repos/{owner}/{name}")).map(|v| repo_of(&v))
}

pub fn create_repo(name: &str, private: bool, description: &str) -> Result<Repo, String> {
    send("POST", "/user/repos", json!({ "name": name, "private": private, "description": description, "auto_init": false })).map(|v| repo_of(&v))
}

fn item_of(v: &Value) -> Item {
    Item {
        number: v["number"].as_u64().unwrap_or(0),
        title: s(v, "title"),
        user: s(&v["user"], "login"),
        html_url: s(v, "html_url"),
        base: s(&v["base"], "ref"),
        head: s(&v["head"], "ref"),
        draft: v["draft"].as_bool().unwrap_or(false),
        comments: v["comments"].as_u64().unwrap_or(0),
    }
}

pub fn pulls(owner: &str, name: &str) -> Result<Vec<Item>, String> {
    let v = get(&format!("/repos/{owner}/{name}/pulls?state=open&per_page=50"))?;
    Ok(v.as_array().map(|a| a.iter().map(item_of).collect()).unwrap_or_default())
}

pub fn issues(owner: &str, name: &str) -> Result<Vec<Item>, String> {
    let v = get(&format!("/repos/{owner}/{name}/issues?state=open&per_page=50"))?;
    // Die Issues-Liste enthaelt auch Pull Requests
    Ok(v.as_array().map(|a| a.iter().filter(|i| i.get("pull_request").is_none()).map(item_of).collect()).unwrap_or_default())
}

pub fn runs(owner: &str, name: &str) -> Result<Vec<Run>, String> {
    let v = get(&format!("/repos/{owner}/{name}/actions/runs?per_page=10"))?;
    Ok(v["workflow_runs"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|r| Run { name: s(r, "name"), branch: s(r, "head_branch"), status: s(r, "status"), conclusion: s(r, "conclusion"), html_url: s(r, "html_url"), when: s(r, "created_at").replace('T', " ").trim_end_matches('Z').to_string() })
                .collect()
        })
        .unwrap_or_default())
}

pub fn create_pull(owner: &str, name: &str, title: &str, head: &str, base: &str, body: &str, draft: bool) -> Result<Item, String> {
    send("POST", &format!("/repos/{owner}/{name}/pulls"), json!({ "title": title, "head": head, "base": base, "body": body, "draft": draft })).map(|v| item_of(&v))
}

/// Pull Request zusammenfuehren: "merge", "squash" oder "rebase"
pub fn merge_pull(owner: &str, name: &str, number: u64, method: &str) -> Result<(), String> {
    send("PUT", &format!("/repos/{owner}/{name}/pulls/{number}/merge"), json!({ "merge_method": method })).map(|_| ())
}

pub fn create_issue(owner: &str, name: &str, title: &str, body: &str) -> Result<Item, String> {
    send("POST", &format!("/repos/{owner}/{name}/issues"), json!({ "title": title, "body": body })).map(|v| item_of(&v))
}

/// Webseite im Browser oeffnen
pub fn open_url(url: &str) {
    if !url.starts_with("https://") {
        return;
    }
    #[cfg(windows)]
    let _ = std::process::Command::new("explorer").arg(url).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_errors_are_readable() {
        assert!(check(200, "{\"a\":1}").is_ok());
        let e = check(422, r#"{"message":"Validation Failed","errors":[{"message":"name already exists on this account"}]}"#).unwrap_err();
        assert!(e.contains("Validation Failed") && e.contains("already exists"), "{e}");
        assert!(check(401, "{}").unwrap_err().contains("GitHub"));
        let pr = item_of(&json!({ "number": 7, "title": "x", "user": { "login": "a" }, "base": { "ref": "main" }, "head": { "ref": "f" }, "html_url": "https://github.com/o/r/pull/7" }));
        assert_eq!((pr.number, pr.base.as_str(), pr.head.as_str()), (7, "main", "f"));
    }
}
