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
}

#[derive(Debug, Serialize)]
pub struct AccountView {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub uuid: String,
    pub active: bool,
}

#[derive(Debug, Clone)]
pub struct LaunchAccount {
    pub name: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
    pub client_id: String,
    pub xuid: String,
}

#[derive(Debug, Serialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
    pub message: String,
}

#[derive(Debug, Serialize)]
pub struct PollResult {
    pub status: String,
    pub account: Option<AccountView>,
    pub message: String,
}

fn accounts_file(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(data_dir(app)?.join("accounts.json"))
}

fn read_accounts(app: &tauri::AppHandle) -> Vec<Account> {
    let Ok(path) = accounts_file(app) else {
        return Vec::new();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_accounts(app: &tauri::AppHandle, accounts: &[Account]) -> Result<(), String> {
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
        .into_iter()
        .map(|account| AccountView {
            active: account.id == active,
            id: account.id,
            kind: account.kind,
            name: account.name,
            uuid: account.uuid,
        })
        .collect()
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
    };
    let id = account.id.clone();
    accounts.push(account);
    write_accounts(app, &accounts)?;
    set_active(app, &id)?;
    Ok(AccountView {
        id,
        kind: "offline".into(),
        name: name.into(),
        uuid,
        active: true,
    })
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
    Err("未配置微软 Client ID。请先在 Azure 注册公共客户端应用，并在设置中填写 Client ID。".into())
}

pub async fn start_login(app: &tauri::AppHandle) -> Result<DeviceCode, String> {
    let client_id = client_id(app)?;
    let client = http_client(app)?;
    let response = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode")
        .form(&[
            ("client_id", client_id.as_str()),
            ("scope", "XboxLive.signin offline_access"),
        ])
        .send()
        .await
        .map_err(|e| format!("微软登录请求失败：{e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "微软设备码接口返回 HTTP {}，请检查 Client ID 是否为公共客户端",
            response.status().as_u16()
        ));
    }
    let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    Ok(DeviceCode {
        device_code: value
            .get("device_code")
            .and_then(|item| item.as_str())
            .unwrap_or_default()
            .into(),
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
        .form(&[
            ("client_id", client_id.as_str()),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("device_code", device_code),
        ])
        .send()
        .await
        .map_err(|e| format!("微软登录轮询失败：{e}"))?;
    let status = response.status();
    let value: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    if !status.is_success() {
        let error = value
            .get("error")
            .and_then(|item| item.as_str())
            .unwrap_or("unknown");
        return Ok(PollResult {
            status: match error {
                "authorization_pending" => "pending",
                "slow_down" => "slow_down",
                "authorization_declined" => "declined",
                "expired_token" => "expired",
                _ => "error",
            }
            .into(),
            account: None,
            message: value
                .get("error_description")
                .and_then(|item| item.as_str())
                .unwrap_or(error)
                .to_string(),
        });
    }
    let ms_access = value
        .get("access_token")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "微软响应缺少 access_token".to_string())?;
    let refresh_token = value
        .get("refresh_token")
        .and_then(|item| item.as_str())
        .unwrap_or_default();
    let expires_in = value
        .get("expires_in")
        .and_then(|item| item.as_u64())
        .unwrap_or(3600);
    let account = finish_login(app, &client, ms_access, refresh_token, expires_in).await?;
    let view = AccountView {
        active: true,
        id: account.id.clone(),
        kind: account.kind.clone(),
        name: account.name.clone(),
        uuid: account.uuid.clone(),
    };
    let mut accounts = read_accounts(app);
    accounts.retain(|item| item.id != account.id);
    accounts.push(account.clone());
    write_accounts(app, &accounts)?;
    set_active(app, &account.id)?;
    Ok(PollResult {
        status: "success".into(),
        account: Some(view),
        message: "登录成功".into(),
    })
}

async fn finish_login(
    _app: &tauri::AppHandle,
    client: &reqwest::Client,
    ms_access: &str,
    refresh_token: &str,
    expires_in: u64,
) -> Result<Account, String> {
    let xbox: serde_json::Value = client
        .post("https://user.auth.xboxlive.com/user/authenticate")
        .json(&serde_json::json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={ms_access}")
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT"
        }))
        .send()
        .await
        .map_err(|e| format!("Xbox Live 认证失败：{e}"))?
        .error_for_status()
        .map_err(|_| "Xbox Live 认证被拒绝".to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let xbl_token = xbox
        .get("Token")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "Xbox 响应缺少 Token".to_string())?;
    let uhs = xbox
        .pointer("/DisplayClaims/xui/0/uhs")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "Xbox 响应缺少 uhs".to_string())?;
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
    if !xsts_response.status().is_success() {
        let value: serde_json::Value = xsts_response.json().await.unwrap_or_default();
        let code = value
            .get("XErr")
            .and_then(|item| item.as_u64())
            .unwrap_or(0);
        let message = match code {
            2148916233 => "该微软账户没有 Xbox 档案，请先在 Xbox 官网创建档案",
            2148916238 => "该账户是儿童账户，需要家长同意",
            _ => "XSTS 认证被拒绝，请确认账户可登录 Minecraft",
        };
        return Err(message.into());
    }
    let xsts: serde_json::Value = xsts_response.json().await.map_err(|e| e.to_string())?;
    let xsts_token = xsts
        .get("Token")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "XSTS 响应缺少 Token".to_string())?;
    let xuid = xsts
        .pointer("/DisplayClaims/xui/0/xid")
        .and_then(|item| item.as_str())
        .unwrap_or_default()
        .to_string();
    let mc: serde_json::Value = client
        .post("https://api.minecraftservices.com/authentication/login_with_xbox")
        .json(&serde_json::json!({
            "identityToken": format!("XBL3.0 x={uhs};{xsts_token}")
        }))
        .send()
        .await
        .map_err(|e| format!("Minecraft 登录失败：{e}"))?
        .error_for_status()
        .map_err(|_| "Minecraft 登录被拒绝，请确认账号拥有游戏".to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let mc_access = mc
        .get("access_token")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "Minecraft 响应缺少 access_token".to_string())?;
    let profile_response = client
        .get("https://api.minecraftservices.com/minecraft/profile")
        .bearer_auth(mc_access)
        .send()
        .await
        .map_err(|e| format!("获取 Minecraft 档案失败：{e}"))?;
    if !profile_response.status().is_success() {
        return Err("该账户没有 Minecraft: Java Edition 档案或游戏所有权".into());
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
        access_token: mc_access.to_string(),
        expires_at: unix_now() + expires_in.max(60) - 30,
        refresh_token_enc: refresh_enc,
    })
}

async fn refresh(app: &tauri::AppHandle, account: &mut Account) -> Result<(), String> {
    if account.kind != "microsoft" || account.refresh_token_enc.is_empty() {
        return Err("该账户无法刷新，请重新登录".into());
    }
    let refresh_token = decrypt_secret(&account.refresh_token_enc)?;
    let client_id = client_id(app)?;
    let client = http_client(app)?;
    let value: serde_json::Value = client
        .post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
        .form(&[
            ("client_id", client_id.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token.as_str()),
            ("scope", "XboxLive.signin offline_access"),
        ])
        .send()
        .await
        .map_err(|e| format!("刷新微软令牌失败：{e}"))?
        .error_for_status()
        .map_err(|_| "刷新微软令牌被拒绝，请重新登录".to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let ms_access = value
        .get("access_token")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "刷新响应缺少 access_token".to_string())?;
    let new_refresh = value
        .get("refresh_token")
        .and_then(|item| item.as_str())
        .unwrap_or(&refresh_token);
    let expires_in = value
        .get("expires_in")
        .and_then(|item| item.as_u64())
        .unwrap_or(3600);
    let mut updated = finish_login(app, &client, ms_access, new_refresh, expires_in).await?;
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
    Ok(LaunchAccount {
        name: account.name,
        uuid: account.uuid,
        access_token: if account.kind == "microsoft" {
            account.access_token
        } else {
            "0".into()
        },
        user_type: if account.kind == "microsoft" {
            "msa".into()
        } else {
            "legacy".into()
        },
        client_id: if account.kind == "microsoft" {
            client_id(app).unwrap_or_default()
        } else {
            String::new()
        },
        xuid: account.xuid,
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
}
