use serde::Serialize;
use std::{
    fs,
    io::BufRead,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::Duration,
};
use tauri::Emitter;

use crate::{data_dir, http_client, safe_filename, write_atomic};

/// EasyTier 组网（对齐 PCL-CE 的联机方案）：主机固定 10.144.144.1，
/// 客人通过 DHCP 在同一子网自动取址；共享节点用于打洞/中继。
const EASYTIER_RELEASE_API: &str = "https://api.github.com/repos/EasyTier/EasyTier/releases/latest";
const HOST_IPV4: &str = "10.144.144.1";
const DEFAULT_PEERS: &[&str] = &[
    "tcp://public.easytier.cn:11010",
    "tcp://public.easytier.top:11010",
    "tcp://easytier.public.kkrainbow.top:11010",
];
const EASYTIER_MAX_BYTES: usize = 200 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub struct MpStatus {
    pub running: bool,
    pub role: String,
    pub network_name: String,
    pub room_code: String,
    pub virtual_ip: String,
    pub peers_connected: usize,
    pub log_tail: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct MpLogPayload {
    line: String,
}

struct Session {
    child: Arc<Mutex<Child>>,
    pid: u32,
    role: &'static str,
    network_name: String,
    room_code: String,
    virtual_ip: Arc<Mutex<String>>,
    peers_connected: Arc<AtomicUsize>,
    log_tail: Arc<Mutex<Vec<String>>>,
    running: Arc<AtomicBool>,
}

impl Session {
    fn status(&self) -> MpStatus {
        MpStatus {
            running: self.running.load(Ordering::SeqCst),
            role: self.role.to_string(),
            network_name: self.network_name.clone(),
            room_code: self.room_code.clone(),
            virtual_ip: self
                .virtual_ip
                .lock()
                .map(|ip| ip.clone())
                .unwrap_or_default(),
            peers_connected: self.peers_connected.load(Ordering::SeqCst),
            log_tail: self
                .log_tail
                .lock()
                .map(|log| log.iter().rev().take(12).rev().cloned().collect())
                .unwrap_or_default(),
        }
    }
}

#[derive(Default)]
pub struct MultiplayerState {
    session: OnceLock<Mutex<Option<Arc<Session>>>>,
}

fn offline_status() -> MpStatus {
    MpStatus {
        running: false,
        role: String::new(),
        network_name: String::new(),
        room_code: String::new(),
        virtual_ip: String::new(),
        peers_connected: 0,
        log_tail: Vec::new(),
    }
}

impl MultiplayerState {
    fn slot(&self) -> &Mutex<Option<Arc<Session>>> {
        self.session.get_or_init(|| Mutex::new(None))
    }

    pub fn status(&self) -> MpStatus {
        let Ok(guard) = self.slot().lock() else {
            return offline_status();
        };
        match guard.as_ref() {
            Some(session) if session.running.load(Ordering::SeqCst) => session.status(),
            _ => offline_status(),
        }
    }

    fn kill_current(&self) {
        let Ok(mut guard) = self.slot().lock() else {
            return;
        };
        if let Some(session) = guard.take() {
            session.running.store(false, Ordering::SeqCst);
            let _ = kill_tree(session.pid);
        }
    }

    fn register(&self, session: Arc<Session>) {
        let Ok(mut guard) = self.slot().lock() else {
            return;
        };
        if let Some(previous) = guard.take() {
            previous.running.store(false, Ordering::SeqCst);
            let _ = kill_tree(previous.pid);
        }
        *guard = Some(session);
    }
}

fn kill_tree(pid: u32) {
    let mut command = Command::new("taskkill");
    command.args(["/PID", &pid.to_string(), "/T", "/F"]);
    hide_console(&mut command);
    let _ = command.output();
}

fn hide_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

fn random_hex(length: usize) -> String {
    use md5::Digest;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let digest = md5::Md5::digest(format!(
        "{}:{}:{}",
        now.as_nanos(),
        std::process::id(),
        length
    ));
    hex::encode(digest).chars().take(length).collect()
}

async fn ensure_easytier(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = data_dir(app)?.join("easytier");
    let exe = dir.join("easytier-core.exe");
    if exe.exists() {
        return Ok(exe);
    }
    if !cfg!(windows) {
        return Err("联机功能目前只支持 Windows".into());
    }
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let client = http_client(app)?;
    let release: serde_json::Value = client
        .get(EASYTIER_RELEASE_API)
        .header("User-Agent", "AuroraLauncher/0.5")
        .send()
        .await
        .map_err(|e| format!("获取 EasyTier 版本信息失败：{e}"))?
        .error_for_status()
        .map_err(|_| "EasyTier 版本接口返回错误".to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let asset = release
        .get("assets")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .find(|asset| {
            asset
                .get("name")
                .and_then(|value| value.as_str())
                .map(|name| name.contains("windows-x86_64") && name.ends_with(".zip"))
                .unwrap_or(false)
        })
        .cloned()
        .ok_or_else(|| "EasyTier 发布里没有 Windows x64 压缩包".to_string())?;
    let download_url = asset
        .get("browser_download_url")
        .and_then(|value| value.as_str())
        .ok_or_else(|| "EasyTier 下载地址缺失".to_string())?;
    let parsed = reqwest::Url::parse(download_url).map_err(|_| "下载地址无效".to_string())?;
    let host = parsed.host_str().unwrap_or_default();
    if parsed.scheme() != "https"
        || !(host == "github.com" || host == "objects.githubusercontent.com")
    {
        return Err("EasyTier 下载地址不受信任".into());
    }
    let response = client
        .get(parsed)
        .timeout(Duration::from_secs(600))
        .send()
        .await
        .map_err(|e| format!("下载 EasyTier 失败：{e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "下载 EasyTier 返回 HTTP {}",
            response.status().as_u16()
        ));
    }
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > EASYTIER_MAX_BYTES {
        return Err("EasyTier 压缩包体积异常".into());
    }
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec()))
        .map_err(|e| format!("EasyTier 压缩包解析失败：{e}"))?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
        let Some(name) = safe_filename(entry.name()) else {
            continue;
        };
        let target = dir.join(name);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            continue;
        }
        let mut buffer = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut buffer).map_err(|e| e.to_string())?;
        write_atomic(&target, &buffer)?;
    }
    if !exe.exists() {
        return Err("EasyTier 压缩包里没有 easytier-core.exe".into());
    }
    Ok(exe)
}

pub fn parse_room_code(code: &str) -> Result<(String, String), String> {
    let (name, secret) = code
        .trim()
        .split_once('#')
        .map(|(name, secret)| (name.trim().to_string(), secret.trim().to_string()))
        .ok_or_else(|| "房间码格式应为 网络名#密钥".to_string())?;
    if name.is_empty() || secret.is_empty() {
        return Err("房间码的网络名和密钥都不能为空".into());
    }
    Ok((name, secret))
}

fn parse_peers(configured: Option<&str>) -> Vec<String> {
    let list: Vec<String> = configured
        .map(|value| {
            value
                .split(|c| c == ',' || c == ';' || c == '\n')
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if list.is_empty() {
        DEFAULT_PEERS.iter().map(|peer| peer.to_string()).collect()
    } else {
        list
    }
}

fn extract_dhcp_ip(line: &str) -> Option<String> {
    let position = line.find("new=Some(")? + "new=Some(".len();
    let rest = &line[position..];
    let end = rest.find('/')?;
    let ip = rest[..end].trim();
    if ip.is_empty() {
        None
    } else {
        Some(ip.to_string())
    }
}

#[allow(clippy::too_many_arguments)]
fn spawn_session(
    app: &tauri::AppHandle,
    state: &MultiplayerState,
    exe: &PathBuf,
    role: &'static str,
    network_name: String,
    room_code: String,
    args: Vec<String>,
    peers: Vec<String>,
) -> Result<MpStatus, String> {
    let mut args = args;
    for peer in &peers {
        args.push("-p".into());
        args.push(peer.clone());
    }
    let mut command = Command::new(exe);
    command
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut command);
    let mut child = command
        .spawn()
        .map_err(|e| format!("启动 EasyTier 失败：{e}"))?;
    let pid = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let child = Arc::new(Mutex::new(child));
    let session = Arc::new(Session {
        child: Arc::clone(&child),
        pid,
        role,
        network_name: network_name.clone(),
        room_code: room_code.clone(),
        virtual_ip: Arc::new(Mutex::new(String::new())),
        peers_connected: Arc::new(AtomicUsize::new(0)),
        log_tail: Arc::new(Mutex::new(Vec::new())),
        running: Arc::new(AtomicBool::new(true)),
    });
    state.register(Arc::clone(&session));
    let _ = app.emit(
        "mp-log",
        MpLogPayload {
            line: format!("[Aurora] EasyTier 已启动（{role}），网络 {network_name}"),
        },
    );

    let streams: Vec<Option<Box<dyn std::io::Read + Send>>> =
        vec![stdout.map(|s| Box::new(s) as Box<dyn std::io::Read + Send>),
             stderr.map(|s| Box::new(s) as Box<dyn std::io::Read + Send>)];
    for stream in streams {
        let Some(stream) = stream else { continue };
        let app = app.clone();
        let log = Arc::clone(&session.log_tail);
        let virtual_ip = Arc::clone(&session.virtual_ip);
        let peers_counter = Arc::clone(&session.peers_connected);
        let running = Arc::clone(&session.running);
        thread::spawn(move || {
            let reader = std::io::BufReader::new(stream);
            for line in reader.lines() {
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(line) = line else { break };
                if let Some(ip) = extract_dhcp_ip(&line) {
                    if let Ok(mut guard) = virtual_ip.lock() {
                        *guard = ip.clone();
                    }
                    let _ = app.emit(
                        "mp-log",
                        MpLogPayload {
                            line: format!("[Aurora] 虚拟 IP：{ip}"),
                        },
                    );
                }
                if line.contains("new peer connection added") {
                    peers_counter.fetch_add(1, Ordering::SeqCst);
                }
                if let Ok(mut guard) = log.lock() {
                    guard.push(line.clone());
                    let length = guard.len();
                    if length > 400 {
                        guard.drain(0..length - 400);
                    }
                }
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.contains("new listener added") {
                    let _ = app.emit("mp-log", MpLogPayload { line: trimmed.to_string() });
                }
            }
        });
    }
    {
        let app = app.clone();
        let running = Arc::clone(&session.running);
        let child_holder = Arc::clone(&session.child);
        thread::spawn(move || {
            let mut exit_code: Option<i32> = None;
            loop {
                if !running.load(Ordering::SeqCst) {
                    break;
                }
                let Ok(mut guard) = child_holder.lock() else {
                    break;
                };
                match guard.try_wait() {
                    Ok(Some(status)) => {
                        exit_code = status.code();
                        break;
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
                drop(guard);
                thread::sleep(Duration::from_millis(500));
            }
            running.store(false, Ordering::SeqCst);
            let _ = app.emit(
                "mp-log",
                MpLogPayload {
                    line: format!("[Aurora] EasyTier 进程已退出（退出码 {exit_code:?}）"),
                },
            );
        });
    }
    Ok(session.status())
}

pub async fn host(app: &tauri::AppHandle, state: &MultiplayerState) -> Result<MpStatus, String> {
    let exe = ensure_easytier(app).await?;
    let name = format!("aurora-{}", random_hex(6));
    let secret = random_hex(8);
    let peers = parse_peers(Some(crate::read_settings(app).mp_peers.as_str()));
    let args = vec![
        "--network-name".to_string(),
        name.clone(),
        "--network-secret".to_string(),
        secret.clone(),
        "--ipv4".to_string(),
        HOST_IPV4.to_string(),
        "--instance-name".to_string(),
        "aurora".to_string(),
    ];
    spawn_session(
        app,
        state,
        &exe,
        "host",
        name.clone(),
        format!("{name}#{secret}"),
        args,
        peers,
    )
}

pub async fn join(
    app: &tauri::AppHandle,
    state: &MultiplayerState,
    code: &str,
) -> Result<MpStatus, String> {
    let (network_name, secret) = parse_room_code(code)?;
    let exe = ensure_easytier(app).await?;
    let peers = parse_peers(Some(crate::read_settings(app).mp_peers.as_str()));
    let args = vec![
        "--network-name".to_string(),
        network_name.clone(),
        "--network-secret".to_string(),
        secret,
        "--dhcp".to_string(),
        "true".to_string(),
        "--instance-name".to_string(),
        "aurora".to_string(),
    ];
    spawn_session(
        app,
        state,
        &exe,
        "join",
        network_name,
        code.trim().to_string(),
        args,
        peers,
    )
}

pub fn stop(state: &MultiplayerState) -> MpStatus {
    state.kill_current();
    state.status()
}

pub fn status(state: &MultiplayerState) -> MpStatus {
    state.status()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn room_codes_are_parsed() {
        assert!(parse_room_code("aurora-abc123#deadbeef").is_ok());
        assert_eq!(
            parse_room_code("  aurora-abc123 # deadbeef ").unwrap(),
            ("aurora-abc123".to_string(), "deadbeef".to_string())
        );
        assert!(parse_room_code("no-secret-here").is_err());
        assert!(parse_room_code("name#").is_err());
        assert!(parse_room_code("#secret").is_err());
        assert!(parse_room_code("").is_err());
    }

    #[test]
    fn random_hex_is_length_limited() {
        let value = random_hex(6);
        assert_eq!(value.len(), 6);
        assert!(value.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn peers_default_when_configured_empty() {
        assert_eq!(parse_peers(None).len(), DEFAULT_PEERS.len());
        assert_eq!(parse_peers(Some("")).len(), DEFAULT_PEERS.len());
        let custom =
            parse_peers(Some("tcp://my-node.example:11010, tcp://backup:11010"));
        assert_eq!(
            custom,
            vec!["tcp://my-node.example:11010", "tcp://backup:11010"]
        );
    }

    #[test]
    fn virtual_ip_is_extracted_from_dhcp_lines() {
        let line = r#"INFO CORE::INSTANCE: dhcp ip changed old=None new=Some(10.144.144.2/24)"#;
        assert_eq!(
            extract_dhcp_ip(line).as_deref(),
            Some("10.144.144.2")
        );
        assert_eq!(extract_dhcp_ip("unrelated log line"), None);
        assert_eq!(extract_dhcp_ip("dhcp ip changed new=Some(/24)"), None);
    }
}
