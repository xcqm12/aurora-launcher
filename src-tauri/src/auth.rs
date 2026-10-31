use crate::{
    data_dir, dpapi_decrypt, dpapi_encrypt, http_client, read_settings, unix_now, write_settings,
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Account {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub uuid: String,
    #[serde(default)]
    pub xuid: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub expires_at: u64,
    #[serde(default)]
    pub refresh_token_enc: String,
    #[serde(default)]
    pub server: String,
    #[serde(default)]
    pub client_token: String,
    #[serde(default)]
    pub metadata: String,
}

#[derive(Debug, Serialize)]
pub struct AccountView {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub uuid: String,
    pub active: bool,
    #[serde(default)]
    pub server: String,
}

impl AccountView {
    fn of(account: &Account, active: bool) -> Self {
        Self {
            id: account.id.clone(),
            kind: account.kind.clone(),
            name: account.name.clone(),
            uuid: account.uuid.clone(),
            active,
            server: account.server.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LaunchAccount {
    pub name: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
    pub client_id: String,
    pub xuid: String,
    pub authlib: Option<crate::yggdrasil::LaunchAuthlib>,
}

#[derive(Debug, Serialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verification_uri_complete: Option<String>,
    pub expires_in: u64,
    pub interval: u64,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct PollResult {
    pub status: String,
    pub account: Option<AccountView>,
    pub message: String,
    pub terminal: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

const STORE_URL: &str = "https://www.minecraft.net/zh-hans/store/minecraft-java-edition";

struct LoginFailure {
    status: &'static str,
    message: String,
    hint: Option<String>,
}

impl LoginFailure {
    fn new(status: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            hint: None,
        }
    }

    fn with_hint(mut self, hint: &str) -> Self {
        self.hint = Some(hint.to_string());
        self
    }
}

impl From<String> for LoginFailure {
    fn from(message: String) -> Self {
        Self::new("error", message)
    }
}

impl From<&str> for LoginFailure {
    fn from(message: &str) -> Self {
        Self::new("error", message)
    }
}

fn device_code_error(status: u16, description: &str) -> String {
    let hint = if description.contains("AADSTS700016") {
        "应用不存在：请确认 Client ID 没有填错，且账户类型支持“个人 Microsoft 账户”。"
    } else if description.contains("AADSTS7000218") || description.contains("AADSTS7000215") {
        "应用未启用公共客户端：Azure 门户 → 身份验证 → 允许公共客户端流，或添加 Web 平台重定向 https://www.microsoft.com/link 。"
    } else if description.contains("AADSTS65001") {
        "应用缺少权限或用户未同意：请在 API 权限中添加 XboxLive.signin 与 offline_access。"
    } else if description.contains("AADSTS700005") {
        "授权请求无效：请重新发起登录；若反复出现请检查应用的账户类型与平台配置。"
    } else if status == 404 {
        "微软返回 404，通常是该 Client ID 未通过 Minecraft 应用审核；可在 https://aka.ms/mce-reviewappid 提交审核。"
    } else {
        "请检查 Client ID、应用的账户类型以及“允许公共客户端流”设置。"
    };
    format!("无法获取设备码（HTTP {status}）：{hint}")
}

fn poll_error(error: &str, description: &str) -> PollResult {
    let (status, message) = match error {
        "authorization_pending" => ("pending", "等待你在浏览器中完成微软登录…".to_string()),
        "slow_down" => ("slow_down", "轮询过快，已自动降低检查频率".to_string()),
        "authorization_declined" => ("declined", "你在浏览器中取消了微软登录授权".to_string()),
        "expired_token" => ("expired", "设备码已过期，请重新点击“微软登录”".to_string()),
        "bad_verification_code" => {
            let message = if description.is_empty() {
                "设备码无效或已被使用，请重新发起登录".to_string()
            } else {
                format!("设备码无效或已被使用，请重新发起登录（{description}）")
            };
            ("invalid_code", message)
        }
        "invalid_client" | "unauthorized_client" => (
            "client_error",
            "Client ID 无效，或应用未启用设备码/公共客户端流程".to_string(),
        ),
        _ => {
            let message = if description.is_empty() {
                error.to_string()
            } else {
                format!("{error}：{description}")
            };
            ("error", message)
        }
    };
    PollResult {
        status: status.into(),
        account: None,
        message,
        terminal: !matches!(status, "pending" | "slow_down"),
        hint: None,
    }
}

#[derive(Debug, Serialize)]
pub struct ClientIdCheck {
    pub ok: bool,
    pub message: String,
}

fn normalize_client_id(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    let bytes = trimmed.as_bytes();
    let valid = bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        });
    if !valid {
        return Err("Client ID 格式不正确，应为 36 位 GUID（8-4-4-4-12）".into());
    }
    Ok(trimmed.to_string())
}

pub async fn check_client_id(
    app: &tauri::AppHandle,
    value: &str,
) -> Result<ClientIdCheck, String> {
    let builtin = value.trim().is_empty();
    let client_id = if builtin {
        client_id(app)?
    } else {
        normalize_client_id(value)?
    };
    let client = http_client(app)?;
    let response = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode")
        .query(&[("mkt", "zh-CN")])
        .header("Accept-Language", "zh-CN")
        .form(&[
            ("client_id", client_id.as_str()),
            ("scope", "XboxLive.signin offline_access"),
        ])
        .send()
        .await
        .map_err(|e| format!("验证 Client ID 失败：{e}"))?;
    let status = response.status();
    if status.is_success() {
        let message = if builtin {
            "内置 Client ID 可用：支持个人账户的设备码登录，且已通过 Mojang appID 白名单审核。".into()
        } else {
            "Client ID 可用：支持个人账户的设备码登录。能否读取 Minecraft 档案取决于该应用是否已通过 Mojang 审核，这一步只能在实际登录时验证。".into()
        };
        return Ok(ClientIdCheck { ok: true, message });
    }
    let value: serde_json::Value = response.json().await.unwrap_or_default();
    let description = value
        .get("error_description")
        .and_then(|item| item.as_str())
        .unwrap_or_default();
    Ok(ClientIdCheck {
        ok: false,
        message: device_code_error(status.as_u16(), description),
    })
}

fn accounts_file(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(data_dir(app)?.join("accounts.json"))
}

pub(crate) fn read_accounts(app: &tauri::AppHandle) -> Vec<Account> {
    let Ok(path) = accounts_file(app) else {
        return Vec::new();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub(crate) fn write_accounts(app: &tauri::AppHandle, accounts: &[Account]) -> Result<(), String> {
    let dir = data_dir(app)?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = accounts_file(app)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(accounts).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(tmp, path).map_err(|e| e.to_string())
}

fn encrypt_secret(value: &str) -> Result<String, String> {
    #[cfg(windows)]
    {
        let bytes = dpapi_encrypt(value)?;
        Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
    }
    #[cfg(not(windows))]
    {
        Ok(base64::engine::general_purpose::STANDARD.encode(value.as_bytes()))
    }
}

fn decrypt_secret(value: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|e| e.to_string())?;
    #[cfg(windows)]
    {
        dpapi_decrypt(&bytes)
    }
    #[cfg(not(windows))]
    {
        String::from_utf8(bytes).map_err(|e| e.to_string())
    }
}

pub(crate) fn protect(value: &str) -> Result<String, String> {
    if value.is_empty() {
        return Ok(String::new());
    }
    Ok(format!("enc:{}", encrypt_secret(value)?))
}

pub(crate) fn unprotect(value: &str) -> Result<String, String> {
    match value.strip_prefix("enc:") {
        Some(rest) => decrypt_secret(rest),
        None => Ok(value.to_string()),
    }
}

pub fn offline_uuid(name: &str) -> String {
    use md5::{Digest, Md5};
    let mut hash = Md5::digest(format!("OfflinePlayer:{name}").as_bytes());
    hash[6] = (hash[6] & 0x0f) | 0x30;
    hash[8] = (hash[8] & 0x3f) | 0x80;
    let hex = hash
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

pub fn list(app: &tauri::AppHandle) -> Vec<AccountView> {
    let active = read_settings(app).active_account;
    read_accounts(app)
        .iter()
        .map(|account| AccountView::of(account, account.id == active))
        .collect()
}

pub(crate) fn upsert(app: &tauri::AppHandle, account: Account) -> Result<AccountView, String> {
    let view = AccountView::of(&account, true);
    let mut accounts = read_accounts(app);
    accounts.retain(|item| item.id != account.id);
    accounts.push(account);
    write_accounts(app, &accounts)?;
    set_active(app, &view.id)?;
    Ok(view)
}

pub fn add_offline(app: &tauri::AppHandle, name: &str) -> Result<AccountView, String> {
    let name = name.trim();
    if name.len() < 1 || name.len() > 16 {
        return Err("离线用户名长度需为 1-16 个字符".into());
    }
    if !name
        .chars()
        .all(|value| value.is_ascii_alphanumeric() || value == '_')
    {
        return Err("离线用户名只允许字母、数字和下划线".into());
    }
    let mut accounts = read_accounts(app);
    if accounts
        .iter()
        .any(|account| account.kind == "offline" && account.name.eq_ignore_ascii_case(name))
    {
        return Err("同名离线账户已存在".into());
    }
    let uuid = offline_uuid(name);
    let account = Account {
        id: format!("offline-{uuid}"),
        kind: "offline".into(),
        name: name.into(),
        uuid: uuid.clone(),
        xuid: String::new(),
        access_token: "0".into(),
        expires_at: u64::MAX,
        refresh_token_enc: String::new(),
        server: String::new(),
        client_token: String::new(),
        metadata: String::new(),
    };
    let view = AccountView::of(&account, true);
    accounts.push(account);
    write_accounts(app, &accounts)?;
    set_active(app, &view.id)?;
    Ok(view)
}

pub fn set_active(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    if !read_accounts(app).iter().any(|account| account.id == id) {
        return Err("账户不存在".into());
    }
    let mut settings = read_settings(app);
    settings.active_account = id.to_string();
    write_settings(app, &settings)
}

pub fn remove(app: &tauri::AppHandle, id: &str) -> Result<(), String> {
    let mut accounts = read_accounts(app);
    accounts.retain(|account| account.id != id);
    write_accounts(app, &accounts)?;
    let mut settings = read_settings(app);
    if settings.active_account == id {
        settings.active_account = accounts
            .first()
            .map(|account| account.id.clone())
            .unwrap_or_default();
        write_settings(app, &settings)?;
    }
    Ok(())
}

/// Aurora 在 Azure 的自有应用（显示名 aurora-launcher），已通过 Mojang appID
/// 白名单审核（2026-10-31 邮件确认）。Client ID 本身不是机密，可作为默认值
/// 内置；设置页与环境变量仍可覆盖。
const DEFAULT_CLIENT_ID: &str = "ae897ba3-93b3-4998-a4fb-b80bad87b538";

fn client_id(app: &tauri::AppHandle) -> Result<String, String> {
    let configured = read_settings(app).ms_client_id;
    if !configured.trim().is_empty() {
        return Ok(configured.trim().to_string());
    }
    if let Ok(value) = std::env::var("AURORA_MS_CLIENT_ID") {
        if !value.trim().is_empty() {
            return Ok(value.trim().to_string());
        }
    }
    if let Some(value) = option_env!("AURORA_MS_CLIENT_ID") {
        if !value.trim().is_empty() {
            return Ok(value.trim().to_string());
        }
    }
    Ok(DEFAULT_CLIENT_ID.to_string())
}

pub async fn start_login(app: &tauri::AppHandle) -> Result<DeviceCode, String> {
    let client_id = client_id(app)?;
    let client = http_client(app)?;
    let response = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode")
        .query(&[("mkt", "zh-CN")])
        .header("Accept-Language", "zh-CN")
        .form(&[
            ("client_id", client_id.as_str()),
            ("scope", "XboxLive.signin offline_access"),
        ])
        .send()
        .await
        .map_err(|e| format!("微软登录请求失败：{e}"))?;
    let status = response.status();
    let value: serde_json::Value = response.json().await.unwrap_or_default();
    if !status.is_success() {
        let description = value
            .get("error_description")
            .and_then(|item| item.as_str())
            .unwrap_or_default();
        return Err(device_code_error(status.as_u16(), description));
    }
    let device_code = value
        .get("device_code")
        .and_then(|item| item.as_str())
        .unwrap_or_default()
        .to_string();
    if device_code.is_empty() {
        return Err("微软响应缺少 device_code，请稍后重试".into());
    }
    Ok(DeviceCode {
        device_code,
        user_code: value
            .get("user_code")
            .and_then(|item| item.as_str())
            .unwrap_or_default()
            .into(),
        verification_uri: value
            .get("verification_uri")
            .and_then(|item| item.as_str())
            .unwrap_or_default()
            .into(),
        verification_uri_complete: value
            .get("verification_uri_complete")
            .and_then(|item| item.as_str())
            .map(|item| item.to_string()),
        expires_in: value
            .get("expires_in")
            .and_then(|item| item.as_u64())
            .unwrap_or(900),
        interval: value
            .get("interval")
            .and_then(|item| item.as_u64())
            .unwrap_or(5),
        message: value
            .get("message")
            .and_then(|item| item.as_str())
            .unwrap_or_default()
            .into(),
    })
}

pub async fn poll_login(app: &tauri::AppHandle, device_code: &str) -> Result<PollResult, String> {
    let client_id = client_id(app)?;
    let client = http_client(app)?;
    let response = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
        .header("Accept-Language", "zh-CN")
        .form(&[
            ("client_id", client_id.as_str()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", device_code),
        ])
        .send()
        .await
        .map_err(|e| format!("微软登录轮询失败：{e}"))?;
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
        return Ok(poll_error(error, description));
    }
    let ms_access = value
        .get("access_token")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "微软响应缺少 access_token".to_string())?;
    let refresh_token = value
        .get("refresh_token")
        .and_then(|item| item.as_str())
        .unwrap_or_default();
    let account = match finish_login(app, &client, ms_access, refresh_token).await {
        Ok(account) => account,
        Err(failure) => {
            return Ok(PollResult {
                status: failure.status.into(),
                account: None,
                message: failure.message,
                terminal: true,
                hint: failure.hint,
            })
        }
    };
    let view = AccountView::of(&account, true);
    let mut accounts = read_accounts(app);
    accounts.retain(|item| item.id != account.id);
    accounts.push(account.clone());
    write_accounts(app, &accounts)?;
    set_active(app, &account.id)?;
    Ok(PollResult {
        status: "success".into(),
        account: Some(view),
        message: "登录成功".into(),
        terminal: true,
        hint: None,
    })
}

async fn finish_login(
    _app: &tauri::AppHandle,
    client: &reqwest::Client,
    ms_access: &str,
    refresh_token: &str,
) -> Result<Account, LoginFailure> {
    let ticket = if ms_access.starts_with("d=") {
        ms_access.to_string()
    } else {
        format!("d={ms_access}")
    };
    let xbox_response = client
        .post("https://user.auth.xboxlive.com/user/authenticate")
        .json(&serde_json::json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": ticket
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        }))
        .send()
        .await
        .map_err(|e| format!("Xbox Live 认证失败：{e}"))?;
    let xbox_status = xbox_response.status();
    let xbox: serde_json::Value = xbox_response.json().await.unwrap_or_default();
    if !xbox_status.is_success() {
        return Err(format!(
            "Xbox Live 认证被拒绝（HTTP {}）",
            xbox_status.as_u16()
        )
        .into());
    }
    let xbl_token = xbox
        .get("Token")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "Xbox 响应缺少 Token".to_string())?;
    let uhs = xbox
        .pointer("/DisplayClaims/xui/0/uhs")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "Xbox 响应缺少 uhs".to_string())?
        .to_string();

    let xsts_response = client
        .post("https://xsts.auth.xboxlive.com/xsts/authorize")
        .json(&serde_json::json!({
            "Properties": {
                "SandboxId": "RETAIL",
                "UserTokens": [xbl_token]
            },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT"
        }))
        .send()
        .await
        .map_err(|e| format!("XSTS 认证失败：{e}"))?;
    let xsts_status = xsts_response.status();
    let xsts: serde_json::Value = xsts_response.json().await.unwrap_or_default();
    if !xsts_status.is_success() {
        let code = xsts.get("XErr").and_then(|item| item.as_u64()).unwrap_or(0);
        let message = match code {
            2148916227 => "该微软账户已被 Xbox Live 封禁",
            2148916233 => "该微软账户没有 Xbox 档案，请先在 Xbox 官网创建档案",
            2148916235 => "该账户所在国家/地区不支持 Xbox Live，请更换网络环境",
            2148916238 => "该账户未满 18 岁，需要家长将其加入 Microsoft 家庭组",
            _ => "XSTS 认证被拒绝，请确认账户可登录 Minecraft",
        };
        return Err(format!("{message}（XErr {code}）").into());
    }
    let xsts_token = xsts
        .get("Token")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "XSTS 响应缺少 Token".to_string())?;
    let xsts_uhs = xsts
        .pointer("/DisplayClaims/xui/0/uhs")
        .and_then(|item| item.as_str())
        .unwrap_or_default();
    if !xsts_uhs.is_empty() && xsts_uhs != uhs {
        return Err("Xbox 与 XSTS 返回的用户标识不一致，请重试".into());
    }
    let xuid = xsts
        .pointer("/DisplayClaims/xui/0/xid")
        .and_then(|item| item.as_str())
        .unwrap_or_default()
        .to_string();

    let mc_response = client
        .post("https://api.minecraftservices.com/authentication/login_with_xbox")
        .json(&serde_json::json!({
            "identityToken": format!("XBL3.0 x={uhs};{xsts_token}")
        }))
        .send()
        .await
        .map_err(|e| format!("Minecraft 登录失败：{e}"))?;
    let mc_status = mc_response.status();
    let mc: serde_json::Value = mc_response.json().await.unwrap_or_default();
    if !mc_status.is_success() {
        let body = mc.to_string();
        let message = if body.contains("ACCOUNT_SUSPENDED") {
            "该 Minecraft 账户已被封禁"
        } else {
            match mc_status.as_u16() {
                429 => "Minecraft 登录过于频繁，请等待几分钟后重试",
                403 => "当前 IP 的登录尝试异常，请关闭或更换代理后重试",
                503 => "Minecraft 登录服务暂时不可用，请稍后重试",
                _ => "Minecraft 登录被拒绝，请在微软登录页选择“使用密码登录”而不是其他方式",
            }
        };
        return Err(format!("{message}（HTTP {}）", mc_status.as_u16()).into());
    }
    let mc_access = mc
        .get("access_token")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "Minecraft 响应缺少 access_token".to_string())?;
    let mc_expires = mc
        .get("expires_in")
        .and_then(|item| item.as_u64())
        .unwrap_or(86400);

    let entitlements_response = client
        .get("https://api.minecraftservices.com/entitlements/mcstore")
        .bearer_auth(mc_access)
        .send()
        .await
        .map_err(|e| format!("检查游戏所有权失败：{e}"))?;
    let entitlements_status = entitlements_response.status();
    if !entitlements_status.is_success() {
        return Err(format!(
            "检查游戏所有权失败（HTTP {}）",
            entitlements_status.as_u16()
        )
        .into());
    }
    let entitlements: serde_json::Value = entitlements_response.json().await.unwrap_or_default();
    if entitlements
        .get("items")
        .and_then(|item| item.as_array())
        .map(|items| items.is_empty())
        .unwrap_or(true)
    {
        return Err(LoginFailure::new(
            "no_license",
            "该账户没有 Minecraft: Java Edition 所有权，或 Game Pass 已过期",
        )
        .with_hint(STORE_URL));
    }

    let profile_response = client
        .get("https://api.minecraftservices.com/minecraft/profile")
        .bearer_auth(mc_access)
        .send()
        .await
        .map_err(|e| format!("获取 Minecraft 档案失败：{e}"))?;
    let profile_status = profile_response.status();
    if profile_status.as_u16() == 404 {
        let license: serde_json::Value = match client
            .get("https://api.minecraftservices.com/entitlements/license")
            .bearer_auth(mc_access)
            .send()
            .await
        {
            Ok(response) => response.json().await.unwrap_or_default(),
            Err(_) => serde_json::Value::default(),
        };
        let owns_game = license
            .get("items")
            .and_then(|item| item.as_array())
            .map(|items| {
                items.iter().any(|item| {
                    item.get("name").and_then(|value| value.as_str()) == Some("game_minecraft")
                })
            })
            .unwrap_or(false);
        return Err(if owns_game {
            LoginFailure::new(
                "no_profile",
                "该账户拥有游戏但还没有 Java 版档案，请先在 minecraft.net 创建档案",
            )
        } else {
            LoginFailure::new("no_license", "该账户没有 Minecraft: Java Edition 许可证")
                .with_hint(STORE_URL)
        });
    }
    if !profile_status.is_success() {
        let message = match profile_status.as_u16() {
            429 => "查询 Minecraft 档案过于频繁，请稍后重试",
            _ => "获取 Minecraft 档案失败",
        };
        return Err(format!("{message}（HTTP {}）", profile_status.as_u16()).into());
    }
    let profile: serde_json::Value = profile_response.json().await.map_err(|e| e.to_string())?;
    let name = profile
        .get("name")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "档案缺少用户名".to_string())?
        .to_string();
    let uuid = profile
        .get("id")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "档案缺少 UUID".to_string())?
        .to_string();
    let refresh_enc = if refresh_token.is_empty() {
        String::new()
    } else {
        encrypt_secret(refresh_token)?
    };
    Ok(Account {
        id: format!("ms-{uuid}"),
        kind: "microsoft".into(),
        name,
        uuid,
        xuid,
        access_token: protect(mc_access)?,
        expires_at: unix_now() + mc_expires.max(60) - 30,
        refresh_token_enc: refresh_enc,
        server: String::new(),
        client_token: String::new(),
        metadata: String::new(),
    })
}

async fn refresh(app: &tauri::AppHandle, account: &mut Account) -> Result<(), String> {
    if account.kind != "microsoft" || account.refresh_token_enc.is_empty() {
        return Err("该账户无法刷新，请重新登录".into());
    }
    let refresh_token = decrypt_secret(&account.refresh_token_enc)?;
    let client_id = client_id(app)?;
    let client = http_client(app)?;
    let refresh_response = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
        .form(&[
            ("client_id", client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.as_str()),
            ("scope", "XboxLive.signin offline_access"),
        ])
        .send()
        .await
        .map_err(|e| format!("刷新微软令牌失败：{e}"))?;
    let refresh_status = refresh_response.status();
    let value: serde_json::Value = refresh_response.json().await.unwrap_or_default();
    if !refresh_status.is_success() {
        let error = value
            .get("error")
            .and_then(|item| item.as_str())
            .unwrap_or("unknown");
        let description = value
            .get("error_description")
            .and_then(|item| item.as_str())
            .unwrap_or_default();
        if error == "invalid_grant" || description.contains("AADSTS70000") {
            return Err("微软登录凭据已过期，请重新进行微软登录".into());
        }
        return Err(format!("刷新微软令牌失败：{error}"));
    }
    let ms_access = value
        .get("access_token")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "刷新响应缺少 access_token".to_string())?;
    let new_refresh = value
        .get("refresh_token")
        .and_then(|item| item.as_str())
        .unwrap_or(&refresh_token);
    let mut updated = finish_login(app, &client, ms_access, new_refresh)
        .await
        .map_err(|failure| failure.message)?;
    updated.id = account.id.clone();
    *account = updated;
    let mut accounts = read_accounts(app);
    accounts.retain(|item| item.id != account.id);
    accounts.push(account.clone());
    write_accounts(app, &accounts)?;
    Ok(())
}

pub async fn launch_account(app: &tauri::AppHandle) -> Result<LaunchAccount, String> {
    let settings = read_settings(app);
    let mut accounts = read_accounts(app);
    if accounts.is_empty() {
        let name = "Player".to_string();
        let uuid = offline_uuid(&name);
        return Ok(LaunchAccount {
            name,
            uuid,
            access_token: "0".into(),
            user_type: "legacy".into(),
            client_id: String::new(),
            xuid: String::new(),
            authlib: None,
        });
    }
    let index = accounts
        .iter()
        .position(|account| account.id == settings.active_account)
        .unwrap_or(0);
    let mut account = accounts[index].clone();
    if account.kind == "microsoft" && account.expires_at <= unix_now() {
        refresh(app, &mut account).await?;
        accounts[index] = account.clone();
    }
    let mut authlib = None;
    if account.kind == "yggdrasil" {
        let (updated, info) = crate::yggdrasil::prepare_launch(app, &account).await?;
        if let Some(updated) = updated {
            accounts[index] = updated.clone();
            account = updated;
            write_accounts(app, &accounts)?;
        }
        authlib = Some(info);
    }
    let access_token = if account.kind == "offline" {
        "0".into()
    } else {
        unprotect(&account.access_token)?
    };
    Ok(LaunchAccount {
        name: account.name,
        uuid: account.uuid,
        access_token,
        user_type: if account.kind == "offline" {
            "legacy".into()
        } else {
            "msa".into()
        },
        client_id: if account.kind == "microsoft" {
            client_id(app).unwrap_or_default()
        } else {
            String::new()
        },
        xuid: account.xuid,
        authlib,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_uuid_matches_java_algorithm() {
        assert_eq!(
            offline_uuid("Player"),
            "a01e3843-e521-3998-958a-f459800e4d11"
        );
    }

    #[test]
    fn device_code_errors_point_at_the_fix() {
        let public_client = device_code_error(400, "AADSTS7000218: request body must contain client_assertion");
        assert!(public_client.contains("公共客户端"));
        assert!(public_client.contains("https://www.microsoft.com/link"));

        let missing_app = device_code_error(400, "AADSTS700016: Application not found");
        assert!(missing_app.contains("应用不存在"));

        let unapproved = device_code_error(404, "");
        assert!(unapproved.contains("aka.ms/mce-reviewappid"));
    }

    #[test]
    fn client_id_must_be_a_guid() {
        assert_eq!(
            normalize_client_id(" 11111111-1111-1111-1111-111111111111 ").unwrap(),
            "11111111-1111-1111-1111-111111111111"
        );
        assert!(normalize_client_id("11111111-1111-1111-1111-11111111111").is_err());
        assert!(normalize_client_id("zzzz1111-1111-1111-1111-111111111111").is_err());
        assert!(normalize_client_id("").is_err());
    }

    #[test]
    fn poll_status_is_terminal_except_pending() {
        assert!(!poll_error("authorization_pending", "").terminal);
        assert!(!poll_error("slow_down", "").terminal);

        let expired = poll_error("expired_token", "");
        assert!(expired.terminal);
        assert_eq!(expired.status, "expired");

        let reused = poll_error("bad_verification_code", "code already used");
        assert!(reused.terminal);
        assert_eq!(reused.status, "invalid_code");
    }
}
