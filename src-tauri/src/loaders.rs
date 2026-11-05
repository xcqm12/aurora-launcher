use crate::{
    http_client, read_instances, safe_filename, write_atomic, write_instance_meta, write_instances,
};
use serde::Serialize;
use serde_json::Value;
use tauri::Emitter;
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

#[derive(Debug, Clone, Serialize)]
pub struct LoaderProgress {
    pub stage: String,
    pub filename: String,
    pub downloaded: u64,
    pub total: u64,
}

fn emit_loader_progress(
    app: &tauri::AppHandle,
    stage: &str,
    filename: &str,
    downloaded: u64,
    total: u64,
) {
    let _ = app.emit(
        "loader-progress",
        LoaderProgress {
            stage: stage.to_string(),
            filename: filename.to_string(),
            downloaded,
            total,
        },
    );
}

/// `download_to` with `loader-progress` events every 256 KiB; the largest
/// candidate response's content-length is used as the total.
async fn download_to_progressing(
    app: &tauri::AppHandle,
    client: &reqwest::Client,
    urls: &[String],
    target: &Path,
    sha1: Option<&str>,
    size: u64,
    stage: &str,
    filename: &str,
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
    let mut last = String::new();
    for url in urls {
        // 每个地址独立重试：网络抖动/代理断流在传输中途才暴露。
        let bytes = crate::download_bytes_retry(
            || client.get(url),
            "下载",
            crate::DOWNLOAD_ATTEMPTS,
            |downloaded, total| emit_loader_progress(app, stage, filename, downloaded, total),
        )
        .await;
        let bytes = match bytes {
            Ok(bytes) => bytes,
            Err(error) => {
                last = format!("{url}: {error}");
                continue;
            }
        };
        if size > 0 && bytes.len() as u64 != size {
            last = format!("{url}: 文件大小校验失败（{} / {size} 字节）", bytes.len());
            continue;
        }
        if let Some(expected) = sha1 {
            if hex::encode(Sha1::digest(&bytes)) != expected.to_lowercase() {
                last = format!("{url}: SHA-1 校验失败");
                continue;
            }
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        return write_atomic(target, &bytes);
    }
    Err(format!("下载失败：{last}"))
}

/// OptiFine 官网反爬需要特定 UA 与 Referer；带重试的文本抓取。
async fn optifine_get_text(
    client: &reqwest::Client,
    url: &str,
    referer: Option<&str>,
) -> Result<String, String> {
    let bytes = crate::download_bytes_retry(
        || {
            let mut builder = client.get(url).header("User-Agent", OPTIFINE_UA);
            if let Some(referer) = referer {
                builder = builder.header("Referer", referer);
            }
            builder
        },
        "OptiFine 官网",
        crate::DOWNLOAD_ATTEMPTS,
        |_, _| {},
    )
    .await?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

async fn fetch(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    // 加载器元数据/版本 JSON 也可能中途断流；统一走重试下载。
    crate::download_bytes_retry(
        || client.get(url),
        "请求",
        crate::DOWNLOAD_ATTEMPTS,
        |_, _| {},
    )
    .await
}

/// Try each candidate URL in order (mirror first when the source says so).
pub(crate) async fn fetch_first(client: &reqwest::Client, urls: &[String]) -> Result<Vec<u8>, String> {
    let mut last = String::new();
    for url in urls {
        match fetch(client, url).await {
            Ok(bytes) => return Ok(bytes),
            Err(error) => last = format!("{url}: {error}"),
        }
    }
    Err(if urls.len() > 1 {
        format!("所有下载源都失败：{last}")
    } else {
        last
    })
}

async fn ensure_vanilla(
    app: &tauri::AppHandle,
    client: &reqwest::Client,
    game_dir: &Path,
    version: &str,
    source: &str,
) -> Result<(), String> {
    let version_dir = game_dir.join("versions").join(version);
    let json_path = version_dir.join(format!("{version}.json"));
    let jar_path = version_dir.join(format!("{version}.jar"));
    if !json_path.exists() {
        let manifest_bytes = fetch_first(
            client,
            &crate::launch::candidates(
                "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
                source,
                version,
            ),
        )
        .await
        .map_err(|e| format!("获取版本清单失败：{e}"))?;
        let manifest: Value =
            serde_json::from_slice(&manifest_bytes).map_err(|e| e.to_string())?;
        let url = manifest
            .get("versions")
            .and_then(|value| value.as_array())
            .into_iter()
            .flatten()
            .find(|item| item.get("id").and_then(|value| value.as_str()) == Some(version))
            .and_then(|item| item.get("url"))
            .and_then(|value| value.as_str())
            .ok_or_else(|| format!("Mojang 清单中找不到版本 {version}"))?;
        let bytes = fetch_first(
            client,
            &crate::launch::candidates(url, source, version),
        )
        .await?;
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
        download_to_progressing(
            app,
            client,
            &crate::launch::candidates(url, source, version),
            &jar_path,
            sha1,
            size,
            "下载 MC 本体",
            &format!("{version}.jar"),
        )
        .await?;
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
            let raw = fetch(&client, &format!("{base}/v2/versions/installer")).await?;
            let values: Vec<Value> = serde_json::from_slice(&raw)
                .map_err(|e| format!("安装器版本列表解析失败：{e}"))?;
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
            let raw = fetch(
                &client,
                "https://api.github.com/repos/CleanroomMC/Cleanroom/releases/latest",
            )
            .await?;
            let release: Value = serde_json::from_slice(&raw)
                .map_err(|e| format!("Cleanroom 发布信息解析失败：{e}"))?;
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
    let source = crate::launch::resolve_download_source(&client, &crate::read_settings(&app).download_source)
        .await;
    ensure_vanilla(&app, &client, &game_dir, &minecraft, &source).await?;
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
        .join(&file_name);
    let installer_urls = crate::launch::candidates(&url, &source, &minecraft);
    download_to_progressing(
        &app,
        &client,
        &installer_urls,
        &installer,
        None,
        0,
        "下载安装器",
        &file_name,
    )
    .await?;
    emit_loader_progress(&app, "校验安装器", &file_name, 1, 1);

    let java = crate::launch::find_java(instance.java_path.as_deref(), 8)?;
    let build_command = |attempt: u32| {
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
        let _ = attempt;
        command
    };
    let mut combined_log = String::new();
    let output = {
        let mut output = None;
        for attempt in 1..=3 {
            emit_loader_progress(
                &app,
                "运行安装器",
                &file_name,
                attempt as u64,
                3,
            );
            let mut command = build_command(attempt);
            let result = tauri::async_runtime::spawn_blocking(move || command.output())
                .await
                .map_err(|e| e.to_string())?
                .map_err(|e| format!("安装器启动失败：{e}"))?;
            let log = format!(
                "{}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            combined_log.push_str(&format!("--- 第 {attempt} 次尝试 ---
{log}
"));
            if result.status.success() {
                output = Some(result);
                break;
            }
            if attempt < 3 {
                let failed = failed_libraries_from_log(&log);
                if !failed.is_empty() {
                    let source_hint = source.as_str();
                    let total = failed.len() as u64;
                    for (index, name) in failed.iter().enumerate() {
                        emit_loader_progress(
                            &app,
                            "预下载缺失库",
                            name,
                            index as u64,
                            total,
                        );
                    }
                    let lines = predownload_failed_libraries(
                        &client,
                        &game_dir,
                        &failed,
                        source_hint,
                    )
                    .await;
                    combined_log.push_str(&lines);
                }
            } else {
                output = Some(result);
            }
        }
        output.ok_or_else(|| "安装器未产生输出".to_string())?
    };
    let log = combined_log.clone();
    if !output.status.success() {
        return Err(format!(
            "安装器退出码 {}（已自动重试 3 次），日志：{}",
            output.status.code().unwrap_or(-1),
            tail_log(&log, 3000)
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
    let source = crate::launch::resolve_download_source(&client, &crate::read_settings(&app).download_source).await;
    ensure_vanilla(&app, &client, &game_dir, &minecraft, &source).await?;

    let name = if version.trim().is_empty() {
        // 参考 PCL：版本列表优先 BMCLAPI JSON（稳定），失败回退官网 HTML 解析。
        match optifine_versions_bmclapi(&client, &minecraft).await {
            Some(versions) => versions
                .first()
                .cloned()
                .ok_or_else(|| format!("OptiFine 没有适配 {minecraft} 的版本"))?,
            None => {
                let html = optifine_get_text(
                    &client,
                    "https://optifine.net/downloads",
                    Some("https://optifine.net"),
                )
                .await?;
                optifine_versions_for_page(&html, &minecraft)
                    .first()
                    .cloned()
                    .ok_or_else(|| format!("OptiFine 官网没有找到适配 {minecraft} 的版本"))?
            }
        }
    } else {
        normalize_optifine_name(&version)
    };
    if !name.starts_with(&format!("OptiFine_{minecraft}_")) {
        return Err(format!("OptiFine 版本 {name} 不匹配 Minecraft {minecraft}"));
    }

    let token = optifine_download_token(&client, &name).await?;
    let bytes = crate::download_bytes_retry(
        || {
            client
                .get(format!(
                    "https://optifine.net/downloadx?f={name}.jar&x={token}"
                ))
                .header("User-Agent", OPTIFINE_UA)
                .header("Referer", format!("https://optifine.net/adloadx?f={name}.jar"))
        },
        "下载 OptiFine 安装器",
        crate::DOWNLOAD_ATTEMPTS,
        |downloaded, total| {
            emit_loader_progress(&app, "下载 OptiFine 安装器", &format!("{name}.jar"), downloaded, total)
        },
    )
    .await?;
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
    let source = crate::launch::resolve_download_source(&client, &crate::read_settings(&app).download_source).await;
    ensure_vanilla(&app, &client, &game_dir, &minecraft, &source).await?;

    let raw = fetch(&client, LITELOADER_META).await?;
    let meta: Value =
        serde_json::from_slice(&raw).map_err(|e| format!("LiteLoader 元数据解析失败：{e}"))?;
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
    // metadata.xml 拿不到就回退到固定名；重试交给 fetch。
    let core_name = match fetch(&client, &metadata_url).await {
        Ok(xml) => match snapshot_file_name(&String::from_utf8_lossy(&xml)) {
            Some(snapshot) => format!("liteloader-{minecraft}-{snapshot}.jar"),
            None => format!("liteloader-{version_str}.jar"),
        },
        Err(_) => format!("liteloader-{version_str}.jar"),
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
            let xml = match fetch(&client, &metadata_url).await {
                Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Err(_) => String::new(),
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

/// OptiFine 版本列表，BMCLAPI（与 PCL 一致的做法）：官网 HTML 反爬不稳定，
/// BMCLAPI 提供 JSON 列表；失败返回 None 由调用方回退官网解析。
async fn optifine_versions_bmclapi(
    client: &reqwest::Client,
    minecraft: &str,
) -> Option<Vec<String>> {
    let raw = crate::download_bytes_retry(
        || client.get("https://bmclapi2.bangbang93.com/optifine/versionList"),
        "OptiFine 版本列表（BMCLAPI）",
        crate::DOWNLOAD_ATTEMPTS,
        |_, _| {},
    )
    .await
    .ok()?;
    let values: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    let list = values.as_array()?;
    let mut versions: Vec<String> = list
        .iter()
        .filter_map(|entry| {
            let mc = entry.get("mcversion")?.as_str()?;
            if mc != minecraft {
                return None;
            }
            // 预览版不自动推荐；手动填名字仍可用（normalize_optifine_name）。
            let patch = entry.get("patch")?.as_str()?;
            if patch.contains("pre") {
                return None;
            }
            let kind = entry.get("type")?.as_str()?;
            Some(format!("OptiFine_{mc}_{kind}_{patch}"))
        })
        .collect();
    versions.reverse();
    if versions.is_empty() {
        None
    } else {
        Some(versions)
    }
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
    // 与 PCL 同样的流程：adloadx 广告页生成一次性 token，解析后拼 downloadx。
    let html = optifine_get_text(
        client,
        &format!("https://optifine.net/adloadx?f={name}.jar"),
        Some("https://optifine.net/downloads"),
    )
    .await?;
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

/// Parse the `These libraries failed to download` section the Forge/NeoForge
/// installer prints on failure; returns `group:artifact:version[:classifier]`
/// coordinates.
fn failed_libraries_from_log(log: &str) -> Vec<String> {
    let Some(position) = log.find("These libraries failed to download") else {
        return Vec::new();
    };
    let tail = &log[position..];
    let mut libs = Vec::new();
    for line in tail.lines().skip(1) {
        let line = line.trim();
        if line.is_empty() || line.contains("There was an error") {
            continue;
        }
        let parts: Vec<&str> = line.split(':').collect();
        if !matches!(parts.len(), 3 | 4) {
            continue;
        }
        if parts.iter().any(|part| {
            part.is_empty()
                || !part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        }) {
            continue;
        }
        if !libs.iter().any(|existing| existing == line) {
            libs.push(line.to_string());
        }
    }
    libs
}

/// Maven repo path for a coordinate, tolerating an optional classifier.
fn maven_relative_full(name: &str) -> Option<String> {
    let parts: Vec<&str> = name.split(':').collect();
    match parts.as_slice() {
        [group, artifact, version] => Some(format!(
            "{}/{}/{}/{}-{}.jar",
            group.replace('.', "/"),
            artifact,
            version,
            artifact,
            version
        )),
        [group, artifact, version, classifier] => Some(format!(
            "{}/{}/{}/{}-{}-{}.jar",
            group.replace('.', "/"),
            artifact,
            version,
            artifact,
            version,
            classifier
        )),
        _ => None,
    }
}

/// Pre-download libraries the installer could not fetch, from mirrors the
/// installer itself does not know about (BMCLAPI, Sponge repo, Maven Central).
/// The installer verifies checksums itself, so a wrong file is simply deleted
/// by the next attempt.
async fn predownload_failed_libraries(
    client: &reqwest::Client,
    game_dir: &Path,
    libs: &[String],
    source: &str,
) -> String {
    let mut report = String::new();
    for name in libs {
        let Some(relative) = maven_relative_full(name) else {
            continue;
        };
        let target = game_dir.join("libraries").join(&relative);
        if target.exists() {
            report.push_str(&format!("预下载跳过（已存在）：{name}
"));
            continue;
        }
        let mut candidates: Vec<String> = vec![
            format!("https://bmclapi2.bangbang93.com/maven/{relative}"),
            format!("https://libraries.minecraft.net/{relative}"),
        ];
        if name.starts_with("org.spongepowered:") {
            candidates.push(format!("https://repo.spongepowered.org/maven/{relative}"));
        }
        candidates.push(format!("https://repo1.maven.org/maven2/{relative}"));
        if source == "bmclapi" || source == "auto" {
            // keep mirror-first ordering above; official fallback last
        } else {
            candidates.reverse();
        }
        let mut downloaded = false;
        for url in &candidates {
            match fetch(client, url).await {
                Ok(bytes) => {
                    if bytes.len() < 4 || &bytes[..2] != b"PK" {
                        report.push_str(&format!("预下载 {name}：{} 不是有效 JAR
", url));
                        continue;
                    }
                    if let Some(parent) = target.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    if write_atomic(&target, &bytes).is_ok() {
                        report.push_str(&format!("预下载 {} 成功（{}）
", name, url));
                        downloaded = true;
                        break;
                    }
                }
                Err(error) => {
                    report.push_str(&format!("预下载 {name}：{url} → {error}
"));
                }
            }
        }
        if !downloaded {
            report.push_str(&format!("预下载 {name} 全部来源失败，交由安装器重试
"));
        }
    }
    report
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
    fn failed_libraries_are_parsed_from_installer_log() {
        let log = r#"Downloading library from https://maven.creeperhost.net/trove/trove/1.0.2/trove-1.0.2.jar
Download failed: Checksum invalid, deleting file: Expected: 112b40 Actual: null
These libraries failed to download. Try again.
org.spongepowered:mixin:0.8.5
There was an error during installation"#;
        assert_eq!(
            failed_libraries_from_log(log),
            vec!["org.spongepowered:mixin:0.8.5".to_string()]
        );
        let multi = "These libraries failed to download. Try again.
org.ow2.asm:asm:7.1
net.minecraftforge:mergetool:1.1.9:fat
";
        assert_eq!(
            failed_libraries_from_log(multi),
            vec![
                "org.ow2.asm:asm:7.1".to_string(),
                "net.minecraftforge:mergetool:1.1.9:fat".to_string(),
            ]
        );
        assert!(failed_libraries_from_log("all good").is_empty());
        assert!(failed_libraries_from_log("These libraries failed to download. Try again.
not a lib").is_empty());
    }

    #[test]
    fn maven_paths_support_classifiers() {
        assert_eq!(
            maven_relative_full("org.spongepowered:mixin:0.8.5").as_deref(),
            Some("org/spongepowered/mixin/0.8.5/mixin-0.8.5.jar")
        );
        assert_eq!(
            maven_relative_full("net.minecraftforge:mergetool:1.1.9:fat").as_deref(),
            Some("net/minecraftforge/mergetool/1.1.9/mergetool-1.1.9-fat.jar")
        );
        assert_eq!(maven_relative_full("bad"), None);
    }

    #[test]
    fn jar_content_check_rejects_non_archive() {
        assert!(verify_jar_contains(b"not a jar", "x").is_err());
        // A valid empty zip (EOCD only) parses but has no entries.
        let empty_zip = b"PK\x05\x06\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00";
        assert!(!verify_jar_contains(empty_zip, "x").unwrap_or(true));
    }
}
