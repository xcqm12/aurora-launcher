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
            "optifine" => name.contains("OptiFine") && name.contains(version),
            "liteloader" => name.contains("LiteLoader") && name.contains(version),
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
    if loader == "optifine" {
        return install_optifine(app, instance_id, version).await;
    }
    if loader == "liteloader" {
        return install_liteloader(app, instance_id).await;
    }
    if !matches!(
        loader.as_str(),
        "fabric" | "legacyfabric" | "forge" | "neoforge" | "cleanroom"
    ) {
        return Err("该加载器没有可用的自动安装方式，请使用官方页面手动导入".into());
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

async fn install_optifine(
    app: tauri::AppHandle,
    instance_id: String,
    version: String,
) -> Result<LoaderInstallResult, String> {
    let instance = read_instances(&app)?
        .into_iter()
        .find(|item| item.id == instance_id)
        .ok_or_else(|| "实例不存在".to_string())?;
    let game_dir = PathBuf::from(&instance.directory).join("game");
    let minecraft = instance.minecraft_version.clone();
    let client = http_client(&app)?;
    ensure_vanilla(&client, &game_dir, &minecraft).await?;

    let name = if version.trim().is_empty() {
        let html = client
            .get("https://optifine.net/downloads")
            .header("User-Agent", OPTIFINE_UA)
            .send()
            .await
            .map_err(|e| format!("打开 OptiFine 官网失败：{e}"))?
            .error_for_status()
            .map_err(|_| "OptiFine 官网返回错误".to_string())?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        optifine_versions_for_page(&html, &minecraft)
            .first()
            .cloned()
            .ok_or_else(|| format!("OptiFine 官网没有找到适配 {minecraft} 的版本"))?
    } else {
        normalize_optifine_name(&version)
    };
    if !name.starts_with(&format!("OptiFine_{minecraft}_")) {
        return Err(format!("OptiFine 版本 {name} 不匹配 Minecraft {minecraft}"));
    }

    let token = optifine_download_token(&client, &name).await?;
    let response = client
        .get(format!(
            "https://optifine.net/downloadx?f={name}.jar&x={token}"
        ))
        .header("User-Agent", OPTIFINE_UA)
        .header("Referer", format!("https://optifine.net/adloadx?f={name}.jar"))
        .send()
        .await
        .map_err(|e| format!("下载 OptiFine 安装器失败：{e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "下载 OptiFine 安装器返回 HTTP {}",
            response.status().as_u16()
        ));
    }
    let bytes = response.bytes().await.map_err(|e| e.to_string())?;
    if bytes.len() > OPTIFINE_MAX_BYTES {
        return Err("OptiFine 安装器体积异常".into());
    }
    verify_jar_contains(&bytes, "optifine/Installer.class")?;
    let downloads = PathBuf::from(&instance.directory).join("downloads");
    let installer = downloads.join(format!("{name}.jar"));
    write_atomic(&installer, &bytes)?;
    let driver = downloads.join("AuroraOptiFineDriver.class");
    write_atomic(&driver, OPTIFINE_DRIVER)?;

    let java = crate::launch::find_java(instance.java_path.as_deref(), 8)?;
    let separator = if cfg!(windows) { ";" } else { ":" };
    let mut command = Command::new(&java);
    command
        .arg("-cp")
        .arg(format!(
            "{}{separator}{}",
            installer.display(),
            driver.display()
        ))
        .arg("AuroraOptiFineDriver")
        .arg(&game_dir);
    crate::hide_console(&mut command);
    let output = tauri::async_runtime::spawn_blocking(move || command.output())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| format!("OptiFine 安装器启动失败：{e}"))?;
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        return Err(format!(
            "OptiFine 安装器退出码 {}，日志：{}",
            output.status.code().unwrap_or(-1),
            tail_log(&log, 2000)
        ));
    }
    let edition = name
        .trim_start_matches(&format!("OptiFine_{minecraft}_"))
        .to_string();
    let mut result = finish_install(
        &app,
        &instance_id,
        &game_dir,
        "optifine",
        &minecraft,
        &name,
        &installer,
        &log,
    )?;
    result.installer = edition;
    Ok(result)
}

/// Read `timestamp`/`buildNumber` out of a small maven-metadata.xml.
fn snapshot_file_name(metadata_xml: &str) -> Option<String> {
    let between = |tag: &str| -> Option<String> {
        let open = format!("<{tag}>");
        let close = format!("</{tag}>");
        let start = metadata_xml.find(&open)? + open.len();
        let end = metadata_xml[start..].find(&close)? + start;
        Some(metadata_xml[start..end].trim().to_string())
    };
    let timestamp = between("timestamp")?;
    let build = between("buildNumber")?;
    Some(format!("{timestamp}-{build}"))
}

fn library_repo_base(name: &str, declared: Option<&str>) -> String {
    if let Some(url) = declared {
        if !url.trim().is_empty() {
            return url.trim_end_matches('/').to_string();
        }
    }
    if name.starts_with("org.spongepowered:") {
        SPONGE_REPO.to_string()
    } else if name.starts_with("org.ow2.asm:") {
        MAVEN_CENTRAL.to_string()
    } else {
        "https://libraries.minecraft.net".to_string()
    }
}

fn maven_relative_path(name: &str) -> String {
    let parts: Vec<&str> = name.split(':').collect();
    let (group, artifact, version) = match parts.as_slice() {
        [group, artifact, version] => (*group, *artifact, *version),
        _ => return String::new(),
    };
    format!(
        "{}/{}/{}/{}-{}.jar",
        group.replace('.', "/"),
        artifact,
        version,
        artifact,
        version
    )
}

async fn install_liteloader(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<LoaderInstallResult, String> {
    let instance = read_instances(&app)?
        .into_iter()
        .find(|item| item.id == instance_id)
        .ok_or_else(|| "实例不存在".to_string())?;
    let game_dir = PathBuf::from(&instance.directory).join("game");
    let minecraft = instance.minecraft_version.clone();
    let client = http_client(&app)?;
    ensure_vanilla(&client, &game_dir, &minecraft).await?;

    let meta: Value = client
        .get(LITELOADER_META)
        .send()
        .await
        .map_err(|e| format!("获取 LiteLoader 官方元数据失败：{e}"))?
        .error_for_status()
        .map_err(|_| "LiteLoader 官方元数据不可用".to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let node = meta
        .pointer(&format!("/versions/{minecraft}"))
        .cloned()
        .ok_or_else(|| format!("LiteLoader 官方最高支持 1.12.2，没有适配 {minecraft} 的版本"))?;
    let artifact = node
        .pointer("/snapshots/com.mumfrey:liteloader")
        .and_then(|value| value.as_object())
        .ok_or_else(|| "LiteLoader 元数据缺少构建信息".to_string())?;
    let version_str = artifact
        .get("version")
        .and_then(|value| value.as_str())
        .unwrap_or("SNAPSHOT")
        .to_string();
    let md5_expected = artifact
        .get("md5")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let mut log = String::new();

    // Resolve the timestamped snapshot file name via maven-metadata.xml.
    let metadata_url = format!(
        "{LITELOADER_REPO}com/mumfrey/liteloader/{version_str}/maven-metadata.xml"
    );
    let core_name = match client.get(&metadata_url).send().await {
        Ok(response) if response.status().is_success() => {
            let xml = response.text().await.unwrap_or_default();
            match snapshot_file_name(&xml) {
                Some(snapshot) => format!("liteloader-{minecraft}-{snapshot}.jar"),
                None => format!("liteloader-{version_str}.jar"),
            }
        }
        _ => format!("liteloader-{version_str}.jar"),
    };
    let core_url = format!("{LITELOADER_REPO}com/mumfrey/liteloader/{version_str}/{core_name}");
    let core_bytes = fetch(&client, &core_url).await?;
    if !verify_jar_contains(
        &core_bytes,
        "com/mumfrey/liteloader/launch/LiteLoaderTweaker.class",
    )? {
        return Err("下载的 LiteLoader 核心 JAR 缺少 LiteLoaderTweaker，已中止".into());
    }
    {
        use md5::Digest as Md5Digest;
        let actual = hex::encode(md5::Md5::digest(&core_bytes));
        if !md5_expected.is_empty() && actual != md5_expected.to_lowercase() {
            log.push_str(&format!(
                "注意：官方元数据 md5（{md5_expected}）与实际（{actual}）不一致，已按“有效 JAR 且包含 Tweaker”放行。\n"
            ));
        }
    }

    // Collect the libraries the official installer would write into the
    // profile: the core artifact plus everything listed under snapshots.
    let mut libraries: Vec<(String, Option<String>)> = vec![(
        format!("com.mumfrey:liteloader:{version_str}"),
        Some(LITELOADER_REPO.to_string()),
    )];
    if let Some(list) = node
        .pointer("/snapshots/libraries")
        .and_then(|value| value.as_array())
    {
        for item in list {
            if let Some(name) = item.get("name").and_then(|value| value.as_str()) {
                let url = item
                    .get("url")
                    .and_then(|value| value.as_str())
                    .map(str::to_string);
                if !libraries.iter().any(|(existing, _)| existing == name) {
                    libraries.push((name.to_string(), url));
                }
            }
        }
    }
    if let Some(list) = artifact
        .get("libraries")
        .and_then(|value| value.as_array())
    {
        for item in list {
            if let Some(name) = item.get("name").and_then(|value| value.as_str()) {
                if !libraries.iter().any(|(existing, _)| existing == name) {
                    libraries.push((name.to_string(), None));
                }
            }
        }
    }

    // Pre-download every dependency into its maven path; snapshot artifacts
    // are stored under the plain -SNAPSHOT file name the JSON references.
    for (name, declared) in &libraries {
        if name == &format!("com.mumfrey:liteloader:{version_str}") {
            continue;
        }
        let base = library_repo_base(name, declared.as_deref());
        let relative = maven_relative_path(name);
        let target = game_dir.join("libraries").join(&relative);
        if target.exists() {
            continue;
        }
        if name.ends_with("-SNAPSHOT") {
            let metadata_url = format!(
                "{base}/{}/maven-metadata.xml",
                relative
                    .rsplit_once('/')
                    .map(|(dir, _)| dir)
                    .unwrap_or_default()
            );
            let xml = match client.get(&metadata_url).send().await {
                Ok(response) if response.status().is_success() => {
                    response.text().await.unwrap_or_default()
                }
                _ => String::new(),
            };
            let snapshot = snapshot_file_name(&xml)
                .ok_or_else(|| format!("无法解析快照依赖 {name}（{metadata_url}）"))?;
            let parts: Vec<&str> = name.split(':').collect();
            if parts.len() != 3 {
                return Err(format!("无法解析依赖坐标 {name}"));
            }
            let real_url = format!(
                "{base}/{}/{}/{}-{}-{}.jar",
                parts[0].replace('.', "/"),
                parts[2],
                parts[1],
                parts[2],
                snapshot
            );
            let bytes = fetch(&client, &real_url).await?;
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            write_atomic(&target, &bytes)?;
            log.push_str(&format!("已下载 {name}（快照 {snapshot}）\n"));
            continue;
        }
        let bytes = fetch(&client, &format!("{base}/{relative}")).await?;
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        write_atomic(&target, &bytes)?;
        log.push_str(&format!("已下载 {name}\n"));
    }

    // Store the core jar under the -SNAPSHOT path the version JSON references.
    let core_relative =
        format!("com/mumfrey/liteloader/{version_str}/liteloader-{version_str}.jar");
    let core_target = game_dir.join("libraries").join(&core_relative);
    if !core_target.exists() {
        if let Some(parent) = core_target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        write_atomic(&core_target, &core_bytes)?;
    }

    // Build the launchwrapper version JSON the way the official installer does.
    let parent_path = game_dir
        .join("versions")
        .join(&minecraft)
        .join(format!("{minecraft}.json"));
    let parent_text =
        fs::read_to_string(&parent_path).map_err(|e| format!("读取原版版本 JSON 失败：{e}"))?;
    let parent: Value = serde_json::from_str(&parent_text).map_err(|e| e.to_string())?;
    let parent_args = parent
        .get("minecraftArguments")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let id = format!("{minecraft}-LiteLoader");
    let mut child_libraries = Vec::new();
    for (name, declared) in &libraries {
        let mut entry = serde_json::json!({ "name": name });
        match declared {
            Some(url) => entry["url"] = serde_json::json!(url),
            None => {
                let base = library_repo_base(name, None);
                if base != "https://libraries.minecraft.net" {
                    entry["url"] = serde_json::json!(format!("{base}/"));
                }
            }
        }
        child_libraries.push(entry);
    }
    let child = serde_json::json!({
        "id": id,
        "inheritsFrom": minecraft,
        "releaseTime": "2017-11-28T00:00:00+00:00",
        "time": "2017-11-28T00:00:00+00:00",
        "type": "release",
        "mainClass": "net.minecraft.launchwrapper.Launch",
        "minecraftArguments": format!(
            "--tweakClass com.mumfrey.liteloader.launch.LiteLoaderTweaker {parent_args}"
        ),
        "libraries": child_libraries,
    });
    let version_dir = game_dir.join("versions").join(&id);
    fs::create_dir_all(&version_dir).map_err(|e| e.to_string())?;
    let version_json = version_dir.join(format!("{id}.json"));
    let version_bytes = serde_json::to_vec_pretty(&child).map_err(|e| e.to_string())?;
    write_atomic(&version_json, &version_bytes)?;

    let installer = game_dir.join("libraries").join(&core_relative);
    finish_install(
        &app,
        &instance_id,
        &game_dir,
        "liteloader",
        &minecraft,
        artifact
            .get("build")
            .and_then(|value| value.as_str())
            .unwrap_or(&version_str),
        &installer,
        &log,
    )
}

const OPTIFINE_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AuroraLauncher";
const OPTIFINE_MAX_BYTES: usize = 40 * 1024 * 1024;
const OPTIFINE_DRIVER: &[u8] = include_bytes!("../resources/AuroraOptiFineDriver.class");
const LITELOADER_META: &str = "http://dl.liteloader.com/versions/versions.json";
const LITELOADER_REPO: &str = "http://repo.mumfrey.com/content/repositories/snapshots/";
const SPONGE_REPO: &str = "https://repo.spongepowered.org/maven/";
const MAVEN_CENTRAL: &str = "https://repo1.maven.org/maven2/";

/// Normalize an OptiFine build identifier: accept `OptiFine_1.20.1_HD_U_I6`,
/// `OptiFine_1.20.1_HD_U_I6.jar` or `preview_...`; returns the jar file stem.
pub(crate) fn normalize_optifine_name(value: &str) -> String {
    value
        .trim()
        .trim_end_matches(".jar")
        .trim_start_matches("preview_")
        .trim_start_matches('/')
        .to_string()
}

/// Extract `OptiFine_<mc>_<edition>` candidates from the official downloads
/// page (`adloadx?f=<name>.jar` links), newest first.
pub(crate) fn optifine_names_from_page(html: &str) -> Vec<String> {
    let mut names = Vec::new();
    for part in html.split("adloadx?f=") {
        let end = match part.find('"').or_else(|| part.find('\'')) {
            Some(index) => index,
            None => continue,
        };
        let raw = &part[..end];
        let name = raw.split('&').next().unwrap_or(raw);
        if !name.ends_with(".jar") {
            continue;
        }
        let normalized = normalize_optifine_name(name);
        if normalized.starts_with("OptiFine_") && !names.contains(&normalized) {
            names.push(normalized);
        }
    }
    names
}

pub(crate) fn optifine_versions_for_page(html: &str, minecraft: &str) -> Vec<String> {
    let prefix = format!("OptiFine_{minecraft}_");
    let mut versions: Vec<String> = optifine_names_from_page(html)
        .into_iter()
        .filter(|name| name.starts_with(&prefix))
        .collect();
    versions.reverse();
    versions
}

async fn optifine_download_token(client: &reqwest::Client, name: &str) -> Result<String, String> {
    let response = client
        .get(format!("https://optifine.net/adloadx?f={name}.jar"))
        .header("User-Agent", OPTIFINE_UA)
        .send()
        .await
        .map_err(|e| format!("打开 OptiFine 下载页失败：{e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "OptiFine 下载页返回 HTTP {}",
            response.status().as_u16()
        ));
    }
    let html = response.text().await.map_err(|e| e.to_string())?;
    let needle = format!("downloadx?f={name}.jar&x=");
    let start = html
        .find(&needle)
        .ok_or_else(|| "OptiFine 下载页中没有生成下载链接".to_string())?
        + needle.len();
    let rest = &html[start..];
    let token: String = rest
        .chars()
        .take_while(|item| item.is_ascii_hexdigit())
        .collect();
    if token.is_empty() {
        return Err("OptiFine 下载令牌解析失败".into());
    }
    Ok(token)
}

fn verify_jar_contains(bytes: &[u8], entry: &str) -> Result<bool, String> {
    if bytes.len() < 4 || &bytes[..2] != b"PK" {
        return Err("下载内容不是有效的 JAR 文件".into());
    }
    let cursor = std::io::Cursor::new(bytes.to_vec());
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| format!("JAR 解析失败：{e}"))?;
    for index in 0..archive.len() {
        if archive
            .by_index(index)
            .map(|file| file.name() == entry)
            .unwrap_or(false)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn update_instance_loader(
    app: &tauri::AppHandle,
    instance_id: &str,
    loader_version: &str,
    version_id: &str,
) -> Result<(), String> {
    let mut all = read_instances(app)?;
    if let Some(item) = all.iter_mut().find(|item| item.id == instance_id) {
        item.loader_version = Some(loader_version.to_string());
        item.launch_version_id = Some(version_id.to_string());
        item.state = "ready".into();
        let updated = item.clone();
        write_instance_meta(&updated)?;
    }
    write_instances(app, &all)
}

fn tail_log(log: &str, limit: usize) -> String {
    log.chars().rev().take(limit).collect::<String>().chars().rev().collect()
}

fn finish_install(
    app: &tauri::AppHandle,
    instance_id: &str,
    game_dir: &Path,
    loader: &str,
    minecraft: &str,
    loader_version: &str,
    installer: &Path,
    log: &str,
) -> Result<LoaderInstallResult, String> {
    let version_id = find_installed_version(game_dir, loader, minecraft)
        .ok_or_else(|| format!("安装完成，但没有在 versions 中找到 {loader} 版本目录。日志：{log}"))?;
    update_instance_loader(app, instance_id, loader_version, &version_id)?;
    Ok(LoaderInstallResult {
        version_id,
        installer: installer.display().to_string(),
        log: tail_log(log, 8000),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optifine_names_are_extracted_newest_first() {
        let html = r#"
            <a href='adloadx?f=OptiFine_1.20.1_HD_U_I6.jar&x=abc'>I6</a>
            <a href="adloadx?f=preview_OptiFine_1.20.1_HD_U_I7_pre.jar">pre</a>
            <a href="adloadx?f=OptiFine_1.19.4_HD_U_I4.jar&x=zz">old</a>
        "#;
        let names = optifine_names_from_page(html);
        assert_eq!(
            names,
            vec![
                "OptiFine_1.20.1_HD_U_I6".to_string(),
                "OptiFine_1.20.1_HD_U_I7_pre".to_string(),
                "OptiFine_1.19.4_HD_U_I4".to_string(),
            ]
        );
        let for_mc = optifine_versions_for_page(html, "1.20.1");
        assert_eq!(
            for_mc,
            vec![
                "OptiFine_1.20.1_HD_U_I7_pre".to_string(),
                "OptiFine_1.20.1_HD_U_I6".to_string(),
            ]
        );
        assert!(optifine_versions_for_page(html, "1.21.9").is_empty());
    }

    #[test]
    fn optifine_names_are_normalized() {
        assert_eq!(
            normalize_optifine_name("OptiFine_1.20.1_HD_U_I6.jar"),
            "OptiFine_1.20.1_HD_U_I6"
        );
        assert_eq!(
            normalize_optifine_name("preview_OptiFine_1.20.1_HD_U_I7_pre.jar"),
            "OptiFine_1.20.1_HD_U_I7_pre"
        );
    }

    #[test]
    fn snapshot_names_are_parsed_from_metadata() {
        let xml = r#"<metadata><versioning><snapshot>
            <timestamp>20171128.144431</timestamp>
            <buildNumber>4</buildNumber>
        </snapshot></versioning></metadata>"#;
        assert_eq!(
            snapshot_file_name(xml).as_deref(),
            Some("20171128.144431-4")
        );
        assert_eq!(snapshot_file_name("<broken>"), None);
    }

    #[test]
    fn maven_paths_and_repo_bases_are_resolved() {
        assert_eq!(
            maven_relative_path("com.mumfrey:liteloader:1.12.2-SNAPSHOT"),
            "com/mumfrey/liteloader/1.12.2-SNAPSHOT/liteloader-1.12.2-SNAPSHOT.jar"
        );
        assert_eq!(
            library_repo_base("org.spongepowered:mixin:0.7.4-SNAPSHOT", None),
            SPONGE_REPO
        );
        assert_eq!(
            library_repo_base("org.ow2.asm:asm-all:5.2", None),
            MAVEN_CENTRAL
        );
        assert_eq!(
            library_repo_base("net.minecraft:launchwrapper:1.12", None),
            "https://libraries.minecraft.net"
        );
        assert_eq!(
            library_repo_base("net.minecraft:launchwrapper:1.12", Some("http://ignored/")),
            "http://ignored"
        );
    }

    #[test]
    fn jar_content_check_rejects_non_archive() {
        assert!(verify_jar_contains(b"not a jar", "x").is_err());
        // A valid empty zip (EOCD only) parses but has no entries.
        let empty_zip = b"PK\x05\x06\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        assert!(!verify_jar_contains(empty_zip, "x").unwrap_or(true));
    }
}
