use crate::{
    auth::{self, Account, AccountView},
    data_dir, http_client, write_atomic,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::{Mutex, OnceLock},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

pub const DEFAULT_SERVER: &str = "https://littleskin.cn/api/yggdrasil/";
const AUTHLIB_LATEST: &str = "https://authlib-injector.yushi.moe/artifact/latest.json";
const OAUTH_CLIENT_ID: &str = "800";
const OAUTH_DEVICE_URL: &str = "https://open.littleskin.cn/oauth/device_code";
const OAUTH_TOKEN_URL: &str = "https://open.littleskin.cn/oauth/token";
const OAUTH_SCOPE: &str =
    "openid offline_access Yggdrasil.PlayerProfiles.Select Yggdrasil.MinecraftToken.Create";
const OAUTH_WHITELIST_HINT: &str = "应用未加入 LittleSkin 设备代码流白名单：请把 OAuth 应用的回调 URL 设为 https://open.littleskin.cn/oauth/callback，并用绑定邮箱发工单（标题：申请 OAuth 设备代码流白名单，正文写应用名称与客户端 ID 800 及 Yggdrasil 权限）。";
const PENDING_TTL: Duration = Duration::from_secs(600);

#[derive(Debug, Clone)]
pub struct LaunchAuthlib {
    pub jar: PathBuf,
    pub server: String,
    pub prefetched: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProfileView {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Serialize)]
pub struct LoginOutcome {
    pub account: Option<AccountView>,
    pub pending_id: Option<String>,
    pub profiles: Vec<ProfileView>,
}

struct PendingAuth {
    server: String,
    access_token: String,
    client_token: String,
    metadata: String,
    profiles: Vec<ProfileView>,
    created: Instant,
}

static PENDING: OnceLock<Mutex<HashMap<String, PendingAuth>>> = OnceLock::new();

fn pending_store() -> &'static Mutex<HashMap<String, PendingAuth>> {
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

fn random_token() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let seed = format!("{}:{}", now.as_nanos(), std::process::id());
    let digest = md5::Md5::digest(seed.as_bytes());
    digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn dash_uuid(value: &str) -> String {
    let hex: String = value
        .chars()
        .filter(|item| item.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_lowercase();
    if hex.len() != 32 {
        return value.to_ascii_lowercase();
    }
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

pub fn normalize_server(value: &str) -> Result<String, String> {
    let raw = value.trim();
    let raw = if raw.is_empty() { DEFAULT_SERVER } else { raw };
    if raw.starts_with("http://") {
        return Err("只支持 https 的 Yggdrasil 服务器".into());
    }
    let mut base = if raw.starts_with("https://") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    };
    while base.ends_with('/') {
        base.pop();
    }
    if let Some(stripped) = base.strip_suffix("/api/yggdrasil") {
        base = stripped.to_string();
    }
    let root = format!("{}/api/yggdrasil/", base.trim_end_matches('/'));
    let url = reqwest::Url::parse(&root).map_err(|_| "Yggdrasil 服务器地址无效".to_string())?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err("Yggdrasil 服务器必须使用 https".into());
    }
    Ok(root)
}

async fn fetch_metadata(client: &reqwest::Client, server: &str) -> Result<String, String> {
    let response = client
        .get(server)
        .header("Accept", "application/json")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| format!("连接 Yggdrasil 服务器失败：{e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "Yggdrasil 元数据请求失败（HTTP {}）",
            response.status().as_u16()
        ));
    }
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > 512 * 1024 {
        return Err("Yggdrasil 元数据过大".into());
    }
    let text = String::from_utf8(bytes.to_vec()).map_err(|_| "Yggdrasil 元数据不是 UTF-8".to_string())?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|_| "Yggdrasil 元数据不是 JSON".to_string())?;
    if value
        .pointer("/meta/serverName")
        .and_then(|item| item.as_str())
        .is_none()
    {
        return Err("该地址不是 authlib-injector 服务器（缺少 meta.serverName）".into());
    }
    Ok(base64::engine::general_purpose::STANDARD.encode(text.as_bytes()))
}

#[derive(Deserialize, Clone)]
struct RawProfile {
    id: String,
    name: String,
}

fn profile_views(profiles: &[RawProfile]) -> Vec<ProfileView> {
    profiles
        .iter()
        .map(|profile| ProfileView {
            id: profile.id.clone(),
            name: profile.name.clone(),
        })
        .collect()
}

fn is_valid_credential_error(status: reqwest::StatusCode) -> bool {
    status.as_u16() == 403
}

async fn authenticate(
    client: &reqwest::Client,
    server: &str,
    username: &str,
    password: &str,
    client_token: &str,
) -> Result<(String, String, Vec<ProfileView>, Option<ProfileView>), String> {
    let response = client
        .post(format!("{server}authserver/authenticate"))
        .json(&serde_json::json!({
            "username": username,
            "password": password,
            "clientToken": client_token,
            "requestUser": false,
            "agent": { "name": "Minecraft", "version": 1 }
        }))
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("连接 LittleSkin 失败：{e}"))?;
    let status = response.status();
    let value: serde_json::Value = response.json().await.unwrap_or_default();
    if !status.is_success() {
        let message = value
            .get("errorMessage")
            .and_then(|item| item.as_str())
            .unwrap_or("用户名或密码错误");
        return Err(if is_valid_credential_error(status) {
            format!("LittleSkin 登录失败：{message}")
        } else {
            format!("LittleSkin 登录失败（HTTP {}）：{message}", status.as_u16())
        });
    }
    let access_token = value
        .get("accessToken")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "LittleSkin 响应缺少 accessToken".to_string())?
        .to_string();
    let returned_client = value
        .get("clientToken")
        .and_then(|item| item.as_str())
        .unwrap_or(client_token)
        .to_string();
    let profiles: Vec<RawProfile> = value
        .get("availableProfiles")
        .and_then(|item| item.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| serde_json::from_value::<RawProfile>(item.clone()).ok())
                .collect()
        })
        .unwrap_or_default();
    let selected = value
        .get("selectedProfile")
        .and_then(|item| serde_json::from_value::<RawProfile>(item.clone()).ok());
    Ok((access_token, returned_client, profile_views(&profiles), selected.map(|profile| ProfileView { id: profile.id, name: profile.name })))
}

async fn validate(
    client: &reqwest::Client,
    server: &str,
    access_token: &str,
    client_token: &str,
) -> Result<bool, String> {
    let response = client
        .post(format!("{server}authserver/validate"))
        .json(&serde_json::json!({
            "accessToken": access_token,
            "clientToken": client_token
        }))
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if response.status().is_success() {
        return Ok(true);
    }
    if response.status().as_u16() == 403 {
        return Ok(false);
    }
    Err(format!(
        "LittleSkin 校验令牌失败（HTTP {}）",
        response.status().as_u16()
    ))
}

async fn refresh(
    client: &reqwest::Client,
    server: &str,
    access_token: &str,
    client_token: &str,
) -> Result<(String, String), String> {
    let response = client
        .post(format!("{server}authserver/refresh"))
        .json(&serde_json::json!({
            "accessToken": access_token,
            "clientToken": client_token,
            "requestUser": false
        }))
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("刷新 LittleSkin 令牌失败：{e}"))?;
    let status = response.status();
    let value: serde_json::Value = response.json().await.unwrap_or_default();
    if !status.is_success() {
        let message = value
            .get("errorMessage")
            .and_then(|item| item.as_str())
            .unwrap_or("凭据已失效");
        return Err(format!("刷新 LittleSkin 令牌失败：{message}"));
    }
    let access = value
        .get("accessToken")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "刷新响应缺少 accessToken".to_string())?
        .to_string();
    let client_token = value
        .get("clientToken")
        .and_then(|item| item.as_str())
        .unwrap_or(client_token)
        .to_string();
    Ok((access, client_token))
}

fn build_account(
    server: &str,
    metadata: &str,
    access_token: &str,
    client_token: &str,
    profile: &ProfileView,
    oauth_refresh: Option<&str>,
) -> Result<Account, String> {
    let uuid = dash_uuid(&profile.id);
    Ok(Account {
        id: format!("ygg-{uuid}"),
        kind: "yggdrasil".into(),
        name: profile.name.clone(),
        uuid,
        xuid: String::new(),
        access_token: auth::protect(access_token)?,
        expires_at: 0,
        refresh_token_enc: match oauth_refresh {
            Some(value) if !value.is_empty() => auth::protect(value)?,
            _ => String::new(),
        },
        server: server.to_string(),
        client_token: client_token.to_string(),
        metadata: metadata.to_string(),
    })
}

fn unsigned_uuid(value: &str) -> String {
    value
        .chars()
        .filter(|item| item.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_lowercase()
}

pub async fn login(
    app: &tauri::AppHandle,
    server: &str,
    username: &str,
    password: &str,
) -> Result<LoginOutcome, String> {
    let username = username.trim();
    if username.is_empty() || password.is_empty() {
        return Err("请填写 LittleSkin 邮箱/用户名和密码".into());
    }
    let server = normalize_server(server)?;
    let client = http_client(app)?;
    let metadata = fetch_metadata(&client, &server).await?;
    let client_token = random_token();
    let (access_token, client_token, profiles, selected) =
        authenticate(&client, &server, username, password, &client_token).await?;
    if profiles.is_empty() && selected.is_none() {
        return Err("该账户还没有角色，请先在 littleskin.cn 创建角色后再登录".into());
    }
    let chosen = selected.or_else(|| {
        if profiles.len() == 1 {
            profiles.first().cloned()
        } else {
            None
        }
    });
    if let Some(profile) = chosen {
        let account = build_account(
            &server,
            &metadata,
            &access_token,
            &client_token,
            &profile,
            None,
        )?;
        let view = auth::upsert(app, account)?;
        return Ok(LoginOutcome {
            account: Some(view),
            pending_id: None,
            profiles,
        });
    }
    let pending_id = random_token();
    pending_store()
        .lock()
        .map_err(|_| "登录状态损坏，请重试".to_string())?
        .insert(
            pending_id.clone(),
            PendingAuth {
                server,
                access_token,
                client_token,
                metadata,
                profiles: profiles.clone(),
                created: Instant::now(),
            },
        );
    Ok(LoginOutcome {
        account: None,
        pending_id: Some(pending_id),
        profiles,
    })
}

pub fn select(
    app: &tauri::AppHandle,
    pending_id: &str,
    profile_id: &str,
) -> Result<AccountView, String> {
    let pending = pending_store()
        .lock()
        .map_err(|_| "登录状态损坏，请重试".to_string())?
        .remove(pending_id)
        .ok_or_else(|| "登录会话已过期，请重新登录".to_string())?;
    if pending.created.elapsed() > PENDING_TTL {
        return Err("登录会话已过期，请重新登录".into());
    }
    let profile = pending
        .profiles
        .iter()
        .find(|item| item.id == profile_id)
        .ok_or_else(|| "角色不存在，请重新登录".to_string())?;
    let account = build_account(
        &pending.server,
        &pending.metadata,
        &pending.access_token,
        &pending.client_token,
        profile,
        None,
    )?;
    auth::upsert(app, account)
}

#[derive(Debug, Serialize)]
pub struct OauthDevice {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Debug, Serialize)]
pub struct OauthPoll {
    pub status: String,
    pub terminal: bool,
    pub message: String,
    pub account: Option<AccountView>,
}

fn decode_jwt_payload(token: &str) -> Option<serde_json::Value> {
    let payload = token.split('.').nth(1)?;
    let engine = &base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let bytes = engine
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload))
        .ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn selected_profile_from_id_token(token: &str) -> Option<ProfileView> {
    let value = decode_jwt_payload(token)?;
    let profile = value.get("selectedProfile")?;
    Some(ProfileView {
        id: profile.get("id")?.as_str()?.to_string(),
        name: profile.get("name")?.as_str()?.to_string(),
    })
}

fn oauth_device_error(error: &str, description: &str) -> String {
    if error == "invalid_client" {
        return OAUTH_WHITELIST_HINT.to_string();
    }
    if error == "invalid_scope" {
        return "LittleSkin 拒绝了 scope：Yggdrasil.PlayerProfiles.Read 与 Select 不能同时申请，且必须包含 openid。".into();
    }
    if description.is_empty() {
        format!("LittleSkin 授权请求失败：{error}")
    } else {
        format!("LittleSkin 授权请求失败：{description}")
    }
}

pub async fn oauth_start(app: &tauri::AppHandle) -> Result<OauthDevice, String> {
    let client = http_client(app)?;
    let response = client
        .post(OAUTH_DEVICE_URL)
        .header("Accept", "application/json")
        .form(&[("client_id", OAUTH_CLIENT_ID), ("scope", OAUTH_SCOPE)])
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("连接 LittleSkin 授权服务失败：{e}"))?;
    let status = response.status();
    let value: serde_json::Value = response.json().await.unwrap_or_default();
    if !status.is_success() {
        let error = value
            .get("error")
            .and_then(|item| item.as_str())
            .unwrap_or("unknown");
        let description = value
            .get("error_description")
            .and_then(|item| item.as_str())
            .unwrap_or_default();
        return Err(oauth_device_error(error, description));
    }
    let device_code = value
        .get("device_code")
        .and_then(|item| item.as_str())
        .unwrap_or_default()
        .to_string();
    if device_code.is_empty() {
        return Err("LittleSkin 响应缺少 device_code".into());
    }
    Ok(OauthDevice {
        device_code,
        user_code: value
            .get("user_code")
            .and_then(|item| item.as_str())
            .unwrap_or_default()
            .into(),
        verification_uri: value
            .get("verification_uri")
            .and_then(|item| item.as_str())
            .unwrap_or("https://open.littleskin.cn/oauth/link")
            .into(),
        verification_uri_complete: value
            .get("verification_uri_complete")
            .and_then(|item| item.as_str())
            .map(str::to_string),
        expires_in: value
            .get("expires_in")
            .and_then(|item| item.as_u64())
            .unwrap_or(300),
        interval: value
            .get("interval")
            .and_then(|item| item.as_u64())
            .unwrap_or(5),
    })
}

pub async fn oauth_poll(
    app: &tauri::AppHandle,
    device_code: &str,
) -> Result<OauthPoll, String> {
    let client = http_client(app)?;
    let response = client
        .post(OAUTH_TOKEN_URL)
        .header("Accept", "application/json")
        .form(&[
            (
                "grant_type",
                "urn:ietf:params:oauth:grant-type:device_code",
            ),
            ("client_id", OAUTH_CLIENT_ID),
            ("device_code", device_code),
        ])
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("LittleSkin 授权轮询失败：{e}"))?;
    let status = response.status();
    let value: serde_json::Value = response.json().await.unwrap_or_default();
    if !status.is_success() {
        let error = value
            .get("error")
            .and_then(|item| item.as_str())
            .unwrap_or("unknown");
        let description = value
            .get("error_description")
            .and_then(|item| item.as_str())
            .unwrap_or_default();
        let (state, message, terminal) = match error {
            "authorization_pending" => ("pending", "等待你在浏览器中完成 LittleSkin 授权…".to_string(), false),
            "slow_down" => ("slow_down", "轮询过快，已自动降低检查频率".to_string(), false),
            "access_denied" => ("declined", "你取消了 LittleSkin 授权".to_string(), true),
            "expired_token" => ("expired", "授权码已过期，请重新发起登录".to_string(), true),
            "invalid_client" => ("client_error", OAUTH_WHITELIST_HINT.to_string(), true),
            _ => (
                "error",
                if description.is_empty() {
                    error.to_string()
                } else {
                    description.to_string()
                },
                true,
            ),
        };
        return Ok(OauthPoll {
            status: state.into(),
            terminal,
            message,
            account: None,
        });
    }
    let oauth_access = value
        .get("access_token")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "LittleSkin 授权响应缺少 access_token".to_string())?
        .to_string();
    let oauth_refresh = value
        .get("refresh_token")
        .and_then(|item| item.as_str())
        .map(str::to_string);
    let profile = value
        .get("id_token")
        .and_then(|item| item.as_str())
        .and_then(selected_profile_from_id_token)
        .ok_or_else(|| {
            "授权成功，但 ID 令牌没有 selectedProfile。请确认应用申请了 Yggdrasil.PlayerProfiles.Select，并在授权页选择角色。".to_string()
        })?;
    let (access_token, client_token, selected) =
        match create_minecraft_token(&client, &oauth_access, &unsigned_uuid(&profile.id)).await {
            Ok(value) => value,
            Err(message) => {
                return Ok(OauthPoll {
                    status: "error".into(),
                    terminal: true,
                    message,
                    account: None,
                })
            }
        };
    let selected = selected.unwrap_or(profile);
    let metadata = fetch_metadata(&client, DEFAULT_SERVER)
        .await
        .unwrap_or_default();
    let account = build_account(
        DEFAULT_SERVER,
        &metadata,
        &access_token,
        &client_token,
        &selected,
        oauth_refresh.as_deref(),
    )?;
    let view = auth::upsert(app, account)?;
    Ok(OauthPoll {
        status: "success".into(),
        terminal: true,
        message: "LittleSkin 授权登录成功".into(),
        account: Some(view),
    })
}

async fn create_minecraft_token(
    client: &reqwest::Client,
    oauth_access: &str,
    uuid: &str,
) -> Result<(String, String, Option<ProfileView>), String> {
    let response = client
        .post("https://littleskin.cn/api/yggdrasil/authserver/oauth")
        .bearer_auth(oauth_access)
        .json(&serde_json::json!({ "uuid": uuid }))
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("创建 Minecraft 令牌失败：{e}"))?;
    let status = response.status();
    let value: serde_json::Value = response.json().await.unwrap_or_default();
    if !status.is_success() {
        let message = value
            .get("errorMessage")
            .and_then(|item| item.as_str())
            .unwrap_or("无法创建 Minecraft 令牌");
        let hint = match status.as_u16() {
            401 | 403 => "（请确认应用已获得 Yggdrasil.MinecraftToken.Create 权限）",
            _ => "",
        };
        return Err(format!("创建 Minecraft 令牌失败：{message}{hint}"));
    }
    let access = value
        .get("accessToken")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "Minecraft 令牌响应缺少 accessToken".to_string())?
        .to_string();
    let client_token = value
        .get("clientToken")
        .and_then(|item| item.as_str())
        .unwrap_or_default()
        .to_string();
    let selected = value
        .get("selectedProfile")
        .and_then(|item| serde_json::from_value::<RawProfile>(item.clone()).ok())
        .map(|item| ProfileView {
            id: item.id,
            name: item.name,
        });
    Ok((access, client_token, selected))
}

async fn refresh_oauth(
    client: &reqwest::Client,
    refresh_token: &str,
) -> Result<(String, Option<String>), String> {
    let response = client
        .post(OAUTH_TOKEN_URL)
        .header("Accept", "application/json")
        .form(&[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", OAUTH_CLIENT_ID),
        ])
        .timeout(Duration::from_secs(30))
        .send()
        .await
        .map_err(|e| format!("刷新 LittleSkin 授权失败：{e}"))?;
    let status = response.status();
    let value: serde_json::Value = response.json().await.unwrap_or_default();
    if !status.is_success() {
        let error = value
            .get("error")
            .and_then(|item| item.as_str())
            .unwrap_or("unknown");
        let description = value
            .get("error_description")
            .and_then(|item| item.as_str())
            .unwrap_or_default();
        let message = if description.is_empty() {
            error.to_string()
        } else {
            format!("{error}：{description}")
        };
        return Err(message);
    }
    let access = value
        .get("access_token")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "刷新响应缺少 access_token".to_string())?
        .to_string();
    let next_refresh = value
        .get("refresh_token")
        .and_then(|item| item.as_str())
        .map(str::to_string);
    Ok((access, next_refresh))
}

async fn ensure_jar(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = data_dir(app)?.join("authlib");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let client = http_client(app)?;
    let mut latest = None;
    if let Ok(response) = client
        .get(AUTHLIB_LATEST)
        .timeout(Duration::from_secs(20))
        .send()
        .await
    {
        if response.status().is_success() {
            if let Ok(value) = response.json::<serde_json::Value>().await {
                let version = value
                    .get("version")
                    .and_then(|item| item.as_str())
                    .map(str::to_string);
                let download = value
                    .get("download_url")
                    .and_then(|item| item.as_str())
                    .map(str::to_string);
                let sha = value
                    .pointer("/checksums/sha256")
                    .and_then(|item| item.as_str())
                    .map(str::to_string);
                if let (Some(version), Some(download), Some(sha)) = (version, download, sha) {
                    latest = Some((version, download, sha));
                }
            }
        }
    }
    if let Some((version, download, sha)) = latest {
        let url = reqwest::Url::parse(&download)
            .map_err(|_| "authlib-injector 下载地址无效".to_string())?;
        if url.scheme() != "https" || url.host_str() != Some("authlib-injector.yushi.moe") {
            return Err("authlib-injector 下载地址不受信任".into());
        }
        let path = dir.join(format!("authlib-injector-{version}.jar"));
        if let Ok(bytes) = fs::read(&path) {
            if hex::encode(Sha256::digest(&bytes)) == sha {
                return Ok(path);
            }
        }
        let response = client
            .get(&download)
            .timeout(Duration::from_secs(120))
            .send()
            .await
            .map_err(|e| format!("下载 authlib-injector 失败：{e}"))?;
        if !response.status().is_success() {
            return Err(format!(
                "下载 authlib-injector 失败（HTTP {}）",
                response.status().as_u16()
            ));
        }
        let bytes = response.bytes().await.map_err(|e| e.to_string())?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("authlib-injector 文件过大".into());
        }
        if hex::encode(Sha256::digest(&bytes)) != sha {
            return Err("authlib-injector 校验失败，已中止".into());
        }
        write_atomic(&path, &bytes)?;
        return Ok(path);
    }
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) == Some("jar") {
                return Ok(path);
            }
        }
    }
    Err("无法获取 authlib-injector：下载接口不可达，且本地没有缓存".into())
}

pub async fn prepare_launch(
    app: &tauri::AppHandle,
    account: &Account,
) -> Result<(Option<Account>, LaunchAuthlib), String> {
    if account.kind != "yggdrasil" {
        return Err("不是外置登录账户".into());
    }
    let server = normalize_server(&account.server)?;
    let client = http_client(app)?;
    let mut current = account.clone();
    let mut changed = false;
    let access = auth::unprotect(&current.access_token)?;
    match validate(&client, &server, &access, &current.client_token).await {
        Ok(true) => {}
        Ok(false) => {
            match refresh(&client, &server, &access, &current.client_token).await {
                Ok((new_access, new_client)) => {
                    current.access_token = auth::protect(&new_access)?;
                    if !new_client.is_empty() {
                        current.client_token = new_client;
                    }
                    changed = true;
                }
                Err(err) => {
                    if current.refresh_token_enc.is_empty() {
                        return Err(format!("LittleSkin 登录已失效，请重新登录：{err}"));
                    }
                    let oauth_refresh_token = auth::unprotect(&current.refresh_token_enc)?;
                    let (oauth_access, next_refresh) =
                        refresh_oauth(&client, &oauth_refresh_token)
                            .await
                            .map_err(|e| format!("LittleSkin 授权已失效，请重新授权登录：{e}"))?;
                    let (mc_access, mc_client, _) =
                        create_minecraft_token(&client, &oauth_access, &unsigned_uuid(&current.uuid))
                            .await?;
                    current.access_token = auth::protect(&mc_access)?;
                    if !mc_client.is_empty() {
                        current.client_token = mc_client;
                    }
                    if let Some(next) = next_refresh {
                        if !next.is_empty() {
                            current.refresh_token_enc = auth::protect(&next)?;
                        }
                    }
                    changed = true;
                }
            }
        }
        Err(_) => {}
    }
    let prefetched = if current.metadata.is_empty() {
        let value = fetch_metadata(&client, &server).await?;
        current.metadata = value.clone();
        changed = true;
        value
    } else {
        current.metadata.clone()
    };
    let jar = ensure_jar(app).await?;
    Ok((
        if changed { Some(current) } else { None },
        LaunchAuthlib {
            jar,
            server,
            prefetched,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_is_normalized_to_api_root() {
        assert_eq!(
            normalize_server("littleskin.cn").unwrap(),
            DEFAULT_SERVER
        );
        assert_eq!(
            normalize_server("https://littleskin.cn/api/yggdrasil/").unwrap(),
            DEFAULT_SERVER
        );
        assert_eq!(
            normalize_server("").unwrap(),
            DEFAULT_SERVER
        );
        assert!(normalize_server("http://littleskin.cn").is_err());
    }

    #[test]
    fn uuid_is_dashed() {
        assert_eq!(
            dash_uuid("a01e3843e5213998958af459800e4d11"),
            "a01e3843-e521-3998-958a-f459800e4d11"
        );
    }

    #[test]
    fn selected_profile_is_read_from_id_token() {
        let payload = serde_json::json!({
            "selectedProfile": {
                "id": "a01e3843e5213998958af459800e4d11",
                "name": "Steve"
            }
        })
        .to_string();
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(payload.as_bytes());
        let token = format!("header.{encoded}.signature");
        let profile = selected_profile_from_id_token(&token).unwrap();
        assert_eq!(profile.name, "Steve");
        assert_eq!(profile.id, "a01e3843e5213998958af459800e4d11");
        assert!(selected_profile_from_id_token("not-a-jwt").is_none());
    }

    #[test]
    fn whitelist_error_is_explained() {
        let message = oauth_device_error("invalid_client", "Client was not found or not whitelisted");
        assert!(message.contains("白名单"));
        assert!(message.contains("https://open.littleskin.cn/oauth/callback"));
        assert!(message.contains("800"));
    }

    #[test]
    fn uuids_are_normalized_for_the_api() {
        assert_eq!(
            unsigned_uuid("A01E3843-E521-3998-958A-F459800E4D11"),
            "a01e3843e5213998958af459800e4d11"
        );
        assert_eq!(
            unsigned_uuid("a01e3843e5213998958af459800e4d11"),
            "a01e3843e5213998958af459800e4d11"
        );
    }
}
