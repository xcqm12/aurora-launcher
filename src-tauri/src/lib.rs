use base64::Engine;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::Manager;

#[derive(Debug, Serialize)]
struct AppInfo {
    name: String,
    version: String,
    data_dir: String,
    curseforge_configured: bool,
    curseforge_mode: String,
    region_locked: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Instance {
    id: String,
    name: String,
    minecraft_version: String,
    loader: String,
    loader_version: Option<String>,
    directory: String,
    state: String,
    #[serde(default)]
    java_path: Option<String>,
    #[serde(default = "default_memory")]
    memory_mb: u32,
    #[serde(default)]
    created_at: u64,
}

fn default_memory() -> u32 {
    4096
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct AppSettings {
    #[serde(default)]
    proxy: String,
    #[serde(default)]
    sakura_enabled: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            proxy: String::new(),
            sakura_enabled: false,
        }
    }
}

#[derive(Debug, Serialize)]
struct JavaRuntime {
    path: String,
    version: String,
    vendor: String,
}

#[derive(Debug, Serialize)]
struct SearchResult {
    source: String,
    id: String,
    name: String,
    description: String,
    icon_url: Option<String>,
    downloads: u64,
    url: String,
}

#[derive(Debug, Serialize)]
struct LoaderInfo {
    id: String,
    name: String,
    kind: String,
    support: String,
}

#[derive(Debug, Serialize)]
struct VersionEntry {
    id: String,
    kind: String,
    url: String,
    release_time: String,
}

#[derive(Debug, Serialize)]
struct VersionManifest {
    latest_release: String,
    latest_snapshot: String,
    versions: Vec<VersionEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ResourceFile {
    filename: String,
    url: String,
    primary: bool,
    size: u64,
    hashes: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct ResourceVersion {
    id: String,
    name: String,
    version_number: String,
    game_versions: Vec<String>,
    loaders: Vec<String>,
    files: Vec<ResourceFile>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CurseForgeFile {
    id: u64,
    mod_id: u64,
    display_name: String,
    file_name: String,
    file_length: u64,
    download_url: Option<String>,
    release_type: String,
    game_versions: Vec<String>,
    mod_loader: Option<String>,
    sha1: Option<String>,
}

fn data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| e.to_string())
}

fn instances_file(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("instances.json"))
}

fn settings_file(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("settings.json"))
}

fn read_settings(app: &tauri::AppHandle) -> AppSettings {
    let Ok(path) = settings_file(app) else {
        return AppSettings::default();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_settings(app: &tauri::AppHandle, settings: &AppSettings) -> Result<(), String> {
    let dir = data_dir(app)?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = settings_file(app)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(tmp, path).map_err(|e| e.to_string())
}

fn normalized_proxy(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    let parsed = reqwest::Url::parse(trimmed).map_err(|_| "代理地址必须是完整 URL".to_string())?;
    match parsed.scheme() {
        "http" | "https" => Ok(trimmed.to_string()),
        _ => Err("代理目前支持 http:// 或 https://".into()),
    }
}

fn build_client(proxy: &str) -> Result<reqwest::Client, String> {
    let mut builder = reqwest::Client::builder()
        .user_agent("AuroraLauncher/0.2")
        .timeout(Duration::from_secs(45))
        .connect_timeout(Duration::from_secs(15));
    let proxy = normalized_proxy(proxy)?;
    if !proxy.is_empty() {
        builder =
            builder.proxy(reqwest::Proxy::all(&proxy).map_err(|e| format!("代理配置失败：{e}"))?);
    }
    builder.build().map_err(|e| e.to_string())
}

fn http_client(app: &tauri::AppHandle) -> Result<reqwest::Client, String> {
    build_client(&read_settings(app).proxy)
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}

fn region_lock_enabled() -> bool {
    option_env!("AURORA_REGION_LOCK") == Some("CN")
}

#[cfg(windows)]
fn current_region() -> Option<String> {
    #[link(name = "Kernel32")]
    extern "system" {
        fn GetUserDefaultGeoName(geonames: *mut u16, count: i32) -> i32;
    }
    let mut buffer = [0u16; 32];
    let length = unsafe { GetUserDefaultGeoName(buffer.as_mut_ptr(), buffer.len() as i32) };
    if length <= 1 {
        return None;
    }
    Some(String::from_utf16_lossy(&buffer[..(length as usize - 1)]))
}

#[cfg(not(windows))]
fn current_region() -> Option<String> {
    None
}

fn region_blocked() -> bool {
    region_lock_enabled() && current_region().as_deref() == Some("CN")
}

fn ensure_download_allowed() -> Result<(), String> {
    if region_blocked() {
        return Err("此发行版在你所在地区不提供资源下载".into());
    }
    Ok(())
}

fn write_instance_meta(instance: &Instance) -> Result<(), String> {
    let root = PathBuf::from(&instance.directory);
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    let path = root.join("instance.json");
    let tmp = path.with_extension("json.tmp");
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(instance).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(tmp, path).map_err(|e| e.to_string())
}

fn read_instances(app: &tauri::AppHandle) -> Result<Vec<Instance>, String> {
    let path = instances_file(app)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

fn write_instances(app: &tauri::AppHandle, instances: &[Instance]) -> Result<(), String> {
    let dir = data_dir(app)?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = instances_file(app)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(instances).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    fs::rename(tmp, path).map_err(|e| e.to_string())
}

fn safe_id(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn secret_file(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("curseforge.key"))
}

fn curseforge_key(app: &tauri::AppHandle) -> Result<String, String> {
    if let Ok(key) = std::env::var("CURSEFORGE_API_KEY") {
        if !key.trim().is_empty() {
            return Ok(key);
        }
    }
    let path = secret_file(app)?;
    if !path.exists() {
        return Err("未配置 CurseForge API Key。请在设置中添加你有权使用的 Key。".into());
    }
    let encrypted = fs::read(path).map_err(|e| format!("无法读取 CurseForge 配置：{e}"))?;
    #[cfg(windows)]
    {
        dpapi_decrypt(&encrypted).map_err(|e| format!("无法解密 CurseForge 配置：{e}"))
    }
    #[cfg(not(windows))]
    {
        String::from_utf8(encrypted).map_err(|e| e.to_string())
    }
}

#[cfg(windows)]
fn dpapi_encrypt(value: &str) -> Result<Vec<u8>, String> {
    use std::{ffi::c_void, ptr};
    #[repr(C)]
    struct Blob {
        size: u32,
        data: *mut u8,
    }
    #[link(name = "Crypt32")]
    extern "system" {
        fn CryptProtectData(
            input: *const Blob,
            description: *const u16,
            entropy: *const Blob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
    }
    #[link(name = "Kernel32")]
    extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }
    let mut output = Blob {
        size: 0,
        data: ptr::null_mut(),
    };
    let input = Blob {
        size: value.len() as u32,
        data: value.as_ptr() as *mut u8,
    };
    let ok = unsafe {
        CryptProtectData(
            &input,
            ptr::null(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
            0x1,
            &mut output,
        )
    };
    if ok == 0 {
        return Err("Windows 凭据加密调用失败".into());
    }
    let bytes = unsafe { std::slice::from_raw_parts(output.data, output.size as usize).to_vec() };
    unsafe {
        LocalFree(output.data as *mut c_void);
    }
    Ok(bytes)
}

#[cfg(windows)]
fn dpapi_decrypt(value: &[u8]) -> Result<String, String> {
    use std::{ffi::c_void, ptr};
    #[repr(C)]
    struct Blob {
        size: u32,
        data: *mut u8,
    }
    #[link(name = "Crypt32")]
    extern "system" {
        fn CryptUnprotectData(
            input: *const Blob,
            description: *mut *mut u16,
            entropy: *const Blob,
            reserved: *mut c_void,
            prompt: *mut c_void,
            flags: u32,
            output: *mut Blob,
        ) -> i32;
    }
    #[link(name = "Kernel32")]
    extern "system" {
        fn LocalFree(memory: *mut c_void) -> *mut c_void;
    }
    let mut output = Blob {
        size: 0,
        data: ptr::null_mut(),
    };
    let input = Blob {
        size: value.len() as u32,
        data: value.as_ptr() as *mut u8,
    };
    let ok = unsafe {
        CryptUnprotectData(
            &input,
            ptr::null_mut(),
            ptr::null(),
            ptr::null_mut(),
            ptr::null_mut(),
            0x1,
            &mut output,
        )
    };
    if ok == 0 {
        return Err("Windows 凭据解密调用失败".into());
    }
    let bytes = unsafe { std::slice::from_raw_parts(output.data, output.size as usize).to_vec() };
    unsafe {
        LocalFree(output.data as *mut c_void);
    }
    String::from_utf8(bytes).map_err(|e| e.to_string())
}

fn curseforge_proxy() -> Option<String> {
    std::env::var("AURORA_CURSEFORGE_PROXY")
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| value.starts_with("https://"))
}

#[tauri::command]
fn app_info(app: tauri::AppHandle) -> Result<AppInfo, String> {
    let dir = data_dir(&app)?;
    let env_key = std::env::var("CURSEFORGE_API_KEY")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);
    let local_key = secret_file(&app)?.exists();
    let mode = if curseforge_proxy().is_some() {
        "proxy"
    } else if env_key {
        "environment"
    } else if local_key {
        "local"
    } else {
        "disabled"
    };
    Ok(AppInfo {
        name: "Aurora Launcher".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        data_dir: dir.display().to_string(),
        curseforge_configured: mode != "disabled",
        curseforge_mode: mode.into(),
        region_locked: region_blocked(),
    })
}

#[tauri::command]
fn set_curseforge_key(app: tauri::AppHandle, key: String) -> Result<(), String> {
    let trimmed = key.trim();
    if trimmed.len() < 20 || trimmed.len() > 512 {
        return Err("API Key 格式无效".into());
    }
    let dir = data_dir(&app)?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = secret_file(&app)?;
    #[cfg(windows)]
    let bytes = dpapi_encrypt(trimmed)?;
    #[cfg(not(windows))]
    let bytes = trimmed.as_bytes().to_vec();
    fs::write(path, bytes).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn clear_curseforge_key(app: tauri::AppHandle) -> Result<(), String> {
    let path = secret_file(&app)?;
    if path.exists() {
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn list_instances(app: tauri::AppHandle) -> Result<Vec<Instance>, String> {
    read_instances(&app)
}

#[tauri::command]
fn create_instance(
    app: tauri::AppHandle,
    name: String,
    minecraft_version: String,
    loader: String,
    loader_version: Option<String>,
) -> Result<Instance, String> {
    if name.trim().is_empty() || minecraft_version.trim().is_empty() {
        return Err("名称和 Minecraft 版本不能为空".into());
    }
    let allowed = [
        "vanilla",
        "fabric",
        "legacyfabric",
        "forge",
        "neoforge",
        "optifine",
        "cleanroom",
        "labymod",
        "liteloader",
    ];
    if !allowed.contains(&loader.as_str()) {
        return Err("不支持的加载器类型".into());
    }
    let id = format!("{}-{}", safe_id(&name.to_lowercase()), chrono_like_stamp());
    let root = data_dir(&app)?.join("instances").join(&id);
    fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    for dir in [
        "mods",
        "resourcepacks",
        "shaderpacks",
        "saves",
        "datapacks",
        "game/versions",
        "game/libraries",
        "game/assets",
        "logs",
        "config",
        "downloads",
    ] {
        fs::create_dir_all(root.join(dir)).map_err(|e| e.to_string())?;
    }
    let instance = Instance {
        id,
        name,
        minecraft_version,
        loader,
        loader_version,
        directory: root.display().to_string(),
        state: "ready".into(),
        java_path: None,
        memory_mb: default_memory(),
        created_at: unix_now(),
    };
    write_instance_meta(&instance)?;
    let mut all = read_instances(&app)?;
    all.push(instance.clone());
    write_instances(&app, &all)?;
    Ok(instance)
}

#[tauri::command]
fn update_instance(
    app: tauri::AppHandle,
    id: String,
    name: Option<String>,
    loader_version: Option<String>,
    java_path: Option<String>,
    memory_mb: Option<u32>,
) -> Result<Instance, String> {
    let mut all = read_instances(&app)?;
    let instance = all
        .iter_mut()
        .find(|item| item.id == id)
        .ok_or_else(|| "实例不存在".to_string())?;
    if let Some(name) = name {
        if name.trim().is_empty() {
            return Err("实例名称不能为空".into());
        }
        instance.name = name;
    }
    if let Some(version) = loader_version {
        instance.loader_version = Some(version);
    }
    if let Some(path) = java_path {
        instance.java_path = Some(path);
    }
    if let Some(memory) = memory_mb {
        instance.memory_mb = memory.clamp(512, 32768);
    }
    let updated = instance.clone();
    write_instances(&app, &all)?;
    write_instance_meta(&updated)?;
    Ok(updated)
}

#[tauri::command]
fn open_external(url: String) -> Result<(), String> {
    let parsed = reqwest::Url::parse(&url).map_err(|_| "链接无效".to_string())?;
    if parsed.scheme() != "https" {
        return Err("只允许打开 HTTPS 链接".into());
    }
    tauri_plugin_opener::open_url(&url, None::<&str>).map_err(|e| format!("无法打开链接：{e}"))
}

#[tauri::command]
fn open_instance_folder(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let instance = read_instances(&app)?
        .into_iter()
        .find(|item| item.id == id)
        .ok_or_else(|| "实例不存在".to_string())?;
    tauri_plugin_opener::open_path(instance.directory, None::<&str>)
        .map_err(|e| format!("无法打开实例目录：{e}"))
}

fn chrono_like_stamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".into())
}

#[tauri::command]
fn delete_instance(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let mut all = read_instances(&app)?;
    let found = all
        .iter()
        .find(|x| x.id == id)
        .cloned()
        .ok_or_else(|| "实例不存在".to_string())?;
    all.retain(|x| x.id != id);
    write_instances(&app, &all)?;
    let dir = PathBuf::from(found.directory);
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn list_loaders() -> Vec<LoaderInfo> {
    vec![
        LoaderInfo {
            id: "vanilla".into(),
            name: "原版 Minecraft".into(),
            kind: "official".into(),
            support: "official metadata".into(),
        },
        LoaderInfo {
            id: "fabric".into(),
            name: "Fabric".into(),
            kind: "loader".into(),
            support: "official Fabric meta API".into(),
        },
        LoaderInfo {
            id: "legacyfabric".into(),
            name: "Legacy Fabric".into(),
            kind: "loader".into(),
            support: "adapter / import".into(),
        },
        LoaderInfo {
            id: "forge".into(),
            name: "Forge".into(),
            kind: "loader".into(),
            support: "installer adapter".into(),
        },
        LoaderInfo {
            id: "neoforge".into(),
            name: "NeoForge".into(),
            kind: "loader".into(),
            support: "installer adapter".into(),
        },
        LoaderInfo {
            id: "optifine".into(),
            name: "OptiFine".into(),
            kind: "special".into(),
            support: "manual import / official installer".into(),
        },
        LoaderInfo {
            id: "cleanroom".into(),
            name: "Cleanroom".into(),
            kind: "special".into(),
            support: "manual import".into(),
        },
        LoaderInfo {
            id: "labymod".into(),
            name: "LabyMod".into(),
            kind: "client".into(),
            support: "official installer".into(),
        },
        LoaderInfo {
            id: "liteloader".into(),
            name: "LiteLoader".into(),
            kind: "legacy".into(),
            support: "manual import / legacy".into(),
        },
    ]
}

#[tauri::command]
fn detect_java() -> Vec<JavaRuntime> {
    let output = Command::new("java").arg("-version").output();
    match output {
        Ok(out) => {
            let raw = String::from_utf8_lossy(&out.stderr).to_string();
            let version = raw
                .lines()
                .find(|l| l.contains("version"))
                .unwrap_or("unknown")
                .trim()
                .to_string();
            vec![JavaRuntime {
                path: "java".into(),
                version,
                vendor: "system".into(),
            }]
        }
        Err(_) => Vec::new(),
    }
}

#[tauri::command]
async fn minecraft_manifest(app: tauri::AppHandle) -> Result<VersionManifest, String> {
    let response: serde_json::Value = http_client(&app)?
        .get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let latest_release = response
        .pointer("/latest/release")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let latest_snapshot = response
        .pointer("/latest/snapshot")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let versions = response
        .get("versions")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .map(|v| VersionEntry {
            id: v
                .get("id")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .into(),
            kind: v
                .get("type")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .into(),
            url: v
                .get("url")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .into(),
            release_time: v
                .get("releaseTime")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .into(),
        })
        .collect();
    Ok(VersionManifest {
        latest_release,
        latest_snapshot,
        versions,
    })
}

#[tauri::command]
async fn search_modrinth(
    app: tauri::AppHandle,
    query: String,
    project_type: Option<String>,
    game_version: Option<String>,
) -> Result<Vec<SearchResult>, String> {
    let mut request = http_client(&app)?
        .get("https://api.modrinth.com/v2/search")
        .query(&[("query", query.as_str()), ("limit", "24")]);
    if let Some(kind) = project_type {
        request = request.query(&[("facets", format!("[[\"project_type:{}\"]]", kind))]);
    }
    if let Some(version) = game_version {
        request = request.query(&[("facets", format!("[[\"versions:{}\"]]", version))]);
    }
    let response: serde_json::Value = request
        .header("User-Agent", "AuroraLauncher/0.1 (launcher)")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(response
        .get("hits")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .map(|x| SearchResult {
            source: "Modrinth".into(),
            id: x
                .get("project_id")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .into(),
            name: x
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .into(),
            description: x
                .get("description")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .into(),
            icon_url: x
                .get("icon_url")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            downloads: x
                .get("downloads")
                .and_then(|v| v.as_u64())
                .unwrap_or_default(),
            url: format!(
                "https://modrinth.com/project/{}",
                x.get("slug").and_then(|v| v.as_str()).unwrap_or_default()
            ),
        })
        .collect())
}

#[tauri::command]
async fn search_curseforge(
    app: tauri::AppHandle,
    query: String,
    game_version: Option<String>,
    class_id: Option<u32>,
) -> Result<Vec<SearchResult>, String> {
    let proxy = curseforge_proxy();
    let endpoint = proxy
        .as_ref()
        .map(|base| format!("{base}/search"))
        .unwrap_or_else(|| "https://api.curseforge.com/v1/mods/search".into());
    let mut request = http_client(&app)?.get(endpoint).query(&[
        ("gameId", "432"),
        ("searchFilter", query.as_str()),
        ("pageSize", "24"),
    ]);
    if proxy.is_none() {
        request = request.header("x-api-key", curseforge_key(&app)?);
    }
    if let Some(v) = game_version {
        request = request.query(&[("gameVersion", v)]);
    }
    if let Some(c) = class_id {
        request = request.query(&[("classId", c.to_string())]);
    }
    let response = request
        .header("User-Agent", "AuroraLauncher/0.1")
        .send()
        .await
        .map_err(|e| format!("CurseForge 请求失败：{e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "CurseForge 返回 HTTP {}。请在 console.curseforge.com 生成 API Key，并确认它可访问官方 API。",
            response.status().as_u16()
        ));
    }
    let response: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    Ok(response
        .get("data")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .map(|x| SearchResult {
            source: "CurseForge".into(),
            id: x
                .get("id")
                .and_then(|v| v.as_u64())
                .unwrap_or_default()
                .to_string(),
            name: x
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .into(),
            description: x
                .get("summary")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .into(),
            icon_url: x
                .get("logo")
                .and_then(|v| v.get("url"))
                .and_then(|v| v.as_str())
                .map(str::to_string),
            downloads: x
                .get("downloadCount")
                .and_then(|v| v.as_u64())
                .unwrap_or_default(),
            url: x
                .get("links")
                .and_then(|v| v.get("websiteUrl"))
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .into(),
        })
        .collect())
}

#[tauri::command]
async fn curseforge_files(
    app: tauri::AppHandle,
    mod_id: u64,
    game_version: Option<String>,
    mod_loader_type: Option<u32>,
) -> Result<Vec<CurseForgeFile>, String> {
    let proxy = curseforge_proxy();
    let endpoint = proxy
        .as_ref()
        .map(|base| format!("{base}/files/{mod_id}"))
        .unwrap_or_else(|| format!("https://api.curseforge.com/v1/mods/{mod_id}/files"));
    let mut request = http_client(&app)?.get(endpoint).query(&[
        ("gameId", "432"),
        ("pageSize", "50"),
        ("sortField", "3"),
        ("sortOrder", "desc"),
    ]);
    if proxy.is_none() {
        request = request.header("x-api-key", curseforge_key(&app)?);
    }
    if let Some(version) = game_version {
        request = request.query(&[("gameVersion", version)]);
    }
    if let Some(loader) = mod_loader_type {
        request = request.query(&[("modLoaderType", loader.to_string())]);
    }
    let response = request
        .header("User-Agent", "AuroraLauncher/0.1")
        .send()
        .await
        .map_err(|e| format!("CurseForge 文件请求失败：{e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "CurseForge 文件接口返回 HTTP {}。请检查 API Key 是否有效。",
            response.status().as_u16()
        ));
    }
    let response: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("无法解析 CurseForge 文件响应：{e}"))?;
    Ok(response
        .get("data")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .map(|file| {
            let sha1 = file
                .get("hashes")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
                .find(|hash| hash.get("algo").and_then(|v| v.as_u64()) == Some(1))
                .and_then(|hash| hash.get("value"))
                .and_then(|v| v.as_str())
                .map(str::to_string);
            CurseForgeFile {
                id: file.get("id").and_then(|v| v.as_u64()).unwrap_or_default(),
                mod_id: file.get("modId").and_then(|v| v.as_u64()).unwrap_or(mod_id),
                display_name: file
                    .get("displayName")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                file_name: file
                    .get("fileName")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string(),
                file_length: file
                    .get("fileLength")
                    .and_then(|v| v.as_u64())
                    .unwrap_or_default(),
                download_url: file
                    .get("downloadUrl")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                release_type: match file
                    .get("releaseType")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0)
                {
                    1 => "release",
                    2 => "beta",
                    3 => "alpha",
                    _ => "unknown",
                }
                .into(),
                game_versions: file
                    .get("gameVersions")
                    .and_then(|v| v.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect(),
                mod_loader: file
                    .get("modLoaderName")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                sha1,
            }
        })
        .collect())
}

fn instance_root(app: &tauri::AppHandle, instance_id: &str) -> Result<PathBuf, String> {
    let instance = read_instances(app)?
        .into_iter()
        .find(|item| item.id == instance_id)
        .ok_or_else(|| "实例不存在".to_string())?;
    let root = PathBuf::from(instance.directory);
    if !root.exists() {
        return Err("实例目录不存在".into());
    }
    Ok(root)
}

fn instance_install_dir(
    app: &tauri::AppHandle,
    instance_id: &str,
    category: &str,
) -> Result<PathBuf, String> {
    let folder = match category {
        "mod" => "mods",
        "shader" => "shaderpacks",
        "resourcepack" => "resourcepacks",
        "datapack" => "datapacks",
        _ => return Err("该资源类型暂不支持自动安装，请先下载再手动导入".into()),
    };
    Ok(instance_root(app, instance_id)?.join(folder))
}

fn safe_filename(value: &str) -> Option<String> {
    let name = PathBuf::from(value)
        .file_name()?
        .to_string_lossy()
        .to_string();
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return None;
    }
    Some(
        name.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ' | '(' | ')') {
                    c
                } else {
                    '_'
                }
            })
            .collect(),
    )
}

fn is_allowed_curseforge_download_url(parsed: &reqwest::Url) -> bool {
    let cdn_host = parsed
        .host_str()
        .map(|host| host == "forgecdn.net" || host.ends_with(".forgecdn.net"))
        .unwrap_or(false);
    parsed.scheme() == "https" && cdn_host
}

async fn verified_curseforge_bytes(
    file: &CurseForgeFile,
    client: &reqwest::Client,
) -> Result<Vec<u8>, String> {
    let download_url = file
        .download_url
        .as_deref()
        .ok_or_else(|| "该文件没有可用的官方直链".to_string())?;
    let parsed = reqwest::Url::parse(download_url).map_err(|_| "下载地址无效".to_string())?;
    if !is_allowed_curseforge_download_url(&parsed) {
        return Err("下载地址不是受支持的 CurseForge HTTPS 来源".into());
    }
    if file.file_length > 512 * 1024 * 1024 {
        return Err("文件超过 512 MiB 安全上限".into());
    }
    let response = client
        .get(parsed)
        .send()
        .await
        .map_err(|e| format!("下载失败：{e}"))?
        .error_for_status()
        .map_err(|_| "下载服务器返回错误状态".to_string())?;
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("读取下载内容失败：{e}"))?;
    if file.file_length > 0 && bytes.len() as u64 != file.file_length {
        return Err("文件大小校验失败".into());
    }
    if let Some(expected) = file.sha1.as_deref() {
        use sha1::{Digest, Sha1};
        if hex::encode(Sha1::digest(&bytes)) != expected.to_lowercase() {
            return Err("SHA-1 校验失败".into());
        }
    }
    Ok(bytes.to_vec())
}

fn write_atomic(target: &PathBuf, bytes: &[u8]) -> Result<(), String> {
    let tmp = target.with_extension("aurora.part");
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, target).map_err(|e| e.to_string())
}

#[tauri::command]
async fn install_curseforge_file(
    app: tauri::AppHandle,
    instance_id: String,
    category: String,
    file: CurseForgeFile,
) -> Result<String, String> {
    ensure_download_allowed()?;
    let filename = safe_filename(&file.file_name).ok_or_else(|| "文件名无效".to_string())?;
    let folder = instance_install_dir(&app, &instance_id, &category)?;
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let target = folder.join(&filename);
    let client = http_client(&app)?;
    let bytes = verified_curseforge_bytes(&file, &client).await?;
    write_atomic(&target, &bytes)?;
    Ok(target.display().to_string())
}

#[tauri::command]
async fn download_curseforge_file(
    app: tauri::AppHandle,
    instance_id: String,
    file: CurseForgeFile,
) -> Result<String, String> {
    ensure_download_allowed()?;
    let filename = safe_filename(&file.file_name).ok_or_else(|| "文件名无效".to_string())?;
    let folder = instance_root(&app, &instance_id)?.join("downloads");
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let target = folder.join(&filename);
    let client = http_client(&app)?;
    let bytes = verified_curseforge_bytes(&file, &client).await?;
    write_atomic(&target, &bytes)?;
    Ok(target.display().to_string())
}

fn is_allowed_modrinth_download_url(parsed: &reqwest::Url) -> bool {
    let host = parsed
        .host_str()
        .map(|host| host == "modrinth.com" || host.ends_with(".modrinth.com"))
        .unwrap_or(false);
    parsed.scheme() == "https" && host
}

async fn verified_modrinth_bytes(
    file: &ResourceFile,
    client: &reqwest::Client,
) -> Result<Vec<u8>, String> {
    let parsed = reqwest::Url::parse(&file.url).map_err(|_| "下载地址无效".to_string())?;
    if !is_allowed_modrinth_download_url(&parsed) {
        return Err("下载地址不是受支持的 Modrinth HTTPS 来源".into());
    }
    if file.size > 512 * 1024 * 1024 {
        return Err("文件超过 512 MiB 安全上限".into());
    }
    let response = client
        .get(parsed)
        .send()
        .await
        .map_err(|e| format!("下载失败：{e}"))?
        .error_for_status()
        .map_err(|_| "下载服务器返回错误状态".to_string())?;
    let bytes = response
        .bytes()
        .await
        .map_err(|e| format!("读取下载内容失败：{e}"))?;
    if file.size > 0 && bytes.len() as u64 != file.size {
        return Err("文件大小校验失败".into());
    }
    if let Some(expected) = file.hashes.get("sha512").and_then(|v| v.as_str()) {
        use sha2::{Digest, Sha512};
        if hex::encode(Sha512::digest(&bytes)) != expected.to_lowercase() {
            return Err("SHA-512 校验失败".into());
        }
    } else if let Some(expected) = file.hashes.get("sha1").and_then(|v| v.as_str()) {
        use sha1::{Digest, Sha1};
        if hex::encode(Sha1::digest(&bytes)) != expected.to_lowercase() {
            return Err("SHA-1 校验失败".into());
        }
    }
    Ok(bytes.to_vec())
}

#[tauri::command]
async fn install_modrinth_file(
    app: tauri::AppHandle,
    instance_id: String,
    category: String,
    file: ResourceFile,
) -> Result<String, String> {
    ensure_download_allowed()?;
    let filename = safe_filename(&file.filename).ok_or_else(|| "文件名无效".to_string())?;
    let folder = instance_install_dir(&app, &instance_id, &category)?;
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let target = folder.join(&filename);
    let client = http_client(&app)?;
    let bytes = verified_modrinth_bytes(&file, &client).await?;
    write_atomic(&target, &bytes)?;
    Ok(target.display().to_string())
}

#[tauri::command]
async fn download_modrinth_file(
    app: tauri::AppHandle,
    instance_id: String,
    file: ResourceFile,
) -> Result<String, String> {
    ensure_download_allowed()?;
    let filename = safe_filename(&file.filename).ok_or_else(|| "文件名无效".to_string())?;
    let folder = instance_root(&app, &instance_id)?.join("downloads");
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let target = folder.join(&filename);
    let client = http_client(&app)?;
    let bytes = verified_modrinth_bytes(&file, &client).await?;
    write_atomic(&target, &bytes)?;
    Ok(target.display().to_string())
}

#[tauri::command]
async fn modrinth_versions(
    app: tauri::AppHandle,
    project_id: String,
    game_version: Option<String>,
    loader: Option<String>,
) -> Result<Vec<ResourceVersion>, String> {
    let mut request = http_client(&app)?.get(format!(
        "https://api.modrinth.com/v2/project/{}/version",
        urlencoding::encode(&project_id)
    ));
    if let Some(v) = game_version {
        request = request.query(&[("game_versions", format!("[\"{}\"]", v))]);
    }
    if let Some(l) = loader {
        request = request.query(&[("loaders", format!("[\"{}\"]", l))]);
    }
    let values: Vec<serde_json::Value> = request
        .header("User-Agent", "AuroraLauncher/0.1 (launcher)")
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    Ok(values
        .iter()
        .map(|v| ResourceVersion {
            id: v
                .get("id")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .into(),
            name: v
                .get("name")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .into(),
            version_number: v
                .get("version_number")
                .and_then(|x| x.as_str())
                .unwrap_or_default()
                .into(),
            game_versions: v
                .get("game_versions")
                .and_then(|x| x.as_array())
                .into_iter()
                .flatten()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect(),
            loaders: v
                .get("loaders")
                .and_then(|x| x.as_array())
                .into_iter()
                .flatten()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect(),
            files: v
                .get("files")
                .and_then(|x| x.as_array())
                .into_iter()
                .flatten()
                .map(|f| ResourceFile {
                    filename: f
                        .get("filename")
                        .and_then(|x| x.as_str())
                        .unwrap_or_default()
                        .into(),
                    url: f
                        .get("url")
                        .and_then(|x| x.as_str())
                        .unwrap_or_default()
                        .into(),
                    primary: f.get("primary").and_then(|x| x.as_bool()).unwrap_or(false),
                    size: f.get("size").and_then(|x| x.as_u64()).unwrap_or_default(),
                    hashes: f.get("hashes").cloned().unwrap_or(serde_json::Value::Null),
                })
                .collect(),
        })
        .collect())
}

#[derive(Debug, Serialize)]
struct ProxyTest {
    ok: bool,
    status: u16,
    latency_ms: u64,
    message: String,
}

#[tauri::command]
fn get_settings(app: tauri::AppHandle) -> AppSettings {
    read_settings(&app)
}

#[tauri::command]
fn save_settings(app: tauri::AppHandle, settings: AppSettings) -> Result<(), String> {
    let proxy = normalized_proxy(&settings.proxy)?;
    write_settings(
        &app,
        &AppSettings {
            proxy,
            sakura_enabled: settings.sakura_enabled,
        },
    )
}

#[tauri::command]
async fn test_proxy(proxy: String) -> Result<ProxyTest, String> {
    let client = build_client(&proxy)?;
    let started = Instant::now();
    match client.get("https://api.github.com/zen").send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            Ok(ProxyTest {
                ok: response.status().is_success(),
                status,
                latency_ms: started.elapsed().as_millis() as u64,
                message: if response.status().is_success() {
                    "连接正常".into()
                } else {
                    "已连接代理，但目标返回错误状态".into()
                },
            })
        }
        Err(_) => Ok(ProxyTest {
            ok: false,
            status: 0,
            latency_ms: started.elapsed().as_millis() as u64,
            message: "连接失败，请检查代理地址和端口".into(),
        }),
    }
}

#[derive(Debug, Serialize)]
struct SakuraBackground {
    data_url: String,
    cached: bool,
}

fn allowed_image_type(value: &str) -> bool {
    matches!(value, "image/jpeg" | "image/png" | "image/webp")
}

#[tauri::command]
async fn fetch_sakura_background(
    app: tauri::AppHandle,
    refresh: Option<bool>,
) -> Result<SakuraBackground, String> {
    if !read_settings(&app).sakura_enabled {
        return Err("樱花主题未开启".into());
    }
    let dir = data_dir(&app)?;
    let cache_path = dir.join("sakura.img");
    let meta_path = dir.join("sakura.meta");
    if !refresh.unwrap_or(false) && cache_path.exists() {
        let fresh = fs::metadata(&meta_path)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| time.elapsed().ok())
            .map(|age| age < Duration::from_secs(6 * 60 * 60))
            .unwrap_or(false);
        if fresh {
            let content_type = fs::read_to_string(&meta_path)
                .unwrap_or_default()
                .trim()
                .to_string();
            if allowed_image_type(&content_type) {
                let bytes = fs::read(&cache_path).map_err(|e| e.to_string())?;
                return Ok(SakuraBackground {
                    data_url: format!(
                        "data:{content_type};base64,{}",
                        base64::engine::general_purpose::STANDARD.encode(&bytes)
                    ),
                    cached: true,
                });
            }
        }
    }
    let client = http_client(&app)?;
    let response = client
        .get("https://www.dmoe.cc/random.php")
        .send()
        .await
        .map_err(|_| "樱花背景请求失败".to_string())?;
    if !response.status().is_success() {
        return Err(format!("樱花背景返回 HTTP {}", response.status().as_u16()));
    }
    let host_ok = response
        .url()
        .host_str()
        .map(|host| host == "dmoe.cc" || host.ends_with(".dmoe.cc"))
        .unwrap_or(false);
    if !host_ok {
        return Err("樱花背景来源不是受信任域名".into());
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if !allowed_image_type(&content_type) {
        return Err("樱花背景不是支持的图片格式".into());
    }
    let bytes = response
        .bytes()
        .await
        .map_err(|_| "读取樱花背景失败".to_string())?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("樱花背景超过 8 MiB 上限".into());
    }
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    fs::write(&cache_path, &bytes).map_err(|e| e.to_string())?;
    fs::write(&meta_path, content_type.as_bytes()).map_err(|e| e.to_string())?;
    Ok(SakuraBackground {
        data_url: format!(
            "data:{content_type};base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&bytes)
        ),
        cached: false,
    })
}

#[derive(Debug, Serialize, Clone)]
struct DiscoverItem {
    source: String,
    id: String,
    slug: String,
    name: String,
    description: String,
    icon_url: Option<String>,
    downloads: u64,
    follows: u64,
    updated: String,
    categories: Vec<String>,
    game_versions: Vec<String>,
    loaders: Vec<String>,
    url: String,
    kind: String,
}

#[derive(Debug, Serialize)]
struct DiscoverPage {
    items: Vec<DiscoverItem>,
    total: u64,
    offset: u64,
    limit: u64,
    source: String,
    note: Option<String>,
}

fn modrinth_type(kind: &str) -> Option<&'static str> {
    match kind {
        "mod" => Some("mod"),
        "modpack" => Some("modpack"),
        "resourcepack" => Some("resourcepack"),
        "shader" => Some("shader"),
        "datapack" => Some("datapack"),
        _ => None,
    }
}

fn modrinth_loader_name(loader: &str) -> Option<&'static str> {
    match loader {
        "fabric" | "legacyfabric" => Some("fabric"),
        "forge" => Some("forge"),
        "neoforge" => Some("neoforge"),
        "quilt" => Some("quilt"),
        "liteloader" => Some("liteloader"),
        "optifine" => Some("optifine"),
        "iris" => Some("iris"),
        _ => None,
    }
}

fn modrinth_index(sort: &str, has_query: bool) -> &'static str {
    match sort {
        "downloads" => "downloads",
        "newest" => "newest",
        "updated" => "updated",
        _ => {
            if has_query {
                "relevance"
            } else {
                "downloads"
            }
        }
    }
}

fn cf_class_id(kind: &str) -> Option<u32> {
    match kind {
        "mod" => Some(6),
        "modpack" => Some(4471),
        "resourcepack" => Some(12),
        "shader" => Some(6552),
        "world" => Some(17),
        "datapack" => Some(6945),
        _ => None,
    }
}

fn cf_loader_id(loader: &str) -> Option<u32> {
    match loader {
        "forge" => Some(1),
        "liteloader" => Some(3),
        "fabric" | "legacyfabric" => Some(4),
        "quilt" => Some(5),
        "neoforge" => Some(6),
        _ => None,
    }
}

fn cf_loader_name(id: u64) -> Option<&'static str> {
    match id {
        1 => Some("Forge"),
        3 => Some("LiteLoader"),
        4 => Some("Fabric"),
        5 => Some("Quilt"),
        6 => Some("NeoForge"),
        _ => None,
    }
}

fn cf_sort_field(sort: &str) -> u32 {
    match sort {
        "downloads" => 6,
        "newest" | "updated" => 3,
        _ => 2,
    }
}

async fn modrinth_discover(
    client: &reqwest::Client,
    kind: &str,
    query: &str,
    game_version: &str,
    loader: &str,
    sort: &str,
    offset: u64,
    limit: u64,
) -> Result<(Vec<DiscoverItem>, u64), String> {
    let Some(project_type) = modrinth_type(kind) else {
        return Ok((Vec::new(), 0));
    };
    let mut facets: Vec<String> = vec![format!("[\"project_type:{project_type}\"]")];
    if !loader.is_empty() && (kind == "mod" || kind == "shader") {
        if let Some(tag) = modrinth_loader_name(loader) {
            facets.push(format!("[\"categories:{tag}\"]"));
        }
    }
    if !game_version.is_empty() {
        facets.push(format!("[\"versions:{game_version}\"]"));
    }
    let mut params: Vec<(&str, String)> = vec![
        ("limit", limit.to_string()),
        ("offset", offset.to_string()),
        (
            "index",
            modrinth_index(sort, !query.trim().is_empty()).to_string(),
        ),
        ("facets", format!("[{}]", facets.join(","))),
    ];
    if !query.trim().is_empty() {
        params.push(("query", query.to_string()));
    }
    let response = client
        .get("https://api.modrinth.com/v2/search")
        .query(&params)
        .send()
        .await
        .map_err(|e| format!("Modrinth 请求失败：{e}"))?;
    if !response.status().is_success() {
        return Err(format!("Modrinth 返回 HTTP {}", response.status().as_u16()));
    }
    let response: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    let total = response
        .get("total_hits")
        .and_then(|value| value.as_u64())
        .unwrap_or_default();
    let items = response
        .get("hits")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .map(|hit| {
            let categories: Vec<String> = hit
                .get("categories")
                .and_then(|value| value.as_array())
                .into_iter()
                .flatten()
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect();
            let loaders = categories
                .iter()
                .filter(|value| {
                    matches!(
                        value.as_str(),
                        "fabric"
                            | "forge"
                            | "neoforge"
                            | "quilt"
                            | "liteloader"
                            | "iris"
                            | "optifine"
                            | "rift"
                    )
                })
                .cloned()
                .collect();
            let slug = hit
                .get("slug")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            DiscoverItem {
                source: "Modrinth".into(),
                id: hit
                    .get("project_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .into(),
                url: format!("https://modrinth.com/{project_type}/{slug}"),
                slug,
                name: hit
                    .get("title")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .into(),
                description: hit
                    .get("description")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .into(),
                icon_url: hit
                    .get("icon_url")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                downloads: hit
                    .get("downloads")
                    .and_then(|value| value.as_u64())
                    .unwrap_or_default(),
                follows: hit
                    .get("follows")
                    .and_then(|value| value.as_u64())
                    .unwrap_or_default(),
                updated: hit
                    .get("date_modified")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .into(),
                categories,
                game_versions: hit
                    .get("versions")
                    .and_then(|value| value.as_array())
                    .into_iter()
                    .flatten()
                    .filter_map(|value| value.as_str().map(str::to_string))
                    .collect(),
                loaders,
                kind: kind.into(),
            }
        })
        .collect();
    Ok((items, total))
}

async fn curseforge_discover(
    app: &tauri::AppHandle,
    client: &reqwest::Client,
    kind: &str,
    query: &str,
    game_version: &str,
    loader: &str,
    sort: &str,
    offset: u64,
    limit: u64,
) -> Result<(Vec<DiscoverItem>, u64), String> {
    let proxy = curseforge_proxy();
    let endpoint = proxy
        .as_ref()
        .map(|base| format!("{base}/search"))
        .unwrap_or_else(|| "https://api.curseforge.com/v1/mods/search".into());
    let mut params: Vec<(&str, String)> = vec![
        ("gameId", "432".into()),
        ("index", offset.to_string()),
        ("pageSize", limit.to_string()),
        ("sortField", cf_sort_field(sort).to_string()),
        ("sortOrder", "desc".into()),
    ];
    if let Some(class_id) = cf_class_id(kind) {
        params.push(("classId", class_id.to_string()));
    }
    if !query.trim().is_empty() {
        params.push(("searchFilter", query.to_string()));
    }
    if !game_version.is_empty() {
        params.push(("gameVersion", game_version.to_string()));
    }
    if !loader.is_empty() {
        if let Some(loader_id) = cf_loader_id(loader) {
            params.push(("modLoaderType", loader_id.to_string()));
        }
    }
    let mut request = client.get(endpoint).query(&params);
    if proxy.is_none() {
        request = request.header("x-api-key", curseforge_key(app)?);
    }
    let response = request
        .header("User-Agent", "AuroraLauncher/0.2")
        .send()
        .await
        .map_err(|e| format!("CurseForge 请求失败：{e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "CurseForge 返回 HTTP {}。请在 console.curseforge.com 生成有效 API Key。",
            response.status().as_u16()
        ));
    }
    let response: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
    let total = response
        .pointer("/pagination/totalCount")
        .and_then(|value| value.as_u64())
        .unwrap_or_default();
    let items = response
        .get("data")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .map(|item| {
            let categories: Vec<String> = item
                .get("categories")
                .and_then(|value| value.as_array())
                .into_iter()
                .flatten()
                .filter_map(|value| value.get("name"))
                .filter_map(|value| value.as_str().map(str::to_string))
                .collect();
            let mut game_versions: Vec<String> = Vec::new();
            let mut loaders: Vec<String> = Vec::new();
            if let Some(indexes) = item
                .get("latestFilesIndexes")
                .and_then(|value| value.as_array())
            {
                for index in indexes {
                    if let Some(version) = index.get("gameVersion").and_then(|value| value.as_str())
                    {
                        if !game_versions.iter().any(|value| value == version) {
                            game_versions.push(version.to_string());
                        }
                    }
                    if let Some(name) = index
                        .get("modLoader")
                        .and_then(|value| value.as_u64())
                        .and_then(cf_loader_name)
                    {
                        let name = name.to_string();
                        if !loaders.contains(&name) {
                            loaders.push(name);
                        }
                    }
                }
            }
            let slug = item
                .get("slug")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            DiscoverItem {
                source: "CurseForge".into(),
                id: item
                    .get("id")
                    .and_then(|value| value.as_u64())
                    .unwrap_or_default()
                    .to_string(),
                url: item
                    .get("links")
                    .and_then(|value| value.get("websiteUrl"))
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .into(),
                slug,
                name: item
                    .get("name")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .into(),
                description: item
                    .get("summary")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .into(),
                icon_url: item
                    .get("logo")
                    .and_then(|value| value.get("url"))
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                downloads: item
                    .get("downloadCount")
                    .and_then(|value| value.as_u64())
                    .unwrap_or_default(),
                follows: 0,
                updated: item
                    .get("dateModified")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .into(),
                categories,
                game_versions,
                loaders,
                kind: kind.into(),
            }
        })
        .collect();
    Ok((items, total))
}

#[tauri::command]
async fn discover_resources(
    app: tauri::AppHandle,
    kind: String,
    source: Option<String>,
    query: Option<String>,
    game_version: Option<String>,
    loader: Option<String>,
    sort: Option<String>,
    offset: Option<u64>,
    limit: Option<u64>,
) -> Result<DiscoverPage, String> {
    let kind = kind.to_lowercase();
    if !matches!(
        kind.as_str(),
        "mod" | "modpack" | "resourcepack" | "shader" | "world" | "datapack"
    ) {
        return Err("未知资源类型".into());
    }
    let source = source.unwrap_or_else(|| "all".into());
    let query = query.unwrap_or_default();
    let game_version = game_version.unwrap_or_default();
    let loader = loader.unwrap_or_default();
    let sort = sort.unwrap_or_else(|| {
        if query.trim().is_empty() {
            "downloads".into()
        } else {
            "relevance".into()
        }
    });
    let offset = offset.unwrap_or(0);
    let limit = limit.unwrap_or(20).clamp(1, 50);
    let client = http_client(&app)?;
    let mut items = Vec::new();
    let mut total = 0u64;
    let mut notes: Vec<String> = Vec::new();
    if source == "all" || source == "modrinth" {
        if kind == "world" {
            notes.push("Modrinth 没有世界分类，世界请从 CurseForge 或本地导入。".into());
        } else {
            match modrinth_discover(
                &client,
                &kind,
                &query,
                &game_version,
                &loader,
                &sort,
                offset,
                limit,
            )
            .await
            {
                Ok((mut list, count)) => {
                    total += count;
                    items.append(&mut list);
                }
                Err(error) => {
                    if source == "modrinth" {
                        return Err(error);
                    }
                    notes.push(error);
                }
            }
        }
    }
    if source == "all" || source == "curseforge" {
        match curseforge_discover(
            &app,
            &client,
            &kind,
            &query,
            &game_version,
            &loader,
            &sort,
            offset,
            limit,
        )
        .await
        {
            Ok((mut list, count)) => {
                total += count;
                items.append(&mut list);
            }
            Err(error) => {
                if source == "curseforge" {
                    return Err(error);
                }
                notes.push(error);
            }
        }
    }
    if sort == "downloads" || sort == "relevance" {
        items.sort_by(|left, right| right.downloads.cmp(&left.downloads));
    } else {
        items.sort_by(|left, right| right.updated.cmp(&left.updated));
    }
    Ok(DiscoverPage {
        items,
        total,
        offset,
        limit,
        source,
        note: if notes.is_empty() {
            None
        } else {
            Some(notes.join(" "))
        },
    })
}

#[derive(Debug, Serialize)]
struct LoaderCatalog {
    id: String,
    name: String,
    game_version: String,
    versions: Vec<String>,
    installer_url: Option<String>,
    installer_template: Option<String>,
    page: String,
    note: String,
}

fn loader_label(id: &str) -> (&'static str, &'static str) {
    match id {
        "fabric" => ("Fabric", "https://fabricmc.net/use/installer/"),
        "legacyfabric" => ("Legacy Fabric", "https://legacyfabric.net/"),
        "forge" => ("Forge", "https://files.minecraftforge.net/"),
        "neoforge" => ("NeoForge", "https://neoforged.net/"),
        "optifine" => ("OptiFine", "https://optifine.net/downloads"),
        "cleanroom" => (
            "Cleanroom",
            "https://github.com/CleanroomMC/Cleanroom/releases",
        ),
        "labymod" => ("LabyMod", "https://www.labymod.net/"),
        "liteloader" => ("LiteLoader", "https://www.liteloader.com/"),
        _ => ("未知加载器", "https://minecraft.net/"),
    }
}

fn xml_versions(text: &str) -> Vec<String> {
    let mut versions = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("<version>") {
        let after = &rest[start + "<version>".len()..];
        let Some(end) = after.find("</version>") else {
            break;
        };
        versions.push(after[..end].trim().to_string());
        rest = &after[end + "</version>".len()..];
    }
    versions
}

fn neoforge_prefix(game_version: &str) -> Option<&'static str> {
    match game_version {
        "1.20.2" => Some("20.2."),
        "1.20.4" => Some("20.4."),
        "1.20.6" => Some("20.6."),
        "1.21" => Some("21.0."),
        "1.21.1" => Some("21.1."),
        "1.21.2" => Some("21.2."),
        "1.21.3" => Some("21.3."),
        "1.21.4" => Some("21.4."),
        "1.21.5" => Some("21.5."),
        _ => None,
    }
}

#[tauri::command]
async fn loader_versions(
    app: tauri::AppHandle,
    id: String,
    game_version: String,
) -> Result<LoaderCatalog, String> {
    let client = http_client(&app)?;
    let (name, page) = loader_label(&id);
    let mut versions = Vec::new();
    let mut installer_url = None;
    let mut installer_template = None;
    let note: String;
    match id.as_str() {
        "fabric" | "legacyfabric" => {
            let base = if id == "fabric" {
                "https://meta.fabricmc.net"
            } else {
                "https://meta.legacyfabric.net"
            };
            let response = client
                .get(format!("{base}/v2/versions/loader/{game_version}"))
                .send()
                .await
                .map_err(|e| format!("{name} 版本请求失败：{e}"))?;
            if !response.status().is_success() {
                return Err(format!("{name} 返回 HTTP {}", response.status().as_u16()));
            }
            let values: Vec<serde_json::Value> =
                response.json().await.map_err(|e| e.to_string())?;
            for value in values {
                if let Some(version) = value
                    .pointer("/loader/version")
                    .and_then(|item| item.as_str())
                {
                    versions.push(version.to_string());
                }
            }
            let installer = client
                .get(format!("{base}/v2/versions/installer"))
                .send()
                .await
                .ok()
                .and_then(|response| response.error_for_status().ok());
            if let Some(response) = installer {
                if let Ok(values) = response.json::<Vec<serde_json::Value>>().await {
                    let chosen = values
                        .iter()
                        .find(|value| {
                            value
                                .get("stable")
                                .and_then(|item| item.as_bool())
                                .unwrap_or(false)
                        })
                        .or_else(|| values.first());
                    installer_url = chosen
                        .and_then(|value| value.get("url"))
                        .and_then(|item| item.as_str())
                        .map(str::to_string)
                        .or_else(|| {
                            chosen
                                .and_then(|value| value.get("version"))
                                .and_then(|item| item.as_str())
                                .map(|version| {
                                    if id == "fabric" {
                                        format!("https://maven.fabricmc.net/net/fabricmc/fabric-installer/{version}/fabric-installer-{version}.jar")
                                    } else {
                                        format!("https://maven.legacyfabric.net/net/legacyfabric/fabric-installer/{version}/fabric-installer-{version}.jar")
                                    }
                                })
                        });
                }
            }
            note = "可下载官方安装器到实例 downloads；自动执行安装器将在后续接入。".into();
        }
        "forge" => {
            let response = client
                .get("https://maven.minecraftforge.net/net/minecraftforge/forge/maven-metadata.xml")
                .send()
                .await
                .map_err(|e| format!("Forge 版本请求失败：{e}"))?;
            if response.status().is_success() {
                let text = response.text().await.map_err(|e| e.to_string())?;
                let prefix = format!("{game_version}-");
                versions = xml_versions(&text)
                    .into_iter()
                    .filter(|version| version.starts_with(&prefix))
                    .map(|version| version[prefix.len()..].to_string())
                    .collect();
            }
            if versions.is_empty() {
                if let Ok(response) = client
                    .get("https://files.minecraftforge.net/net/minecraftforge/forge/promotions_slim.json")
                    .send()
                    .await
                {
                    if let Ok(value) = response.json::<serde_json::Value>().await {
                        if let Some(promos) = value.get("promos").and_then(|item| item.as_object()) {
                            for key in [
                                format!("{game_version}-recommended"),
                                format!("{game_version}-latest"),
                            ] {
                                if let Some(version) = promos.get(&key).and_then(|item| item.as_str())
                                {
                                    if !versions.iter().any(|value| value == version) {
                                        versions.push(version.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            versions.reverse();
            versions.truncate(40);
            installer_template = Some("https://maven.minecraftforge.net/net/minecraftforge/forge/{mc}-{version}/forge-{mc}-{version}-installer.jar".into());
            note = "Forge 安装器需要 Java 运行；首版仅下载安装器。".into();
        }
        "neoforge" => {
            let response = client
                .get("https://maven.neoforged.net/releases/net/neoforged/neoforge/maven-metadata.xml")
                .send()
                .await
                .map_err(|e| format!("NeoForge 版本请求失败：{e}"))?;
            if !response.status().is_success() {
                return Err(format!("NeoForge 返回 HTTP {}", response.status().as_u16()));
            }
            let text = response.text().await.map_err(|e| e.to_string())?;
            versions = xml_versions(&text);
            if let Some(prefix) = neoforge_prefix(&game_version) {
                versions.retain(|version| version.starts_with(prefix));
            }
            versions.reverse();
            versions.truncate(40);
            installer_template = Some("https://maven.neoforged.net/releases/net/neoforged/neoforge/{version}/neoforge-{version}-installer.jar".into());
            note = "NeoForge 版本按 Minecraft 版本前缀筛选；首版仅下载安装器。".into();
        }
        _ => {
            note = match id.as_str() {
                "optifine" => "OptiFine 无稳定公开 API，请从官网下载后手动导入。",
                "cleanroom" => "请从 GitHub Releases 下载；首版不自动安装。",
                "labymod" => "请使用 LabyMod 官方安装器；首版不自动执行。",
                "liteloader" => "LiteLoader 已停止更新，仅支持手动导入。",
                _ => "请从官方页面下载。",
            }
            .into();
        }
    }
    Ok(LoaderCatalog {
        id,
        name: name.into(),
        game_version,
        versions,
        installer_url,
        installer_template,
        page: page.into(),
        note,
    })
}

fn allowed_installer_host(host: &str) -> bool {
    matches!(
        host,
        "maven.fabricmc.net"
            | "maven.legacyfabric.net"
            | "maven.minecraftforge.net"
            | "maven.neoforged.net"
            | "files.minecraftforge.net"
            | "meta.fabricmc.net"
            | "meta.legacyfabric.net"
    ) || host.ends_with(".fabricmc.net")
        || host.ends_with(".neoforged.net")
        || host.ends_with(".minecraftforge.net")
}

#[tauri::command]
async fn download_loader_installer(
    app: tauri::AppHandle,
    instance_id: String,
    url: String,
) -> Result<String, String> {
    ensure_download_allowed()?;
    let parsed = reqwest::Url::parse(&url).map_err(|_| "安装器地址无效".to_string())?;
    if parsed.scheme() != "https" {
        return Err("安装器只允许 HTTPS".into());
    }
    let host = parsed.host_str().unwrap_or_default();
    if !allowed_installer_host(host) {
        return Err("安装器来源不在官方白名单内".into());
    }
    let filename = parsed
        .path_segments()
        .and_then(|segments| segments.last())
        .and_then(safe_filename)
        .ok_or_else(|| "无法确定安装器文件名".to_string())?;
    let folder = instance_root(&app, &instance_id)?.join("downloads");
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let target = folder.join(filename);
    let client = http_client(&app)?;
    let response = client
        .get(parsed)
        .send()
        .await
        .map_err(|e| format!("安装器下载失败：{e}"))?
        .error_for_status()
        .map_err(|_| "安装器下载服务器返回错误".to_string())?;
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > 512 * 1024 * 1024 {
        return Err("安装器超过 512 MiB 安全上限".into());
    }
    write_atomic(&target, &bytes)?;
    Ok(target.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safe_id_keeps_only_portable_characters() {
        assert_eq!(safe_id("My World!/../x"), "My-World-----x");
    }

    #[test]
    fn safe_filename_rejects_traversal() {
        assert_eq!(safe_filename(".."), None);
        assert_eq!(safe_filename("a/../../b.jar"), Some("b.jar".into()));
        assert_eq!(safe_filename("evil\\..\\x.jar"), Some("x.jar".into()));
        assert_eq!(
            safe_filename("ok-mod 1.2.jar"),
            Some("ok-mod 1.2.jar".into())
        );
    }

    #[test]
    fn curseforge_host_allowlist_blocks_other_origins() {
        let allowed = |url: &str| {
            reqwest::Url::parse(url)
                .map(|parsed| is_allowed_curseforge_download_url(&parsed))
                .unwrap_or(false)
        };
        assert!(allowed("https://mediafilez.forgecdn.net/files/1/2/mod.jar"));
        assert!(allowed("https://edge.forgecdn.net/files/1/2/mod.jar"));
        assert!(allowed("https://media.forgecdn.net/files/1/2/mod.jar"));
        assert!(!allowed("https://example.com/mod.jar"));
        assert!(!allowed("http://edge.forgecdn.net/mod.jar"));
        assert!(!allowed("https://forgecdn.net.evil.example/mod.jar"));
    }

    #[test]
    fn installer_hosts_are_restricted() {
        assert!(allowed_installer_host("maven.fabricmc.net"));
        assert!(allowed_installer_host("maven.minecraftforge.net"));
        assert!(allowed_installer_host("maven.neoforged.net"));
        assert!(!allowed_installer_host("example.com"));
        assert!(!allowed_installer_host("fabricmc.net.evil.example"));
    }

    #[test]
    fn image_types_are_restricted() {
        assert!(allowed_image_type("image/jpeg"));
        assert!(allowed_image_type("image/png"));
        assert!(!allowed_image_type("text/html"));
        assert!(!allowed_image_type("image/svg+xml"));
    }

    #[test]
    fn modrinth_host_allowlist_blocks_other_origins() {
        let allowed = |url: &str| {
            reqwest::Url::parse(url)
                .map(|parsed| is_allowed_modrinth_download_url(&parsed))
                .unwrap_or(false)
        };
        assert!(allowed(
            "https://cdn.modrinth.com/data/abc/versions/1/mod.jar"
        ));
        assert!(!allowed("https://modrinth.com.evil.example/mod.jar"));
        assert!(!allowed("http://cdn.modrinth.com/mod.jar"));
    }
}

fn load_local_env() {
    let Ok(text) = fs::read_to_string(".env") else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || std::env::var_os(key).is_some() {
            continue;
        }
        let value = value.trim().trim_matches('"').trim_matches('\'');
        std::env::set_var(key, value);
    }
}

pub fn self_test() -> Result<String, String> {
    load_local_env();
    let appdata = std::env::var("APPDATA").map_err(|_| "APPDATA 未设置".to_string())?;
    let root = PathBuf::from(appdata).join("com.aurora.launcher");
    let mut lines = vec![format!("data_dir={}", root.display())];
    let key_path = root.join("curseforge.key");
    if key_path.exists() {
        let encrypted = fs::read(&key_path).map_err(|e| e.to_string())?;
        #[cfg(windows)]
        {
            let key = dpapi_decrypt(&encrypted)?;
            lines.push(format!(
                "curseforge_local=decrypted_ok(empty={})",
                key.trim().is_empty()
            ));
        }
        #[cfg(not(windows))]
        {
            lines.push(format!(
                "curseforge_local=plain_len={}",
                String::from_utf8_lossy(&encrypted).len()
            ));
        }
    } else {
        lines.push("curseforge_local=absent".into());
    }
    lines.push(format!("curseforge_proxy={}", curseforge_proxy().is_some()));
    lines.push(format!(
        "curseforge_env={}",
        std::env::var("CURSEFORGE_API_KEY")
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    ));
    Ok(lines.join("\n"))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    load_local_env();
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            app_info,
            set_curseforge_key,
            clear_curseforge_key,
            get_settings,
            save_settings,
            test_proxy,
            fetch_sakura_background,
            list_instances,
            create_instance,
            update_instance,
            open_instance_folder,
            open_external,
            delete_instance,
            list_loaders,
            loader_versions,
            download_loader_installer,
            detect_java,
            minecraft_manifest,
            discover_resources,
            search_modrinth,
            search_curseforge,
            curseforge_files,
            install_curseforge_file,
            download_curseforge_file,
            install_modrinth_file,
            download_modrinth_file,
            modrinth_versions
        ])
        .run(tauri::generate_context!())
        .expect("error while running Aurora Launcher");
}
