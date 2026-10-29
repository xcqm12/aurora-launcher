use crate::{
    http_client, read_instances, safe_filename, write_atomic, write_instance_meta, write_instances,
};
use serde::Serialize;
use serde_json::Value;
use sha1::{Digest as Sha1Digest, Sha1};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Serialize)]
pub struct LoaderInstallResult {
    pub version_id: String,
    pub installer: String,
    pub log: String,
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("请求失败：{e}"))?;
    if !response.status().is_success() {
        return Err(format!("HTTP {}", response.status().as_u16()));
    }
    Ok(response.bytes().await.map_err(|e| e.to_string())?.to_vec())
}

async fn download_to(
    client: &reqwest::Client,
    url: &str,
    target: &Path,
    sha1: Option<&str>,
    size: u64,
) -> Result<(), String> {
    if target.exists() {
        if size == 0 {
            return Ok(());
        }
        if let Ok(meta) = fs::metadata(target) {
            if meta.len() == size {
                return Ok(());
            }
        }
    }
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("下载失败：{e}"))?;
    if !response.status().is_success() {
        return Err(format!("下载返回 HTTP {}", response.status().as_u16()));
    }
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    if size > 0 && bytes.len() as u64 != size {
        return Err("文件大小校验失败".into());
    }
    if let Some(expected) = sha1 {
        if hex::encode(Sha1::digest(&bytes)) != expected.to_lowercase() {
            return Err("SHA-1 校验失败".into());
        }
    }
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    write_atomic(target, &bytes)
}

async fn ensure_vanilla(
    client: &reqwest::Client,
    game_dir: &Path,
    version: &str,
) -> Result<(), String> {
    let version_dir = game_dir.join("versions").join(version);
    let json_path = version_dir.join(format!("{version}.json"));
    let jar_path = version_dir.join(format!("{version}.jar"));
    if !json_path.exists() {
        let manifest: Value = client
            .get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json")
            .send()
            .await
            .map_err(|e| format!("获取版本清单失败：{e}"))?
            .error_for_status()
            .map_err(|_| "获取版本清单失败".to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        let url = manifest
            .get("versions")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .find(|item| item.get("id").and_then(|value| value.as_str()) == Some(version))
            .and_then(|item| item.get("url"))
            .and_then(|value| value.as_str())
            .ok_or_else(|| format!("Mojang 清单中找不到版本 {version}"))?;
        let bytes = fetch(client, url).await?;
        fs::create_dir_all(&version_dir).map_err(|e| e.to_string())?;
        write_atomic(&json_path, &bytes)?;
    }
    if !jar_path.exists() {
        let text = fs::read_to_string(&json_path).map_err(|e| e.to_string())?;
        let json: Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let download = json
            .get("downloads")
            .and_then(|value| value.get("client"))
            .ok_or_else(|| "版本 JSON 缺少 client 下载信息".to_string())?;
        let url = download
            .get("url")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "client 下载地址缺失".to_string())?;
        let sha1 = download.get("sha1").and_then(|value| value.as_str());
        let size = download
            .get("size")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        download_to(client, url, &jar_path, sha1, size).await?;
    }
    let profiles = game_dir.join("launcher_profiles.json");
    if !profiles.exists() {
        fs::create_dir_all(game_dir).map_err(|e| e.to_string())?;
        fs::write(profiles, br#"{"profiles":{},"settings":{},"version":3}"#)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

async fn installer_url(
    client: &reqwest::Client,
    loader: &str,
    minecraft: &str,
    version: &str,
) -> Result<String, String> {
    match loader {
        "fabric" | "legacyfabric" => {
            let base = if loader == "fabric" {
                "https://meta.fabricmc.net"
            } else {
                "https://meta.legacyfabric.net"
            };
            let values: Vec<Value> = client
                .get(format!("{base}/v2/versions/installer"))
                .send()
                .await
                .map_err(|e| format!("获取安装器信息失败：{e}"))?
                .error_for_status()
                .map_err(|_| "获取安装器信息失败".to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;
            let chosen = values
                .iter()
                .find(|value| value.get("stable").and_then(|item| item.as_bool()) == Some(true))
                .or_else(|| values.first())
                .ok_or_else(|| "安装器版本列表为空".to_string())?;
            if let Some(url) = chosen.get("url").and_then(|value| value.as_str()) {
                return Ok(url.to_string());
            }
            let installer_version = chosen
                .get("version")
                .and_then(|value| value.as_str())
                .ok_or_else(|| "安装器版本缺失".to_string())?;
            if loader == "fabric" {
                Ok(format!(
                    "https://maven.fabricmc.net/net/fabricmc/fabric-installer/{installer_version}/fabric-installer-{installer_version}.jar"
                ))
            } else {
                Ok(format!(
                    "https://maven.legacyfabric.net/net/legacyfabric/fabric-installer/{installer_version}/fabric-installer-{installer_version}.jar"
                ))
            }
        }
        "forge" => Ok(format!(
            "https://maven.minecraftforge.net/net/minecraftforge/forge/{minecraft}-{version}/forge-{minecraft}-{version}-installer.jar"
        )),
        "neoforge" => Ok(format!(
            "https://maven.neoforged.net/releases/net/neoforged/neoforge/{version}/neoforge-{version}-installer.jar"
        )),
        "cleanroom" => {
            let release: Value = client
                .get("https://api.github.com/repos/CleanroomMC/Cleanroom/releases/latest")
                .header("User-Agent", "AuroraLauncher/0.4")
                .send()
                .await
                .map_err(|e| format!("获取 Cleanroom 发布信息失败：{e}"))?
                .error_for_status()
                .map_err(|_| "获取 Cleanroom 发布信息失败".to_string())?
                .json()
                .await
                .map_err(|e| e.to_string())?;
            release
                .get("assets")
                .and_then(|value| value.as_array())
                .into_iter()
                .flatten()
                .find(|asset| {
                    asset
                        .get("name")
                        .and_then(|value| value.as_str())
                        .map(|name| name.ends_with("installer.jar"))
                        .unwrap_or(false)
                })
                .and_then(|asset| asset.get("browser_download_url"))
                .and_then(|value| value.as_str())
                .map(str::to_string)
                .ok_or_else(|| "Cleanroom 发布中找不到 installer.jar".to_string())
        }
        _ => Err("该加载器不支持自动安装".into()),
    }
}

fn allowed_installer_url(url: &str) -> bool {
    let Ok(parsed) = reqwest::Url::parse(url) else {
        return false;
    };
    if parsed.scheme() != "https" {
        return false;
    }
    let host = parsed.host_str().unwrap_or_default();
    matches!(
        host,
        "maven.fabricmc.net"
            | "maven.legacyfabric.net"
            | "maven.minecraftforge.net"
            | "maven.neoforged.net"
            | "github.com"
            | "objects.githubusercontent.com"
    ) || host.ends_with(".forgecdn.net")
        || host.ends_with(".fabricmc.net")
        || host.ends_with(".neoforged.net")
        || host.ends_with(".minecraftforge.net")
}

fn find_installed_version(game_dir: &Path, loader: &str, version: &str) -> Option<String> {
    let versions = game_dir.join("versions");
    let mut best: Option<(u64, String)> = None;
    let entries = fs::read_dir(&versions).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let json = path.join(format!("{name}.json"));
        if !json.exists() {
            continue;
        }
        let matches = match loader {
            "fabric" | "legacyfabric" => name.contains("fabric-loader") && name.contains(version),
            "forge" => (name.contains("forge") || name.contains("Forge")) && name.contains(version),
            "neoforge" => name.to_lowercase().contains("neoforge") && name.contains(version),
            "cleanroom" => name.to_lowercase().contains("cleanroom"),
            _ => false,
        };
        if !matches {
            continue;
        }
        let modified = fs::metadata(&json)
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs())
            .unwrap_or(0);
        if best
            .as_ref()
            .map(|(best_time, _)| modified > *best_time)
            .unwrap_or(true)
        {
            best = Some((modified, name));
        }
    }
    best.map(|(_, name)| name)
}

pub async fn install(
    app: tauri::AppHandle,
    instance_id: String,
    loader: String,
    version: String,
) -> Result<LoaderInstallResult, String> {
    if !matches!(
        loader.as_str(),
        "fabric" | "legacyfabric" | "forge" | "neoforge" | "cleanroom"
    ) {
        return Err("该加载器没有可用的无头安装器，请使用官方页面手动导入".into());
    }
    if version.trim().is_empty() {
        return Err("加载器版本不能为空".into());
    }
    let instance = read_instances(&app)?
        .into_iter()
        .find(|item| item.id == instance_id)
        .ok_or_else(|| "实例不存在".to_string())?;
    let game_dir = PathBuf::from(&instance.directory).join("game");
    let minecraft = instance.minecraft_version.clone();
    let client = http_client(&app)?;
    ensure_vanilla(&client, &game_dir, &minecraft).await?;
    let url = installer_url(&client, &loader, &minecraft, &version).await?;
    if !allowed_installer_url(&url) {
        return Err("安装器来源不在官方白名单内".into());
    }
    let file_name = reqwest::Url::parse(&url)
        .ok()
        .and_then(|parsed| {
            parsed
                .path_segments()
                .and_then(|segments| segments.last())
                .map(str::to_string)
        })
        .and_then(|name| safe_filename(&name))
        .unwrap_or_else(|| format!("{loader}-installer.jar"));
    let installer = PathBuf::from(&instance.directory)
        .join("downloads")
        .join(file_name);
    download_to(&client, &url, &installer, None, 0).await?;

    let java = crate::launch::find_java(instance.java_path.as_deref(), 8)?;
    let mut command = Command::new(&java);
    command.current_dir(&game_dir);
    crate::hide_console(&mut command);
    match loader.as_str() {
        "fabric" | "legacyfabric" => {
            command
                .arg("-jar")
                .arg(&installer)
                .arg("client")
                .arg("-dir")
                .arg(&game_dir)
                .arg("-mcversion")
                .arg(&minecraft)
                .arg("-loader")
                .arg(&version);
        }
        "forge" | "neoforge" => {
            command
                .arg("-jar")
                .arg(&installer)
                .arg("--installClient")
                .arg(&game_dir);
        }
        "cleanroom" => {
            command
                .arg("-jar")
                .arg(&installer)
                .arg("--directory")
                .arg(&game_dir)
                .arg("--yes")
                .arg("--no-gui");
        }
        _ => {}
    }
    let output = tauri::async_runtime::spawn_blocking(move || command.output())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("安装器启动失败：{e}"))?;
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(format!(
            "安装器退出码 {}，日志：{}",
            output.status.code().unwrap_or(-1),
            log.chars()
                .rev()
                .take(2000)
                .collect::<String>()
                .chars()
                .rev()
                .collect::<String>()
        ));
    }
    let version_id = find_installed_version(&game_dir, &loader, &version)
        .ok_or_else(|| "安装器完成，但没有找到新生成的版本 JSON".to_string())?;

    let mut all = read_instances(&app)?;
    if let Some(item) = all.iter_mut().find(|item| item.id == instance_id) {
        item.loader_version = Some(version.clone());
        item.launch_version_id = Some(version_id.clone());
        item.state = "ready".into();
        let updated = item.clone();
        write_instance_meta(&updated)?;
    }
    write_instances(&app, &all)?;
    Ok(LoaderInstallResult {
        version_id,
        installer: installer.display().to_string(),
        log: log
            .chars()
            .rev()
            .take(8000)
            .collect::<String>()
            .chars()
            .rev()
            .collect(),
    })
}
