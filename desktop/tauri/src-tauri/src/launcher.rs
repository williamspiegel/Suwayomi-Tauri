use once_cell::sync::Lazy;
use regex::Regex;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

const DEFAULT_IP: &str = "127.0.0.1";
const DEFAULT_PORT: u16 = 4567;
const HEALTH_ENDPOINT: &str = "/api/v1/settings/about/";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(10);
const POLL_INTERVAL: Duration = Duration::from_millis(300);

static CHILD_PROCESS: Lazy<Mutex<Option<Child>>> = Lazy::new(|| Mutex::new(None));

#[derive(Debug, Error)]
pub enum LauncherError {
    #[error("could not determine launcher executable path")]
    MissingExecutable,
    #[error("required file is missing: {0}")]
    MissingFile(String),
    #[error("failed to start server process: {0}")]
    SpawnServer(String),
    #[error("server did not become healthy at {base_url} within {timeout_secs} seconds")]
    StartupTimeout { base_url: String, timeout_secs: u64 },
    #[error("invalid base url: {0}")]
    InvalidBaseUrl(String),
}

#[derive(Debug, Clone)]
pub struct LauncherBootstrap {
    pub base_url: String,
}

#[derive(Debug, Clone)]
struct ParsedConfig {
    ip: String,
    port: u16,
    subpath: String,
}

impl Default for ParsedConfig {
    fn default() -> Self {
        Self {
            ip: DEFAULT_IP.to_string(),
            port: DEFAULT_PORT,
            subpath: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct LauncherConfig {
    runtime_root: PathBuf,
    java_bin: PathBuf,
    jar_file: PathBuf,
    base_url: String,
    root_dir: Option<String>,
}

pub fn bootstrap(resource_dir: Option<PathBuf>) -> Result<LauncherBootstrap, LauncherError> {
    let base_url = resolve_base_url();
    if url::Url::parse(&base_url).is_err() {
        return Err(LauncherError::InvalidBaseUrl(base_url.clone()));
    }

    if is_server_healthy(&base_url) {
        return Ok(LauncherBootstrap { base_url });
    }

    let config = LauncherConfig::discover(base_url, resource_dir)?;

    if !is_server_healthy(&config.base_url) {
        let mut child = spawn_server(&config)?;

        if !wait_for_server(&config.base_url, STARTUP_TIMEOUT) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(LauncherError::StartupTimeout {
                base_url: config.base_url,
                timeout_secs: STARTUP_TIMEOUT.as_secs(),
            });
        }

        *CHILD_PROCESS.lock().expect("child process mutex poisoned") = Some(child);
    }

    Ok(LauncherBootstrap {
        base_url: config.base_url,
    })
}

pub fn shutdown_child_process() {
    let mut guard = CHILD_PROCESS.lock().expect("child process mutex poisoned");
    let Some(mut child) = guard.take() else {
        return;
    };

    graceful_terminate(&mut child);

    if !wait_for_exit(&mut child, SHUTDOWN_TIMEOUT) {
        let _ = child.kill();
        let _ = child.wait();
    }
}

impl LauncherConfig {
    fn discover(base_url: String, resource_dir: Option<PathBuf>) -> Result<Self, LauncherError> {
        let app_dir = current_app_dir()?;
        let roots = runtime_roots(resource_dir.as_ref(), &app_dir);

        let (runtime_root, java_bin, jar_file) = find_runtime_paths(roots)?;

        let root_dir = env::var("SUWAYOMI_ROOT_DIR").ok();

        Ok(Self {
            runtime_root,
            java_bin,
            jar_file,
            base_url,
            root_dir,
        })
    }
}

fn runtime_roots(resource_dir: Option<&PathBuf>, app_dir: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(resource_dir) = resource_dir {
        push_unique_path(&mut roots, resource_dir.to_path_buf());
        push_unique_path(&mut roots, resource_dir.join("resources"));
    }

    push_unique_path(&mut roots, app_dir.to_path_buf());
    push_unique_path(&mut roots, app_dir.join("resources"));

    #[cfg(target_os = "macos")]
    {
        push_unique_path(&mut roots, app_dir.join("Resources"));
        push_unique_path(&mut roots, app_dir.join("Resources").join("resources"));
    }

    roots
}

fn push_unique_path(paths: &mut Vec<PathBuf>, path: PathBuf) {
    if !paths.iter().any(|existing| existing == &path) {
        paths.push(path);
    }
}

fn current_app_dir() -> Result<PathBuf, LauncherError> {
    let executable = env::current_exe().map_err(|_| LauncherError::MissingExecutable)?;
    let executable_parent = executable.parent().ok_or(LauncherError::MissingExecutable)?;

    #[cfg(target_os = "macos")]
    {
        // Support both plain binary bundles and .app bundle layout.
        if executable_parent.file_name().and_then(|f| f.to_str()) == Some("MacOS") {
            if let Some(contents_dir) = executable_parent.parent() {
                return Ok(contents_dir.to_path_buf());
            }
        }
    }

    Ok(executable_parent.to_path_buf())
}

fn java_binary_path(app_dir: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        app_dir.join("jre").join("bin").join("java.exe")
    }

    #[cfg(not(target_os = "windows"))]
    {
        app_dir.join("jre").join("bin").join("java")
    }
}

fn spawn_server(config: &LauncherConfig) -> Result<Child, LauncherError> {
    let mut command = Command::new(&config.java_bin);

    for arg in build_java_args(config.root_dir.as_deref()) {
        command.arg(arg);
    }

    command.arg("-jar");
    command.arg(&config.jar_file);
    command.current_dir(&config.runtime_root);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x08000000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
        .spawn()
        .map_err(|e| LauncherError::SpawnServer(e.to_string()))
}

fn find_runtime_paths(roots: Vec<PathBuf>) -> Result<(PathBuf, PathBuf, PathBuf), LauncherError> {
    let mut first_missing_java: Option<PathBuf> = None;
    let mut first_missing_jar: Option<PathBuf> = None;

    for root in roots {
        let java_bin = java_binary_path(&root);
        let jar_file = root.join("bin").join("Suwayomi-Server.jar");

        if !java_bin.exists() {
            if first_missing_java.is_none() {
                first_missing_java = Some(java_bin);
            }
            continue;
        }

        if !jar_file.exists() {
            if first_missing_jar.is_none() {
                first_missing_jar = Some(jar_file);
            }
            continue;
        }

        return Ok((root, java_bin, jar_file));
    }

    if let Some(java_path) = first_missing_java {
        return Err(LauncherError::MissingFile(java_path.display().to_string()));
    }

    if let Some(jar_path) = first_missing_jar {
        return Err(LauncherError::MissingFile(jar_path.display().to_string()));
    }

    Err(LauncherError::MissingExecutable)
}

fn build_java_args(root_dir: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "-Dsuwayomi.tachidesk.config.server.initialOpenInBrowserEnabled=false".to_string(),
        "-Dsuwayomi.tachidesk.config.server.webUIInterface=browser".to_string(),
    ];

    if let Some(root_dir) = root_dir {
        args.push(format!("-Dsuwayomi.tachidesk.config.server.rootDir={root_dir}"));
    }

    args
}

fn resolve_base_url() -> String {
    if let Some(cli_url) = env::args().nth(1) {
        if let Some(base_url) = normalize_base_url(&cli_url) {
            return base_url;
        }
    }

    if let Ok(raw_url) = env::var("SUWAYOMI_BASE_URL") {
        if let Some(base_url) = normalize_base_url(&raw_url) {
            return base_url;
        }
    }

    let parsed = load_server_conf().unwrap_or_default();
    build_base_url(&parsed.ip, parsed.port, &parsed.subpath)
}

pub fn fallback_base_url() -> String {
    resolve_base_url()
}

fn load_server_conf() -> Option<ParsedConfig> {
    let config_path =
        env::var("SUWAYOMI_CONFIG_PATH").map(PathBuf::from).ok().or_else(default_server_config_path)?;

    let content = fs::read_to_string(config_path).ok()?;
    Some(parse_server_conf(&content))
}

fn default_server_config_path() -> Option<PathBuf> {
    let mut base = dirs::data_local_dir()?;
    base.push("Tachidesk");
    base.push("server.conf");
    Some(base)
}

fn parse_server_conf(content: &str) -> ParsedConfig {
    let mut config = ParsedConfig::default();

    let ip_pattern = Regex::new(r#"(?m)^\s*server\.ip\s*=\s*\"([^\"]+)\""#).expect("valid regex");
    let port_pattern = Regex::new(r"(?m)^\s*server\.port\s*=\s*(\d+)").expect("valid regex");
    let subpath_pattern =
        Regex::new(r#"(?m)^\s*server\.webUISubpath\s*=\s*\"([^\"]*)\""#).expect("valid regex");

    if let Some(captures) = ip_pattern.captures(content) {
        let ip = captures.get(1).map(|value| value.as_str().trim()).unwrap_or(DEFAULT_IP);
        config.ip = normalize_ip(ip).to_string();
    }

    if let Some(captures) = port_pattern.captures(content) {
        let port = captures.get(1).and_then(|value| value.as_str().parse::<u16>().ok());
        if let Some(port) = port {
            config.port = port;
        }
    }

    if let Some(captures) = subpath_pattern.captures(content) {
        let subpath = captures.get(1).map(|value| value.as_str().trim()).unwrap_or("");
        config.subpath = normalize_subpath(subpath);
    }

    config
}

fn normalize_ip(ip: &str) -> &str {
    if ip == "0.0.0.0" {
        DEFAULT_IP
    } else {
        ip
    }
}

fn normalize_subpath(subpath: &str) -> String {
    if subpath.is_empty() || subpath == "/" {
        return String::new();
    }

    let mut path = subpath.trim().trim_end_matches('/').to_string();
    if !path.starts_with('/') {
        path = format!("/{path}");
    }

    path
}

fn build_base_url(ip: &str, port: u16, subpath: &str) -> String {
    format!("http://{}:{}{}", normalize_ip(ip), port, normalize_subpath(subpath))
}

fn normalize_base_url(url: &str) -> Option<String> {
    let mut parsed = url::Url::parse(url).ok()?;

    if parsed.host_str() == Some("0.0.0.0") {
        parsed.set_host(Some(DEFAULT_IP)).ok()?;
    }

    let normalized = parsed.to_string().trim_end_matches('/').to_string();
    Some(normalized)
}

pub(crate) fn wait_for_server(base_url: &str, timeout: Duration) -> bool {
    let started = Instant::now();

    while started.elapsed() < timeout {
        if is_server_healthy(base_url) {
            return true;
        }

        thread::sleep(POLL_INTERVAL);
    }

    false
}

fn is_server_healthy(base_url: &str) -> bool {
    let health_url = format!("{}{}", base_url.trim_end_matches('/'), HEALTH_ENDPOINT);
    match ureq::get(&health_url).timeout(POLL_INTERVAL).call() {
        Ok(response) => response.status() == 200,
        Err(_) => false,
    }
}

fn wait_for_exit(child: &mut Child, timeout: Duration) -> bool {
    let started = Instant::now();

    while started.elapsed() < timeout {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) => thread::sleep(POLL_INTERVAL),
            Err(_) => return false,
        }
    }

    false
}

fn graceful_terminate(child: &mut Child) {
    #[cfg(unix)]
    {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;

        let _ = kill(Pid::from_raw(child.id() as i32), Signal::SIGTERM);
    }

    #[cfg(windows)]
    {
        let _ = child;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::path::PathBuf;

    #[test]
    fn parse_server_conf_uses_defaults() {
        let parsed = parse_server_conf("server.webUIEnabled = true");

        assert_eq!(parsed.ip, DEFAULT_IP);
        assert_eq!(parsed.port, DEFAULT_PORT);
        assert!(parsed.subpath.is_empty());
    }

    #[test]
    fn parse_server_conf_reads_values() {
        let parsed = parse_server_conf(
            r#"
            server.ip = "0.0.0.0"
            server.port = 8080
            server.webUISubpath = "suwayomi"
            "#,
        );

        assert_eq!(parsed.ip, DEFAULT_IP);
        assert_eq!(parsed.port, 8080);
        assert_eq!(parsed.subpath, "/suwayomi");
    }

    #[test]
    fn build_base_url_normalizes_subpath() {
        assert_eq!(build_base_url("127.0.0.1", 4567, ""), "http://127.0.0.1:4567");
        assert_eq!(build_base_url("127.0.0.1", 4567, "abc/"), "http://127.0.0.1:4567/abc");
    }

    #[test]
    fn build_java_args_includes_root_dir_when_present() {
        let args = build_java_args(Some("/tmp/suwa"));

        assert!(args
            .iter()
            .any(|arg| arg == "-Dsuwayomi.tachidesk.config.server.initialOpenInBrowserEnabled=false"));
        assert!(args
            .iter()
            .any(|arg| arg == "-Dsuwayomi.tachidesk.config.server.rootDir=/tmp/suwa"));
    }

    #[test]
    fn wait_for_server_accepts_healthy_endpoint() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let port = listener.local_addr().expect("listener addr").port();

        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                respond_ok(&mut stream);
            }
        });

        let healthy = wait_for_server(&format!("http://127.0.0.1:{port}"), Duration::from_secs(2));
        assert!(healthy);
    }

    #[test]
    fn runtime_roots_include_nested_resources() {
        let app_dir = PathBuf::from("/tmp/Suwayomi Launcher.app/Contents");
        let resource_dir = PathBuf::from("/tmp/Suwayomi Launcher.app/Contents/Resources");
        let roots = runtime_roots(Some(&resource_dir), &app_dir);

        assert!(roots.contains(&resource_dir));
        assert!(roots.contains(&resource_dir.join("resources")));
    }

    fn respond_ok(stream: &mut TcpStream) {
        let mut buffer = [0_u8; 1024];
        let _ = stream.read(&mut buffer);
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}");
    }
}
