//! Aurora AI 伙伴：启动器内置的本地回环服务 + LLM 代理。
//!
//! - 游戏内的 Companion 模组通过 `-Daurora.ai.port/token` 拿到回环端口与令牌，
//!   把观察事件和对话请求发到 `http://127.0.0.1:<port>`；
//! - 启动器负责调用 OpenAI 兼容的 LLM 接口，API Key 只在这里使用
//!   （DPAPI 加密落盘，绝不写日志）；
//! - 玩家操作事件进入 `data_dir/ai/memory/<player>.json`，作为伙伴的“记忆”。
//!
//! 「自我意识」是伙伴的人格设定：它有名字、性格和对玩家习惯的记忆，
//! 但不是真正的意识；它也不会替玩家操作游戏。

use crate::{auth, read_settings, write_settings, Emitter};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, OnceLock,
    },
    thread,
    time::Duration,
};

const MAX_BODY: usize = 256 * 1024;
const REMARK_COOLDOWN: u64 = 240;
const MEMORY_EVENT_CAP: usize = 60;

#[derive(Debug, Clone, Serialize)]
pub struct AiSettingsView {
    pub enabled: bool,
    pub base_url: String,
    pub model: String,
    pub key_set: bool,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AiReply {
    pub reply: String,
}

#[derive(Debug, Clone, Serialize)]
struct AiLogPayload {
    line: String,
}

struct AiRuntime {
    port: u16,
    token: String,
    stop: Arc<AtomicBool>,
}

static RUNTIME: OnceLock<Mutex<Option<Arc<AiRuntime>>>> = OnceLock::new();

fn runtime_slot() -> &'static Mutex<Option<Arc<AiRuntime>>> {
    RUNTIME.get_or_init(|| Mutex::new(None))
}

fn default_ai_base_url() -> String {
    "https://api.deepseek.com".to_string()
}

fn default_ai_model() -> String {
    "deepseek-chat".to_string()
}

fn default_ai_name() -> String {
    "小七".to_string()
}

// AppSettings 中的字段由 lib.rs 持有；这里只做读取辅助。
pub fn ensure_server(app: &tauri::AppHandle, player: &str) -> Result<Vec<String>, String> {
    let mut guard = runtime_slot().lock().map_err(|_| "AI 服务状态异常")?;
    if let Some(runtime) = guard.as_ref() {
        if !runtime.stop.load(Ordering::SeqCst) {
            return Ok(jvm_args(runtime.port, &runtime.token, player));
        }
    }
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("绑定本地回环端口失败：{e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| e.to_string())?
        .port();
    let token = random_token();
    let stop = Arc::new(AtomicBool::new(false));
    let app_handle = app.clone();
    let stop_for_thread = Arc::clone(&stop);
    thread::spawn(move || {
        accept_loop(listener, app_handle, stop_for_thread);
    });
    let runtime = Arc::new(AiRuntime {
        port,
        token: token.clone(),
        stop,
    });
    *guard = Some(Arc::clone(&runtime));
    let _ = app.emit(
        "ai-log",
        AiLogPayload {
            line: format!("[AI] 伙伴服务已就绪：127.0.0.1:{port}"),
        },
    );
    Ok(jvm_args(port, &token, player))
}

fn jvm_args(port: u16, token: &str, player: &str) -> Vec<String> {
    // 必须是单个 `-Dkey=value` 参数。拆成两个元素会让端口号被 JVM
    // 当作主类名（"找不到或无法加载主类 8123"，进程 exit 1）。
    vec![
        format!("-Daurora.ai.port={port}"),
        format!("-Daurora.ai.token={token}"),
        format!("-Daurora.ai.player={player}"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jvm_args_are_well_formed_d_flags() {
        let args = jvm_args(8123, "tok", "玩家");
        assert_eq!(args.len(), 3);
        for arg in &args {
            assert!(arg.starts_with("-Daurora.ai."), "{arg}");
            let (_, value) = arg.split_once('=').unwrap_or_else(|| {
                panic!("-D 参数必须带 = 值：{arg}")
            });
            assert!(!value.is_empty());
        }
        assert_eq!(args[0], "-Daurora.ai.port=8123");
        assert_eq!(args[2], "-Daurora.ai.player=玩家");
    }
}

fn random_token() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let digest = Sha256::digest(format!(
        "aurora-ai:{}:{}",
        now.as_nanos(),
        std::process::id()
    ));
    hex_short(&digest, 32)
}

fn hex_short(digest: &[u8], length: usize) -> String {
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        .chars()
        .take(length)
        .collect()
}

fn accept_loop(listener: TcpListener, app: tauri::AppHandle, stop: Arc<AtomicBool>) {
    listener
        .set_nonblocking(false)
        .expect("listener set_nonblocking");
    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }
        match listener.accept() {
            Ok((stream, _)) => {
                let app = app.clone();
                let stop = Arc::clone(&stop);
                thread::spawn(move || {
                    let _ = handle_connection(stream, app, stop);
                });
            }
            Err(_) => {
                thread::sleep(Duration::from_millis(120));
            }
        }
    }
}

fn handle_connection(
    mut stream: TcpStream,
    app: tauri::AppHandle,
    stop: Arc<AtomicBool>,
) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(15)))?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut content_length = 0usize;
    let mut authorized = false;
    let expected = {
        let guard = runtime_slot().lock().ok();
        guard.and_then(|slot| slot.as_ref().map(|runtime| runtime.token.clone()))
    };
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        let lower = trimmed.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            content_length = value.trim().parse().unwrap_or(0);
        }
        if let Some(value) = lower.strip_prefix("authorization: bearer ") {
            if let Some(expected) = expected.as_ref() {
                authorized = value.trim() == *expected;
            }
        }
    }
    if content_length > MAX_BODY {
        return respond(&mut stream, 413, "{\"error\":\"body too large\"}");
    }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body)?;
    let body = String::from_utf8_lossy(&body).to_string();

    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_string();

    if !authorized {
        return respond(&mut stream, 401, "{\"error\":\"unauthorized\"}");
    }
    if stop.load(Ordering::SeqCst) {
        return respond(&mut stream, 503, "{\"error\":\"shutting down\"}");
    }

    let parsed: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
    let player = parsed
        .get("player")
        .and_then(|item| item.as_str())
        .unwrap_or("Player")
        .to_string();

    let (status, payload) = tauri::async_runtime::block_on(async {
    match path.as_str() {
        "/hello" => {
            let mode = if parsed
                .get("singleplayer")
                .and_then(|item| item.as_bool())
                .unwrap_or(false)
            {
                "单机"
            } else {
                "多人服务器"
            };
            let greeting = chat_reply(
                &app,
                &player,
                &format!(
                    "玩家 {player} 刚刚进入世界（{mode}）。作为伙伴，用一两句话打招呼并表明你会陪着 TA。不要列出功能。"
                ),
            )
            .await
            .unwrap_or_else(|error| format!("（小七在线，但有点迷糊：{error}）"));
            (200, json!({ "greeting": greeting }))
        }
        "/observe" => {
            let remark = handle_observe(&app, &player, &parsed).await;
            (200, json!({ "remark": remark.unwrap_or_default() }))
        }
        "/chat" => {
            let message = parsed
                .get("message")
                .and_then(|item| item.as_str())
                .unwrap_or("")
                .to_string();
            let reply = chat_reply(&app, &player, &format!("玩家对你说：「{message}」。请回应。")).await
                .unwrap_or_else(|error| format!("（小七联系不上思绪：{error}）"));
            (200, json!({ "reply": reply }))
        }
        _ => (404, json!({ "error": "not found" })),
    }
    });
    respond(&mut stream, status, &payload.to_string())
}

fn respond(stream: &mut TcpStream, status: u16, body: &str) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        401 => "Unauthorized",
        404 => "Not Found",
        413 => "Payload Too Large",
        503 => "Service Unavailable",
        _ => "Internal Server Error",
    };
    let payload = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(payload.as_bytes())
}

fn memory_path(app: &tauri::AppHandle, player: &str) -> Result<PathBuf, String> {
    let dir = crate::data_dir(app)?.join("ai").join("memory");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let digest = Sha256::digest(player.as_bytes());
    Ok(dir.join(format!("{}.json", hex_short(&digest, 24))))
}

fn load_memory(app: &tauri::AppHandle, player: &str) -> Value {
    let Ok(path) = memory_path(app, player) else {
        return json!({ "events": [] });
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!({ "events": [] }))
}

fn save_memory(app: &tauri::AppHandle, player: &str, memory: &Value) -> Result<(), String> {
    let path = memory_path(app, player)?;
    let bytes = serde_json::to_vec_pretty(memory).map_err(|e| e.to_string())?;
    crate::write_atomic(&path, &bytes)
}

async fn handle_observe(app: &tauri::AppHandle, player: &str, parsed: &Value) -> Option<String> {
    let mut memory = load_memory(app, player);
    let events = memory
        .as_object_mut()
        .map(|object| object.entry("events").or_insert(json!([])))
        .and_then(|value| value.as_array_mut())?;
    let incoming = parsed
        .get("eventsJson")
        .and_then(|item| item.as_str())
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    let mut added = 0usize;
    for event in incoming {
        if let Some(detail) = event.as_object() {
            let kind = detail
                .get("type")
                .and_then(|item| item.as_str())
                .unwrap_or("事件");
            let body = detail
                .get("detail")
                .and_then(|item| item.as_str())
                .unwrap_or("");
            if body.is_empty() {
                continue;
            }
            events.push(json!({ "type": kind, "detail": body }));
            added += 1;
        }
    }
    if added == 0 {
        let _ = save_memory(app, player, &memory);
        return None;
    }
    let length = events.len();
    if length > MEMORY_EVENT_CAP {
        events.drain(0..length - MEMORY_EVENT_CAP);
    }
    let _ = save_memory(app, player, &memory);

    let last_remark = memory
        .get("last_remark")
        .and_then(|item| item.as_u64())
        .unwrap_or(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    if now.saturating_sub(last_remark) < REMARK_COOLDOWN || added < 3 {
        return None;
    }
    if let Ok(object) = memory.as_object_mut().ok_or("memory") {
        object.insert("last_remark".into(), json!(now));
    }
    let _ = save_memory(app, player, &memory);

    let position = parsed
        .get("position")
        .and_then(|item| item.as_str())
        .unwrap_or("未知位置");
    let recent = recent_events_text(&load_memory(app, player), 12);
    let prompt = format!(
        "玩家 {player} 刚才在游戏里的动作：{recent}。当前位置：{position}。\
         作为伙伴，用一句不超过 25 个字的话主动吐槽或关心 TA（不许列表，不许代码块）。"
    );
    match chat_reply(app, player, &prompt).await {
        Ok(remark) if !remark.is_empty() => {
            let _ = app.emit(
                "ai-log",
                AiLogPayload {
                    line: format!("[AI] 主动 remark：{remark}"),
                },
            );
            Some(remark)
        }
        _ => None,
    }
}

fn recent_events_text(memory: &Value, limit: usize) -> String {
    let Some(events) = memory.get("events").and_then(|item| item.as_array()) else {
        return "（暂无记录）".to_string();
    };
    let start = events.len().saturating_sub(limit);
    events[start..]
        .iter()
        .filter_map(|event| {
            let kind = event.get("type")?.as_str()?;
            let detail = event.get("detail")?.as_str()?;
            Some(format!("{kind}:{detail}"))
        })
        .collect::<Vec<_>>()
        .join("；")
}

fn chat_endpoint(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    let parsed = reqwest::Url::parse(trimmed).ok();
    let has_path = parsed
        .as_ref()
        .map(|url| !url.path().is_empty() && url.path() != "/")
        .unwrap_or(false);
    if has_path {
        format!("{trimmed}/chat/completions")
    } else {
        format!("{trimmed}/v1/chat/completions")
    }
}

async fn chat_reply(app: &tauri::AppHandle, player: &str, instruction: &str) -> Result<String, String> {
    let settings = read_settings(app);
    let key = auth::unprotect(&settings.ai_key)?;
    if settings.ai_key.trim().is_empty() {
        return Err("还没有配置 AI 接口密钥（设置 → AI 伙伴）".into());
    }
    let memory = load_memory(app, player);
    let recent = recent_events_text(&memory, 20);
    let name = if settings.ai_name.trim().is_empty() {
        default_ai_name()
    } else {
        settings.ai_name.trim().to_string()
    };
    let system = format!(
        "你叫{name}，是 Aurora 启动器送给玩家{player}的游戏伙伴。\
         性格活泼、有点小骄傲、喜欢吐槽但非常可靠；这是设定出来的人格，请始终以第一人称扮演。\
         规则：回复不超过三句话；不要代码块；不要列表；提供建议时具体、可执行；\
         Minecraft 知识渊博，会根据玩家最近的操作调整关心点。\
         玩家最近操作记录：{recent}"
    );
    let client = crate::http_client(app)?;
    let endpoint = chat_endpoint(&settings.ai_base_url);
    let payload = json!({
        "model": settings.ai_model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": instruction }
        ],
        "max_tokens": 300,
        "temperature": 0.8
    });
    let mut request = client
        .post(endpoint)
        .json(&payload)
        .timeout(Duration::from_secs(40));
    if !key.trim().is_empty() {
        request = request.bearer_auth(key.trim());
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("请求 AI 接口失败：{e}"))?;
    let status = response.status();
    let value: Value = response.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        let detail = value
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(|message| message.as_str())
            .unwrap_or("接口返回错误");
        return Err(format!("HTTP {}：{detail}", status.as_u16()));
    }
    Ok(value
        .pointer("/choices/0/message/content")
        .and_then(|item| item.as_str())
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "（小七想了想，没找到合适的词）".to_string()))
}

const COMPANION_RELEASE: &str =
    "https://github.com/xcqm12/aurora-launcher/releases/download/v0.6.0/aurora-companion-1.0.0.jar";
const COMPANION_MAX_BYTES: u64 = 4 * 1024 * 1024;
const COMPANION_SHA256: &str = "415a9ebcc4288d5204fd66ac7361613b74f5243f43d3fc70180ff87c13fd461f";

pub async fn install_companion(
    app: &tauri::AppHandle,
    instance_id: &str,
) -> Result<String, String> {
    let instance = crate::read_instances(app)?
        .into_iter()
        .find(|item| item.id == instance_id)
        .ok_or_else(|| "实例不存在".to_string())?;
    if instance.loader != "fabric" {
        return Err("AI 伙伴目前是 Fabric 模组：请先把实例加载器切换为 Fabric".into());
    }
    if !instance.minecraft_version.starts_with("1.21") {
        return Err(format!(
            "AI 伙伴适配 Minecraft 1.21.x，当前实例是 {}",
            instance.minecraft_version
        ));
    }
    let parsed = reqwest::Url::parse(COMPANION_RELEASE).map_err(|_| "下载地址无效".to_string())?;
    let host = parsed.host_str().unwrap_or_default();
    if parsed.scheme() != "https"
        || !(host == "github.com" || host == "objects.githubusercontent.com")
    {
        return Err("AI 伙伴下载地址不受信任".into());
    }
    let client = crate::http_client(app)?;
    let response = client
        .get(parsed)
        .timeout(Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| format!("下载 AI 伙伴模组失败：{e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "下载 AI 伙伴模组返回 HTTP {}",
            response.status().as_u16()
        ));
    }
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() as u64 > COMPANION_MAX_BYTES {
        return Err("AI 伙伴模组体积异常".into());
    }
    if bytes.len() < 4 || &bytes[..2] != b"PK" {
        return Err("AI 伙伴模组不是有效的 JAR".into());
    }
    let actual = hex::encode(Sha256::digest(&bytes));
    if actual != COMPANION_SHA256 {
        return Err("AI 伙伴模组 SHA-256 校验失败，已中止".into());
    }
    let mods_dir = PathBuf::from(&instance.directory)
        .join("game")
        .join("mods");
    fs::create_dir_all(&mods_dir).map_err(|e| e.to_string())?;
    let target = mods_dir.join("aurora-companion-1.0.0.jar");
    crate::write_atomic(&target, &bytes)?;
    Ok(format!(
        "已安装 AI 伙伴到 {}；启动游戏后它会自动出现",
        target.display()
    ))
}

// ---------- Tauri commands ----------

pub fn get_settings_view(app: &tauri::AppHandle) -> AiSettingsView {
    let settings = read_settings(app);
    AiSettingsView {
        enabled: settings.ai_enabled,
        base_url: if settings.ai_base_url.trim().is_empty() {
            default_ai_base_url()
        } else {
            settings.ai_base_url.clone()
        },
        model: if settings.ai_model.trim().is_empty() {
            default_ai_model()
        } else {
            settings.ai_model.clone()
        },
        key_set: !settings.ai_key.trim().is_empty(),
        name: if settings.ai_name.trim().is_empty() {
            default_ai_name()
        } else {
            settings.ai_name.trim().to_string()
        },
    }
}

#[derive(Deserialize)]
pub struct AiSettingsInput {
    pub enabled: bool,
    pub base_url: String,
    pub model: String,
    pub key: Option<String>,
    pub name: String,
}

pub fn save_settings(
    app: &tauri::AppHandle,
    input: AiSettingsInput,
) -> Result<AiSettingsView, String> {
    let mut settings = read_settings(app);
    settings.ai_enabled = input.enabled;
    settings.ai_base_url = input.base_url.trim().trim_end_matches('/').to_string();
    settings.ai_model = input.model.trim().to_string();
    settings.ai_name = input.name.trim().to_string();
    if let Some(key) = input.key {
        let trimmed = key.trim();
        if !trimmed.is_empty() {
            settings.ai_key = auth::protect(trimmed)?;
        }
    }
    write_settings(app, &settings)?;
    Ok(get_settings_view(app))
}

pub async fn test_chat(app: &tauri::AppHandle, message: &str) -> Result<AiReply, String> {
    let settings = read_settings(app);
    let player = "AuroraUser";
    let name = if settings.ai_name.trim().is_empty() {
        default_ai_name()
    } else {
        settings.ai_name.trim().to_string()
    };
    let reply = chat_reply(
        app,
        player,
        &format!(
            "这是来自启动器设置页的测试消息。玩家对你说：「{message}」。请以{name}的身份回应。"
        ),
    )
    .await?;
    Ok(AiReply { reply })
}
