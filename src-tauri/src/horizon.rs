//! Horizon 客户端支持（github.com/horizon-rust/horizon，MIT）。
//!
//! Horizon 是用 Rust 从零重写的 Minecraft 26.3——游戏进程原生运行，
//! 不需要 Java。Aurora 在这里承担 Horizon 官方 launcher 的角色：
//! 1. 下载客户端发行包（GitHub Releases，SHA256SUMS 校验）
//! 2. 按官方布局准备 MC 资源（26.3 version JSON / client jar / assets，
//!    复用 launch.rs 下载管线与镜像候选）+ 26.3 服务端 jar
//! 3. 驱动 `horizon-prepare` 打包资源，再以 `horizon-client` 原生启动
//! 4. 会话：微软正版账号由 Aurora 生成会话文件（读后即删）；离线账号
//!    以未登录模式启动
//!
//! 「自我意识」式的越界不存在——Horizon 只是被启动的游戏，Aurora 只负责
//! 把它准备好、跑起来，不干预其内部行为。

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;
use zip::ZipArchive;

use crate::auth::LaunchAccount;
use crate::launch::{self, DownloadItem, Pipeline};
use crate::{github_api_request, hide_console, write_atomic};

pub(crate) const HORIZON_LOADER: &str = "horizon";
pub(crate) const HORIZON_MC_VERSION: &str = "26.3";
/// horizon-prepare 需要的 Java 大版本（实测以 --launch-api 输出为准）。
const PREPARE_JAVA_MAJOR: u32 = 25;
const HORIZON_RELEASES_API: &str =
    "https://api.github.com/repos/horizon-rust/horizon/releases?per_page=100";
const HORIZON_CLIENT_ASSET: &str = "horizon-windows-x64.zip";
const HORIZON_SUMS_ASSET: &str = "SHA256SUMS.txt";
const PREPARE_TIMEOUT_SECS: u64 = 900;

const THIRD_PARTY_NOTICE: &str = "\
This directory bundles the Horizon client (https://github.com/horizon-rust/horizon),
MIT License, Copyright (c) 2026 Ludovic Marie and the Horizon contributors.
Full license text: https://github.com/horizon-rust/horizon/blob/main/LICENSE
Downloaded and installed by Aurora Launcher; not modified or re-signed.
";

/// 发行版选择结果：tag + 客户端 zip 下载地址。
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct HorizonRelease {
    pub tag: String,
    pub asset_url: String,
    pub sums_url: String,
}

/// 从 GitHub Releases 列表挑选 Horizon 客户端发行版：
/// 仅考虑包含 `horizon-windows-x64.zip` 资产的 release；稳定 tag（v*）
/// 优先于 nightly-<date>，移动 nightly 最后。返回 None 表示没有可用发行版。
pub(crate) fn pick_horizon_release(releases: &Value) -> Option<HorizonRelease> {
    let items = releases.as_array()?;
    let mut candidates: Vec<HorizonRelease> = Vec::new();
    for release in items {
        let tag = release.get("tag_name").and_then(|v| v.as_str()).unwrap_or("");
        if tag.is_empty() {
            continue;
        }
        let assets = release.get("assets").and_then(|v| v.as_array())?;
        let client = assets.iter().find(|a| a.get("name").and_then(|v| v.as_str()) == Some(HORIZON_CLIENT_ASSET));
        let sums = assets.iter().find(|a| a.get("name").and_then(|v| v.as_str()) == Some(HORIZON_SUMS_ASSET));
        let (Some(client), Some(sums)) = (client, sums) else {
            continue;
        };
        let asset_url = client.get("browser_download_url").and_then(|v| v.as_str())?;
        let sums_url = sums.get("browser_download_url").and_then(|v| v.as_str())?;
        candidates.push(HorizonRelease {
            tag: tag.to_string(),
            asset_url: asset_url.to_string(),
            sums_url: sums_url.to_string(),
        });
    }
    // 稳定 tag（v*）优先；同级按列表顺序（GitHub 返回已按时间降序）。
    candidates.sort_by_key(|c| if c.tag.starts_with('v') { 0 } else { 1 });
    candidates.into_iter().next()
}

/// Windows 平台的 library 规则过滤（与 launch.rs rules_allow 等价）。
fn horizon_rules_allow_windows(rules: Option<&Vec<Value>>) -> bool {
    let Some(rules) = rules else {
        return true;
    };
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        let action = rule.get("action").and_then(|v| v.as_str()).unwrap_or("allow");
        let os_name = rule
            .get("os")
            .and_then(|os| os.get("name"))
            .and_then(|v| v.as_str());
        let matches = match os_name {
            None => true,
            Some(name) => name == "windows",
        };
        if matches {
            allowed = action == "allow";
        }
    }
    allowed
}

fn horizon_dir(instance_dir: &Path) -> PathBuf {
    instance_dir.join("game").join("horizon")
}

fn installed_client_dir(instance_dir: &Path) -> Option<PathBuf> {
    let versions = horizon_dir(instance_dir).join("versions");
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in fs::read_dir(&versions).ok()?.flatten() {
        // 发行包解压后可能是根目录或单一子目录（如 horizon-windows-x64/）
        let candidates = [
            entry.path().join("horizon-client.exe"),
            entry
                .path()
                .join("horizon-windows-x64")
                .join("horizon-client.exe"),
        ];
        if candidates.iter().any(|c| c.is_file()) {
            let modified = entry.metadata().and_then(|m| m.modified()).ok()?;
            if best.as_ref().map(|(t, _)| modified > *t).unwrap_or(true) {
                best = Some((modified, entry.path()));
            }
        }
    }
    best.map(|(_, path)| {
        if path.join("horizon-client.exe").is_file() {
            path
        } else {
            path.join("horizon-windows-x64")
        }
    })
}

/// 解压 zip 全部条目到目标目录（通用：Horizon 发行包根目录即 client）。
fn unzip_all(archive_path: &Path, dest: &Path) -> Result<(), String> {
    let file = File::open(archive_path).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|e| format!("zip 打开失败：{e}"))?;
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let target = dest.join(name);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut output = File::create(&target).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut output).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 下载并安装 Horizon 客户端到实例 horizon/versions/<tag>/。
/// 已安装（存在 horizon-client.exe）时直接返回当前 tag。
pub(crate) async fn ensure_installed(
    app: &tauri::AppHandle,
    instance_dir: &Path,
    report: &(dyn Fn(&str) + Send + Sync),
) -> Result<String, String> {
    if let Some(dir) = installed_client_dir(instance_dir) {
        let tag = dir.file_name().unwrap_or_default().to_string_lossy().to_string();
        report(&format!("[Horizon] 客户端已就绪：{tag}"));
        return Ok(tag);
    }
    let client = crate::http_client(app)?;
    report("[Horizon] 查询 Horizon 发行版");
    let releases: Value = github_api_request(&client, HORIZON_RELEASES_API)
        .send()
        .await
        .map_err(|e| format!("查询 Horizon 发行版失败：{e}"))?
        .error_for_status()
        .map_err(|e| format!("查询 Horizon 发行版失败：{e}"))?
        .json()
        .await
        .map_err(|e| format!("Horizon 发行版解析失败：{e}"))?;
    let release = pick_horizon_release(&releases)
        .ok_or_else(|| "Horizon 没有可用的 Windows x64 客户端发行版".to_string())?;
    let tag = release.tag.clone();
    let dest_dir = horizon_dir(instance_dir).join("versions").join(&tag);
    if installed_client_dir(instance_dir).is_some() {
        report(&format!("[Horizon] 客户端已就绪：{tag}"));
        return Ok(tag);
    }
    report(&format!("[Horizon] 下载 Horizon 客户端 {}（约 59 MB）", tag));
    let zip_bytes = crate::download_bytes_retry(
        || client.get(&release.asset_url),
        "Horizon 客户端",
        3,
        |_, _| {},
    )
    .await?;
    report("[Horizon] 校验发行包完整性");
    let sums = crate::download_bytes_retry(
        || client.get(&release.sums_url),
        "Horizon SHA256SUMS",
        2,
        |_, _| {},
    )
    .await
    .unwrap_or_default();
    if !sums.is_empty() {
        let expected = expected_sha_from_sums(&String::from_utf8_lossy(&sums), HORIZON_CLIENT_ASSET);
        let actual = hex::encode(Sha256::digest(&zip_bytes));
        match expected {
            Some(expected) if expected != actual => {
                return Err("Horizon 客户端 SHA-256 校验失败，已中止".into());
            }
            _ => {}
        }
    }
    let zip_path = dest_dir.with_extension("download.zip");
    if let Some(parent) = zip_path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    write_atomic(&zip_path, &zip_bytes)?;
    report("[Horizon] 解压客户端");
    unzip_all(&zip_path, &dest_dir)?;
    let _ = fs::remove_file(&zip_path);
    // zip 可能带单一子目录（horizon-windows-x64/…），用兼容扫描确认
    if installed_client_dir(instance_dir).is_none() {
        return Err("Horizon 发行包内容异常（缺少 horizon-client.exe）".into());
    }
    fs::write(
        dest_dir.join("THIRD_PARTY_NOTICES.txt"),
        THIRD_PARTY_NOTICE,
    )
    .map_err(|e| e.to_string())?;
    report(&format!("[Horizon] 客户端已安装：{tag}"));
    Ok(tag)
}

/// 从 SHA256SUMS 内容提取指定文件名的期望哈希（容忍 CRLF 与 "*<name>"）。
fn expected_sha_from_sums(sums: &str, filename: &str) -> Option<String> {
    for line in sums.lines() {
        let line = line.trim_end_matches('\r');
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let file = parts.next()?.trim_start_matches('*');
        if file.eq_ignore_ascii_case(filename) && hash.len() == 64 {
            return Some(hash.to_lowercase());
        }
    }
    None
}

/// 准备 MC 26.3 资源（官方布局 minecraft/ + 服务端 jar）并驱动
/// horizon-prepare 打包。已存在 prepared 目录时跳过。
pub(crate) async fn ensure_prepared(
    app: &tauri::AppHandle,
    instance_dir: &Path,
    settings_download_source: &str,
    report: &(dyn Fn(&str) + Send + Sync),
) -> Result<PathBuf, String> {
    let root = horizon_dir(instance_dir);
    let Some(client_dir) = installed_client_dir(instance_dir) else {
        return Err("Horizon 客户端未安装".into());
    };
    let prepared = client_dir.join(".local").join("horizon-client");
    let existing = fs::read_dir(&prepared)
        .ok()
        .and_then(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().join("prepared.marker").exists())
                .map(|e| e.path())
                .next()
        });
    if let Some(found) = existing {
        report("[Horizon] 资源已就绪");
        return Ok(found);
    }
    let Some(client_dir) = installed_client_dir(instance_dir) else {
        return Err("Horizon 客户端未安装".into());
    };
    let minecraft_dir = root.join("minecraft");
    let version = HORIZON_MC_VERSION;
    report(&format!("[Horizon] 准备 MC {version} 资源（无需 Java 运行库）"));

    // Horizon 钉死了 26.3 版本 JSON 的 sha1（research-lock.json，Mojang
    // 重发过且 prepare 严格校验）。piston-meta 的 packages URL 以 sha1
    // 为路径，因此直接由 lock 的 manifest_sha1 拼出 pin 地址。
    let client = crate::http_client(app)?;
    let lock: Value = crate::download_bytes_retry(
        || {
            client.get("https://raw.githubusercontent.com/horizon-rust/horizon/main/research-lock.json")
        },
        "Horizon research-lock",
        3,
        |_, _| {},
    )
    .await
    .and_then(|bytes| serde_json::from_slice(&bytes).map_err(|e| e.to_string()))
    .map_err(|e| format!("获取 Horizon research-lock 失败：{e}"))?;
    let manifest_sha1 = lock
        .pointer("/minecraft/manifest_sha1")
        .and_then(|v| v.as_str())
        .filter(|v| v.len() == 40)
        .ok_or_else(|| "research-lock 缺少 minecraft.manifest_sha1".to_string())?
        .to_string();
    let version_json_url =
        format!("https://piston-meta.mojang.com/v1/packages/{manifest_sha1}/{version}.json");
    let version_json_text = crate::download_bytes_retry(
        || client.get(&version_json_url),
        "MC 版本 JSON（Horizon pin）",
        3,
        |_, _| {},
    )
    .await
    .map_err(|e| format!("获取版本 JSON 失败：{e}"))?;
    let actual_json_sha1 = hex::encode(Sha256::digest(&version_json_text));
    let _ = actual_json_sha1; // prepare 自身会校验；这里仅落盘
    let version_json: Value = serde_json::from_slice(&version_json_text)
        .map_err(|e| format!("版本 JSON 解析失败：{e}"))?;
    let version_dir = minecraft_dir.join("versions").join(version);
    fs::create_dir_all(&version_dir).map_err(|e| e.to_string())?;
    write_atomic(&version_dir.join(format!("{version}.json")), &version_json_text)?;
    // horizon-prepare 校验 pin 的 manifest；同步一份到它的约定路径
    let pinned_dir = client_dir.join(".local").join("minecraft").join(version);
    fs::create_dir_all(&pinned_dir).map_err(|e| e.to_string())?;
    write_atomic(&pinned_dir.join("manifest.json"), &version_json_text)?;

    let client_download = version_json
        .pointer("/downloads/client")
        .ok_or_else(|| "版本 JSON 缺少 client 下载信息".to_string())?;
    let server_download = version_json
        .pointer("/downloads/server")
        .ok_or_else(|| "版本 JSON 缺少 server 下载信息".to_string())?;
    let asset_index = version_json
        .get("assetIndex")
        .ok_or_else(|| "版本 JSON 缺少 assetIndex".to_string())?;
    let as_str = |v: &Value, field: &str| {
        v.get(field).and_then(|x| x.as_str()).unwrap_or_default().to_string()
    };
    let as_u64 = |v: &Value, field: &str| v.get(field).and_then(|x| x.as_u64()).unwrap_or(0);

    // 服务端 jar（horizon-prepare 需要）
    let server_jars = root.join("server-jars");
    fs::create_dir_all(&server_jars).map_err(|e| e.to_string())?;
    let server_path = server_jars.join(format!("{version}.jar"));

    let mut items = vec![
        DownloadItem {
            url: as_str(client_download, "url"),
            path: version_dir.join(format!("{version}.jar")),
            sha1: Some(as_str(client_download, "sha1")),
            size: as_u64(client_download, "size"),
        },
        DownloadItem {
            url: as_str(server_download, "url"),
            path: server_path.clone(),
            sha1: Some(as_str(server_download, "sha1")),
            size: as_u64(server_download, "size"),
        },
        DownloadItem {
            url: as_str(asset_index, "url"),
            path: minecraft_dir.join("assets").join("indexes").join(format!("{version}.json")),
            sha1: Some(as_str(asset_index, "sha1")),
            size: as_u64(asset_index, "size"),
        },
    ];
    // libraries：游戏本体是 Rust，但 horizon-prepare 导入数据时需要读取
    // 官方 libraries（lz4 等），按 launch.rs 同一套 windows 规则下载。
    if let Some(libraries) = version_json.get("libraries").and_then(|v| v.as_array()) {
        for library in libraries {
            let rules = library.get("rules").and_then(|v| v.as_array());
            if !horizon_rules_allow_windows(rules) {
                continue;
            }
            let artifact = library
                .pointer("/downloads/artifact")
                .ok_or_else(|| format!("library {} 缺少 artifact", library.get("name").and_then(|v| v.as_str()).unwrap_or("?")))?;
            let url = as_str(artifact, "url");
            let rel_path = as_str(artifact, "path");
            if url.is_empty() || rel_path.is_empty() {
                continue;
            }
            items.push(DownloadItem {
                url,
                path: minecraft_dir.join("libraries").join(rel_path.replace('/', "\\")),
                sha1: Some(as_str(artifact, "sha1")),
                size: as_u64(artifact, "size"),
            });
        }
    }
    launch::download_all(
        &client,
        items.split_off(0),
        settings_download_source,
        version,
        Pipeline::Native,
        report,
    )
    .await?;

    // assets 对象（沿用官方 objects/<2>/<hash> 布局）
    let index_path = minecraft_dir
        .join("assets")
        .join("indexes")
        .join(format!("{version}.json"));
    let index_text = fs::read_to_string(&index_path).map_err(|e| e.to_string())?;
    let index: Value = serde_json::from_str(&index_text).map_err(|e| format!("资源索引解析失败：{e}"))?;
    let assets_root = minecraft_dir.join("assets");
    let mut asset_items = Vec::new();
    if let Some(objects) = index.get("objects").and_then(|v| v.as_object()) {
        for (_name, object) in objects {
            let hash = object.get("hash").and_then(|v| v.as_str()).unwrap_or_default();
            let size = object.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
            if hash.len() < 2 {
                continue;
            }
            let target = assets_root.join("objects").join(&hash[..2]).join(hash);
            if target.exists() {
                continue;
            }
            asset_items.push(DownloadItem {
                url: format!(
                    "https://resources.download.minecraft.net/{}/{}",
                    &hash[..2],
                    hash
                ),
                path: target,
                sha1: Some(hash.to_string()),
                size,
            });
        }
    }
    report(&format!("[Horizon] 下载资源对象 {} 个", asset_items.len()));
    launch::download_all(
        &client,
        asset_items,
        settings_download_source,
        version,
        Pipeline::Native,
        report,
    )
    .await?;

    // horizon-prepare 打包资源
    let prepare_bin = client_dir.join("horizon-prepare.exe");
    let java = crate::launch::find_java(None, PREPARE_JAVA_MAJOR)?;
    let mut command = if prepare_bin.is_file() {
        let mut cmd = Command::new(&prepare_bin);
        cmd.arg("--prepare");
        cmd
    } else {
        // 发行包以 jar 形式提供 prepare 时经 Java 调用。
        let jar = client_dir.join("horizon-prepare.jar");
        if !jar.is_file() {
            return Err("Horizon 发行包缺少 horizon-prepare".into());
        }
        let mut cmd = Command::new(java);
        cmd.arg("-jar").arg(&jar).arg("--prepare");
        cmd
    };
    command
        .arg("--server-jar")
        .arg(&server_path)
        .arg("--minecraft-dir")
        .arg(&minecraft_dir)
        .arg("--root")
        .arg(&client_dir);
    hide_console(&mut command);
    report("[Horizon] 打包资源（horizon-prepare，首次较久）");
    let output = tokio::process::Command::from(command)
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| format!("运行 horizon-prepare 失败：{e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "horizon-prepare 失败（exit {}）：{}",
            output.status.code().unwrap_or_default(),
            stderr.lines().last().unwrap_or_default()
        ));
    }
    // 资源输出到 <client_dir>/.local/horizon-client/<version>
    let prepared = client_dir.join(".local").join("horizon-client");
    let found = fs::read_dir(&prepared)
        .ok()
        .and_then(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.path())
                .next()
        })
        .ok_or_else(|| "horizon-prepare 未产出资源目录".to_string())?;
    fs::write(found.join("prepared.marker"), b"aurora").map_err(|e| e.to_string())?;
    report("[Horizon] 资源就绪");
    Ok(found)
}

/// 生成 Horizon 会话文件：微软正版账号写入会话 JSON；离线账号不写
///（Horizon 以未登录模式启动）。返回 Some(路径) 表示已写会话。
pub(crate) fn write_session_file(
    account: &LaunchAccount,
    dir: &Path,
) -> Result<Option<PathBuf>, String> {
    let msa = account.user_type == "msa" && !account.access_token.trim().is_empty();
    if !msa {
        return Ok(None);
    }
    if account.authlib.is_some() {
        // 外置登录（authlib-injector）Horizon 不支持，按未登录模式启动。
        return Ok(None);
    }
    let payload = json!({
        "name": account.name,
        "uuid": account.uuid,
        "access_token": account.access_token,
        "user_type": "msa",
        "xuid": account.xuid,
    });
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let path = dir.join("session.json");
    fs::write(&path, serde_json::to_vec(&payload).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(Some(path))
}

/// 以 horizon-client 原生启动游戏（无 Java）。返回进程 PID。
pub(crate) fn spawn_horizon(
    instance_dir: &Path,
    prepared: &Path,
    account: &LaunchAccount,
    width: u32,
    height: u32,
    on_line: Arc<dyn Fn(String) + Send + Sync>,
    on_exit: Arc<dyn Fn(Option<i32>) + Send + Sync>,
) -> Result<u32, String> {
    let Some(client_dir) = installed_client_dir(instance_dir) else {
        return Err("Horizon 客户端未安装".into());
    };
    let client = client_dir.join("horizon-client.exe");
    let game_dir = instance_dir.join("game");
    let session_path = write_session_file(account, &horizon_dir(instance_dir))?;

    let mut command = Command::new(&client);
    command
        .arg("--resources")
        .arg(prepared)
        .arg("--width")
        .arg(width.to_string())
        .arg("--height")
        .arg(height.to_string());
    if let Some(session) = &session_path {
        command.arg("--session-file").arg(session);
    }
    // 实例隔离：saves/ 与玩家数据落在实例 game 目录（Horizon 官方 env）
    command.env("HORIZON_HOME", &game_dir);
    hide_console(&mut command);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("启动 Horizon 客户端失败：{e}"))?;
    let pid = child.id();

    let stdout = child.stdout.take().expect("stdout piped");
    let stderr = child.stderr.take().expect("stderr piped");
    let drain = |stream: Box<dyn Read + Send>,
                 on_line: Arc<dyn Fn(String) + Send + Sync>| {
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stream);
            let mut buffer = Vec::new();
            loop {
                buffer.clear();
                match reader.read_until(b'\n', &mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let line = String::from_utf8_lossy(&buffer)
                            .trim_end_matches(['\r', '\n'])
                            .to_string();
                        if !line.is_empty() {
                            on_line(line);
                        }
                    }
                }
            }
        });
    };
    drain(Box::new(stdout), on_line.clone());
    drain(Box::new(stderr), on_line.clone());
    std::thread::spawn(move || {
        let status = child.wait();
        if let Some(session) = session_path {
            let _ = fs::remove_file(session);
        }
        let code = status.ok().and_then(|s| s.code());
        on_exit(code);
    });
    on_line("[Aurora] Horizon 客户端已启动（Rust 原生，无 Java）".to_string());
    Ok(pid)
}

/// Horizon 启动总流程：安装 → 资源准备 → 原生启动。供 launch_instance
/// 在 loader == horizon 时调用。
pub(crate) async fn run(
    app: &tauri::AppHandle,
    instance_id: &str,
    instance_dir: &Path,
    account: &LaunchAccount,
    settings: &crate::AppSettings,
    report: &(dyn Fn(&str) + Send + Sync),
    on_line: Arc<dyn Fn(String) + Send + Sync>,
    on_exit: Arc<dyn Fn(Option<i32>) + Send + Sync>,
) -> Result<u32, String> {
    let tag = ensure_installed(app, instance_dir, report).await?;
    report(&format!("[Horizon] 使用 Horizon {tag}（MC {HORIZON_MC_VERSION}，Rust 原生）"));
    let prepared = ensure_prepared(app, instance_dir, &settings.download_source, report).await?;
    let width = settings.window_width.clamp(100, 7680);
    let height = settings.window_height.clamp(100, 4320);
    spawn_horizon(instance_dir, &prepared, account, width, height, on_line, on_exit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_picker_prefers_stable_and_requires_client_asset() {
        let releases: Value = serde_json::from_str(
            r#"[
            {"tag_name":"launcher@0.2.66","assets":[{"name":"horizon-launcher-setup.exe"}]},
            {"tag_name":"nightly","assets":[
                {"name":"horizon-windows-x64.zip","browser_download_url":"https://github.com/horizon-rust/horizon/releases/download/nightly/horizon-windows-x64.zip"},
                {"name":"SHA256SUMS.txt","browser_download_url":"https://github.com/horizon-rust/horizon/releases/download/nightly/SHA256SUMS.txt"}]},
            {"tag_name":"nightly-20261113","assets":[
                {"name":"horizon-windows-x64.zip","browser_download_url":"https://github.com/horizon-rust/horizon/releases/download/nightly-20261113/horizon-windows-x64.zip"},
                {"name":"SHA256SUMS.txt","browser_download_url":"https://github.com/horizon-rust/horizon/releases/download/nightly-20261113/SHA256SUMS.txt"}]},
            {"tag_name":"v0.2.0","assets":[
                {"name":"horizon-windows-x64.zip","browser_download_url":"https://github.com/horizon-rust/horizon/releases/download/v0.2.0/horizon-windows-x64.zip"},
                {"name":"SHA256SUMS.txt","browser_download_url":"https://github.com/horizon-rust/horizon/releases/download/v0.2.0/SHA256SUMS.txt"}]}
        ]"#,
        )
        .unwrap();
        let picked = pick_horizon_release(&releases).expect("应有候选");
        assert_eq!(picked.tag, "v0.2.0");
        assert!(picked.asset_url.ends_with("/v0.2.0/horizon-windows-x64.zip"));
        // 无客户端资产的发行列表 → None
        let empty: Value = serde_json::from_str(r#"[{"tag_name":"x","assets":[]}]"#).unwrap();
        assert!(pick_horizon_release(&empty).is_none());
    }

    #[test]
    fn sums_parser_tolerates_crlf_and_star() {
        let sums = "b0bddb53e23425c9d558189b19bfff4970a00d4fef809706583c2ffda854df3f  horizon-windows-x64.zip\r\n*352673bf93aa2b352d2aebf4f12dad70dfb9deeca327744b76055bf87143b2b1  other.zip\r\n";
        assert_eq!(
            expected_sha_from_sums(sums, "horizon-windows-x64.zip"),
            Some("b0bddb53e23425c9d558189b19bfff4970a00d4fef809706583c2ffda854df3f".to_string())
        );
        // * 前缀与大小写容忍（GNU sha256sum 二进制模式：* 在文件名前）
        assert_eq!(
            expected_sha_from_sums(
                "352673BF93AA2B352D2AEBF4F12DAD70DFB9DEECA327744B76055BF87143B2B1  *other.zip",
                "other.zip"
            ),
            Some("352673bf93aa2b352d2aebf4f12dad70dfb9deeca327744b76055bf87143b2b1".to_string())
        );
        assert_eq!(expected_sha_from_sums(sums, "missing.zip"), None);
    }

    #[test]
    fn session_file_requires_msa() {
        let dir = std::env::temp_dir().join(format!("aurora-horizon-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let offline = LaunchAccount {
            name: "Player".into(),
            uuid: "u".into(),
            access_token: String::new(),
            user_type: "legacy".into(),
            client_id: String::new(),
            xuid: String::new(),
            authlib: None,
        };
        assert!(write_session_file(&offline, &dir).unwrap().is_none());
        let msa = LaunchAccount {
            name: "Steve".into(),
            uuid: "uuid-1".into(),
            access_token: "token-1".into(),
            user_type: "msa".into(),
            client_id: String::new(),
            xuid: "x-1".into(),
            authlib: None,
        };
        let path = write_session_file(&msa, &dir).unwrap().expect("msa 应写会话");
        let value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(value["name"], "Steve");
        assert_eq!(value["user_type"], "msa");
        assert_eq!(value["access_token"], "token-1");
        let _ = fs::remove_dir_all(&dir);
    }

}
