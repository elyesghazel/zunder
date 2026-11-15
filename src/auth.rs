//! GitHub login through the OAuth device flow, for sync (gists).
//!
//! `zunder login` asks GitHub for a device code, prints it with the URL, opens the URL and polls
//! until the code was confirmed on github.com. The token goes to the system keyring
//! (secret-service through oo7) on Linux, else to `<data dir>/zunder/token` with mode 0600.
//! `ZUNDER_GITHUB_TOKEN` wins over both, for CI and tests.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Deserialize;

/// The OAuth app's client id (public, Device Flow enabled).
/// `ZUNDER_GITHUB_CLIENT_ID` overrides it.
const GITHUB_CLIENT_ID: &str = "Ov23liFCsHy34iE7kQF5";
const SCOPE: &str = "gist";
const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const USER_URL: &str = "https://api.github.com/user";
const GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:device_code";

/// Where the token came from, for `--status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Env,
    Keyring,
    File,
}

impl Source {
    fn label(self) -> &'static str {
        match self {
            Source::Env => "ZUNDER_GITHUB_TOKEN",
            Source::Keyring => "system keyring",
            Source::File => "token file",
        }
    }
}

/// The keyring is a D-Bus round trip, so a lookup is kept for a minute. Not longer: the server
/// runs for days and has to notice a `zunder login` / `logout` from another process.
type Cached = Option<(Instant, Option<(String, Source)>)>;
static CACHE: Mutex<Cached> = Mutex::new(None);
const CACHE_FOR: Duration = Duration::from_secs(60);

/// The GitHub token: `ZUNDER_GITHUB_TOKEN`, then the keyring, then the token file.
#[allow(dead_code)] // sync (#15) is the first caller
pub fn token() -> Option<String> {
    lookup().map(|(t, _)| t)
}

/// The GitHub login name, cached at login.
pub fn user() -> Option<String> {
    let name = fs::read_to_string(user_path()).ok()?;
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

fn lookup() -> Option<(String, Source)> {
    if let Some(t) = env_token(std::env::var("ZUNDER_GITHUB_TOKEN").ok()) {
        return Some((t, Source::Env));
    }
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, found)) = &*cache
        && at.elapsed() < CACHE_FOR
    {
        return found.clone();
    }
    let found = keyring::get()
        .map(|t| (t, Source::Keyring))
        .or_else(|| read_token_file(&token_path()).map(|t| (t, Source::File)));
    *cache = Some((Instant::now(), found.clone()));
    found
}

fn forget_cache() {
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

fn env_token(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

fn client_id() -> String {
    pick_client_id(std::env::var("ZUNDER_GITHUB_CLIENT_ID").ok())
}

fn pick_client_id(env: Option<String>) -> String {
    env.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty()).unwrap_or_else(|| GITHUB_CLIENT_ID.to_owned())
}

fn token_path() -> PathBuf {
    crate::store::data_dir().join("token")
}

fn user_path() -> PathBuf {
    crate::store::data_dir().join("github-user")
}

// ── commands ────────────────────────────────────────────

/// `zunder login [--status]`.
pub fn login(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        None => device_login(),
        Some("--status" | "status") => {
            println!("{}", status());
            Ok(())
        }
        Some(other) => Err(format!("unknown option `{other}`; usage: zunder login [--status]")),
    }
}

/// `zunder logout`: removes the token from the keyring and the file.
pub fn logout() -> Result<(), String> {
    let had_keyring = keyring::delete();
    let path = token_path();
    let had_file = path.exists();
    if had_file {
        fs::remove_file(&path).map_err(|e| format!("could not remove {}: {e}", path.display()))?;
    }
    let _ = fs::remove_file(user_path());
    forget_cache();
    if had_keyring || had_file {
        println!("logged out of GitHub");
    } else {
        println!("not logged in");
    }
    if env_token(std::env::var("ZUNDER_GITHUB_TOKEN").ok()).is_some() {
        println!("note: ZUNDER_GITHUB_TOKEN is still set and keeps being used");
    }
    Ok(())
}

fn status() -> String {
    match lookup() {
        Some((_, source)) => match user() {
            Some(name) => format!("logged in to GitHub as {name} (token from {})", source.label()),
            None => format!("logged in to GitHub (token from {})", source.label()),
        },
        None => "not logged in; run `zunder login`".into(),
    }
}

fn device_login() -> Result<(), String> {
    let client_id = client_id();
    let code = request_device_code(&client_id)?;
    println!("Open {} and enter the code:\n\n    {}\n", code.verification_uri, code.user_code);
    crate::platform::open(&code.verification_uri);
    println!("waiting for the confirmation on github.com…");

    let token = poll_token(&client_id, &code)?;
    let login = fetch_login(&token);
    save(&token)?;
    if let Some(name) = &login {
        let path = user_path();
        let _ = fs::create_dir_all(path.parent().unwrap_or(Path::new(".")));
        let _ = fs::write(&path, name);
    }
    forget_cache();
    match login {
        Some(name) => println!("logged in to GitHub as {name}"),
        None => println!("logged in to GitHub"),
    }
    Ok(())
}

/// Keyring first; the file when there is no secret-service.
fn save(token: &str) -> Result<(), String> {
    if keyring::set(token) {
        // a stale file from an earlier keyring-less login would shadow nothing, but drop it
        let _ = fs::remove_file(token_path());
        return Ok(());
    }
    write_token_file(&token_path(), token).map_err(|e| format!("could not save the token: {e}"))
}

// ── device flow ─────────────────────────────────────────

#[derive(Debug, Deserialize, PartialEq)]
struct DeviceCode {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    #[serde(default = "default_interval")]
    interval: u64,
}

fn default_interval() -> u64 {
    5
}

#[derive(Debug, Deserialize)]
struct OauthError {
    error: String,
    #[serde(default)]
    error_description: Option<String>,
}

fn parse_device_code(body: &[u8]) -> Result<DeviceCode, String> {
    if let Ok(code) = serde_json::from_slice::<DeviceCode>(body) {
        return Ok(code);
    }
    match serde_json::from_slice::<OauthError>(body) {
        Ok(e) => Err(describe(&e)),
        Err(_) => Err("GitHub sent an unexpected answer to the device code request".into()),
    }
}

#[derive(Debug, PartialEq)]
enum Poll {
    Token(String),
    Pending,
    /// slow down: wait this many seconds between polls from now on
    SlowDown(u64),
    Failed(String),
}

fn parse_poll(body: &[u8], interval: u64) -> Poll {
    #[derive(Deserialize)]
    struct Answer {
        access_token: Option<String>,
        error: Option<String>,
        error_description: Option<String>,
        interval: Option<u64>,
    }
    let Ok(a) = serde_json::from_slice::<Answer>(body) else {
        return Poll::Failed("GitHub sent an unexpected answer while waiting".into());
    };
    if let Some(t) = a.access_token.filter(|t| !t.is_empty()) {
        return Poll::Token(t);
    }
    match a.error.as_deref() {
        Some("authorization_pending") => Poll::Pending,
        Some("slow_down") => Poll::SlowDown(a.interval.unwrap_or(interval + 5)),
        Some(error) => Poll::Failed(describe(&OauthError { error: error.to_owned(), error_description: a.error_description })),
        None => Poll::Failed("GitHub sent neither a token nor an error".into()),
    }
}

fn describe(e: &OauthError) -> String {
    match e.error.as_str() {
        "expired_token" => "the code expired; run `zunder login` again".into(),
        "access_denied" => "the login was cancelled on github.com".into(),
        "device_flow_disabled" => "Device Flow is not enabled for this OAuth app".into(),
        // an unknown client id gets a bare `{"error":"Not Found"}`
        "incorrect_client_credentials" | "Not Found" => "GitHub does not know this client id".into(),
        _ => match &e.error_description {
            Some(d) => format!("GitHub: {d} ({})", e.error),
            None => format!("GitHub: {}", e.error),
        },
    }
}

fn request_device_code(client_id: &str) -> Result<DeviceCode, String> {
    let body = post_form(DEVICE_CODE_URL, &[("client_id", client_id), ("scope", SCOPE)])?;
    parse_device_code(&body)
}

fn poll_token(client_id: &str, code: &DeviceCode) -> Result<String, String> {
    let deadline = Instant::now() + Duration::from_secs(code.expires_in);
    let mut interval = code.interval.max(1);
    loop {
        std::thread::sleep(Duration::from_secs(interval));
        if Instant::now() > deadline {
            return Err("the code expired; run `zunder login` again".into());
        }
        let body = match post_form(
            TOKEN_URL,
            &[("client_id", client_id), ("device_code", &code.device_code), ("grant_type", GRANT_TYPE)],
        ) {
            Ok(b) => b,
            // a network hiccup while waiting is not the end of the login
            Err(_) => continue,
        };
        match parse_poll(&body, interval) {
            Poll::Token(t) => return Ok(t),
            Poll::Pending => {}
            Poll::SlowDown(secs) => interval = secs,
            Poll::Failed(e) => return Err(e),
        }
    }
}

/// POST a form and return the body. GitHub answers OAuth errors with 200 and JSON, so no `-f`.
fn post_form(url: &str, fields: &[(&str, &str)]) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new("curl");
    cmd.args(["-sSL", "-m", "15", "-X", "POST", "-H", "Accept: application/json"]);
    for (k, v) in fields {
        cmd.arg("--data-urlencode").arg(format!("{k}={v}"));
    }
    cmd.arg(url);
    let out = crate::platform::no_window(&mut cmd).output().map_err(|e| format!("could not run curl: {e}"))?;
    if !out.status.success() {
        return Err(format!("could not reach GitHub: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(out.stdout)
}

/// The login name for a token. The token goes to curl through a config on stdin, never argv.
fn fetch_login(token: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct User {
        login: String,
    }
    let mut cmd = Command::new("curl");
    cmd.args(["-fsSL", "-m", "15", "-K", "-", "-H", "Accept: application/vnd.github+json", USER_URL])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = crate::platform::no_window(&mut cmd).spawn().ok()?;
    child.stdin.take()?.write_all(format!("header = \"Authorization: Bearer {token}\"\n").as_bytes()).ok()?;
    let out = child.wait_with_output().ok()?;
    if !out.status.success() {
        return None;
    }
    serde_json::from_slice::<User>(&out.stdout).ok().map(|u| u.login)
}

// ── token file ──────────────────────────────────────────

fn read_token_file(path: &Path) -> Option<String> {
    let t = fs::read_to_string(path).ok()?;
    let t = t.trim();
    (!t.is_empty()).then(|| t.to_owned())
}

fn write_token_file(path: &Path, token: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    // an existing file keeps its old mode on open: tighten it
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        f.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    f.write_all(token.as_bytes())?;
    f.write_all(b"\n")
}

// ── keyring ─────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod keyring {
    use std::collections::HashMap;

    fn attributes() -> HashMap<&'static str, &'static str> {
        HashMap::from([("application", "zunder"), ("service", "github")])
    }

    fn run<T>(f: impl std::future::Future<Output = oo7::Result<T>>) -> Option<T> {
        futures_lite::future::block_on(f).ok()
    }

    pub fn get() -> Option<String> {
        // tests and CI never touch the real keyring
        if cfg!(test) {
            return None;
        }
        run(async {
            let keyring = oo7::Keyring::new().await?;
            let items = keyring.search_items(&attributes()).await?;
            let Some(item) = items.first() else { return Ok(None) };
            if item.is_locked().await? {
                item.unlock().await?;
            }
            let secret = item.secret().await?;
            Ok(String::from_utf8(secret.as_bytes().to_vec()).ok())
        })
        .flatten()
        .filter(|t| !t.is_empty())
    }

    pub fn set(token: &str) -> bool {
        if cfg!(test) {
            return false;
        }
        run(async {
            let keyring = oo7::Keyring::new().await?;
            keyring.unlock().await?;
            keyring.create_item("zunder GitHub token", &attributes(), token, true).await
        })
        .is_some()
    }

    /// True when there was an item to remove.
    pub fn delete() -> bool {
        if cfg!(test) {
            return false;
        }
        run(async {
            let keyring = oo7::Keyring::new().await?;
            let found = !keyring.search_items(&attributes()).await?.is_empty();
            if found {
                keyring.delete(&attributes()).await?;
            }
            Ok(found)
        })
        .unwrap_or(false)
    }
}

#[cfg(not(target_os = "linux"))]
mod keyring {
    pub fn get() -> Option<String> {
        None
    }
    pub fn set(_token: &str) -> bool {
        false
    }
    pub fn delete() -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_device_code() {
        let body = br#"{"device_code":"3584d83530557fdd1f46af8289938c8ef79f9dc5","user_code":"WDJB-MJHT","verification_uri":"https://github.com/login/device","expires_in":900,"interval":5}"#;
        let code = parse_device_code(body).unwrap();
        assert_eq!(code.user_code, "WDJB-MJHT");
        assert_eq!(code.verification_uri, "https://github.com/login/device");
        assert_eq!(code.expires_in, 900);
        assert_eq!(code.interval, 5);
        let no_interval = br#"{"device_code":"d","user_code":"u","verification_uri":"v","expires_in":900}"#;
        assert_eq!(parse_device_code(no_interval).unwrap().interval, 5);
    }

    #[test]
    fn device_code_errors_are_readable() {
        let body = br#"{"error":"device_flow_disabled","error_description":"Device Flow must be explicitly enabled for this App"}"#;
        assert_eq!(parse_device_code(body).unwrap_err(), "Device Flow is not enabled for this OAuth app");
        let body = br#"{"error":"unauthorized_client","error_description":"nope"}"#;
        assert_eq!(parse_device_code(body).unwrap_err(), "GitHub: nope (unauthorized_client)");
        assert_eq!(parse_device_code(br#"{"error":"Not Found"}"#).unwrap_err(), "GitHub does not know this client id");
        assert!(parse_device_code(b"<html>").is_err());
    }

    #[test]
    fn parses_poll_answers() {
        assert_eq!(parse_poll(br#"{"access_token":"gho_x","token_type":"bearer","scope":"gist"}"#, 5), Poll::Token("gho_x".into()));
        assert_eq!(parse_poll(br#"{"error":"authorization_pending"}"#, 5), Poll::Pending);
        assert_eq!(parse_poll(br#"{"error":"slow_down","interval":10}"#, 5), Poll::SlowDown(10));
        assert_eq!(parse_poll(br#"{"error":"slow_down"}"#, 5), Poll::SlowDown(10));
        assert_eq!(parse_poll(br#"{"error":"expired_token"}"#, 5), Poll::Failed("the code expired; run `zunder login` again".into()));
        assert_eq!(parse_poll(br#"{"error":"access_denied"}"#, 5), Poll::Failed("the login was cancelled on github.com".into()));
        assert!(matches!(parse_poll(b"garbage", 5), Poll::Failed(_)));
        assert!(matches!(parse_poll(b"{}", 5), Poll::Failed(_)));
    }

    #[test]
    fn env_token_wins_when_set() {
        assert_eq!(env_token(Some(" abc \n".into())), Some("abc".into()));
        assert_eq!(env_token(Some("  ".into())), None);
        assert_eq!(env_token(None), None);
    }

    #[test]
    fn client_id_env_overrides_the_built_in_one() {
        assert_eq!(pick_client_id(None), GITHUB_CLIENT_ID);
        assert_eq!(pick_client_id(Some(" ".into())), GITHUB_CLIENT_ID);
        assert_eq!(pick_client_id(Some("Ov23liExample".into())), "Ov23liExample");
    }

    #[test]
    fn token_file_roundtrip() {
        let dir = std::env::temp_dir().join(format!("zunder-auth-{}", std::process::id()));
        let path = dir.join("sub").join("token");
        assert_eq!(read_token_file(&path), None);
        write_token_file(&path, "test-token-1").unwrap();
        assert_eq!(read_token_file(&path), Some("test-token-1".into()));
        write_token_file(&path, "test-token-2").unwrap();
        assert_eq!(read_token_file(&path), Some("test-token-2".into()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
            write_token_file(&path, "test-token-3").unwrap();
            assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
        }
        fs::write(&path, "  \n").unwrap();
        assert_eq!(read_token_file(&path), None);
        fs::remove_dir_all(&dir).unwrap();
    }
}
