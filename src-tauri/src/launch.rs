use crate::{hide_console, safe_filename, write_atomic};
use futures::stream::{self, StreamExt};
use serde::Deserialize;
use serde_json::Value;
use sha1::{Digest, Sha1};
use std::{
    collections::HashMap,
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};
use zip::ZipArchive;

#[derive(Debug, Clone)]
pub struct LaunchContext {
    pub instance_dir: PathBuf,
    pub game_dir: PathBuf,
    pub assets_root: PathBuf,
    pub libraries_root: PathBuf,
    pub shared_root: Option<PathBuf>,
    pub version_id: String,
    pub java: Option<PathBuf>,
    pub memory_mb: u32,
    pub username: String,
    pub uuid: String,
    pub access_token: String,
    pub user_type: String,
    pub client_id: String,
    pub xuid: String,
    pub authlib: Option<crate::yggdrasil::LaunchAuthlib>,
    pub ai_args: Vec<String>,
    pub download_source: String,
    pub proxy: String,
    /// 启动管线：native（默认，Rust 原生快速管线）| compat（兼容模式，
    /// 完全沿用 0.6.9 及之前的下载/参数行为，供出问题时回退对比）。
    pub pipeline: String,
    /// JVM 优化预设与额外参数（仅 native 生效）。
    pub jvm_optimize: bool,
    pub jvm_gc: String,
    pub extra_jvm_args: String,
    pub extra_game_args: String,
    /// 默认窗口尺寸与全屏（替换旧硬编码 854×480）。
    pub window_width: u32,
    pub window_height: u32,
    pub fullscreen: bool,
}

/// 启动管线模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pipeline {
    Native,
    Compat,
}

pub(crate) fn pipeline_of(value: &str) -> Pipeline {
    if value.trim() == "compat" {
        Pipeline::Compat
    } else {
        Pipeline::Native
    }
}

#[derive(Debug)]
pub struct Prepared {
    pub java: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
}

#[derive(Debug, Deserialize, Clone, Default)]
struct VersionJson {
    id: String,
    #[serde(rename = "inheritsFrom")]
    inherits_from: Option<String>,
    #[serde(rename = "mainClass")]
    main_class: Option<String>,
    #[serde(rename = "minecraftArguments")]
    minecraft_arguments: Option<String>,
    arguments: Option<Arguments>,
    libraries: Option<Vec<Library>>,
    #[serde(rename = "assetIndex")]
    asset_index: Option<AssetIndex>,
    assets: Option<String>,
    downloads: Option<VersionDownloads>,
    #[serde(rename = "javaVersion")]
    java_version: Option<JavaVersion>,
    logging: Option<Logging>,
    #[serde(rename = "type")]
    version_type: Option<String>,
    #[serde(skip)]
    client_jar_id: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
struct Arguments {
    game: Option<Vec<Value>>,
    jvm: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize, Clone)]
struct Library {
    name: String,
    downloads: Option<LibraryDownloads>,
    natives: Option<HashMap<String, String>>,
    rules: Option<Vec<Rule>>,
    extract: Option<Extract>,
    url: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
struct LibraryDownloads {
    artifact: Option<Artifact>,
    classifiers: Option<HashMap<String, Artifact>>,
}

#[derive(Debug, Deserialize, Clone)]
struct Artifact {
    path: Option<String>,
    sha1: Option<String>,
    size: Option<u64>,
    url: String,
}

#[derive(Debug, Deserialize, Clone)]
struct Extract {
    exclude: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Clone)]
struct Rule {
    action: String,
    os: Option<OsRule>,
    features: Option<HashMap<String, bool>>,
}

#[derive(Debug, Deserialize, Clone)]
struct OsRule {
    name: Option<String>,
    arch: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
struct AssetIndex {
    id: String,
    sha1: Option<String>,
    size: Option<u64>,
    url: String,
}

#[derive(Debug, Deserialize, Clone, Default)]
struct VersionDownloads {
    client: Option<Artifact>,
}

#[derive(Debug, Deserialize, Clone)]
struct JavaVersion {
    #[serde(rename = "majorVersion")]
    major_version: u32,
}

#[derive(Debug, Deserialize, Clone)]
struct Logging {
    client: Option<LoggingClient>,
}

#[derive(Debug, Deserialize, Clone)]
struct LoggingClient {
    argument: Option<String>,
    file: Option<LoggingFile>,
}

#[derive(Debug, Deserialize, Clone)]
struct LoggingFile {
    id: String,
    sha1: Option<String>,
    size: Option<u64>,
    url: String,
}

#[derive(Debug, Deserialize, Clone)]
struct AssetObjects {
    objects: HashMap<String, AssetObject>,
    #[serde(default, rename = "virtual")]
    virtual_: bool,
    #[serde(default)]
    map_to_resources: bool,
}

#[derive(Debug, Deserialize, Clone)]
struct AssetObject {
    hash: String,
    size: u64,
}

struct DownloadItem {
    url: String,
    path: PathBuf,
    sha1: Option<String>,
    size: u64,
}

fn merge_child(mut parent: VersionJson, child: VersionJson) -> VersionJson {
    if let Some(libs) = child.libraries.clone() {
        let mut merged = parent.libraries.take().unwrap_or_default();
        merged.extend(libs);
        parent.libraries = Some(merged);
    }
    if let Some(child_args) = child.arguments.clone() {
        let mut args = parent.arguments.take().unwrap_or_default();
        if let Some(game) = child_args.game {
            args.game.get_or_insert_with(Vec::new).extend(game);
        }
        if let Some(jvm) = child_args.jvm {
            args.jvm.get_or_insert_with(Vec::new).extend(jvm);
        }
        parent.arguments = Some(args);
    }
    if child.minecraft_arguments.is_some() {
        parent.minecraft_arguments = child.minecraft_arguments;
    }
    if child.main_class.is_some() {
        parent.main_class = child.main_class;
    }
    if child.asset_index.is_some() {
        parent.asset_index = child.asset_index;
    }
    if child.assets.is_some() {
        parent.assets = child.assets;
    }
    if let Some(downloads) = child.downloads {
        parent.downloads = Some(downloads);
        parent.client_jar_id = Some(child.id.clone());
    }
    if child.java_version.is_some() {
        parent.java_version = child.java_version;
    }
    if child.logging.is_some() {
        parent.logging = child.logging;
    }
    if child.version_type.is_some() {
        parent.version_type = child.version_type;
    }
    parent.id = child.id;
    // 保留继承来源：合并结果代表的是「继承型」版本，加载器启动时据此
    // 不放原版 client jar（见 json_provides_client）。
    if parent.inherits_from.is_none() {
        parent.inherits_from = child.inherits_from.clone();
    }
    parent
}

fn load_version(ctx: &LaunchContext, id: &str, depth: usize) -> Result<VersionJson, String> {
    if depth > 8 {
        return Err("版本继承层级过深".into());
    }
    let instance_path = ctx
        .game_dir
        .join("versions")
        .join(id)
        .join(format!("{id}.json"));
    let shared_path = ctx
        .shared_root
        .as_ref()
        .map(|root| root.join("versions").join(id).join(format!("{id}.json")));
    let path = if instance_path.exists() {
        instance_path
    } else if let Some(shared) = shared_path.filter(|value| value.exists()) {
        shared
    } else {
        return Err(format!("找不到版本 JSON：{id}"));
    };
    let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    let mut version: VersionJson =
        serde_json::from_str(&text).map_err(|e| format!("版本 JSON 解析失败：{e}"))?;
    if version.id.is_empty() {
        version.id = id.to_string();
    }
    if version.client_jar_id.is_none() {
        version.client_jar_id = Some(version.id.clone());
    }
    if let Some(parent_id) = version.inherits_from.clone() {
        let parent = load_version(ctx, &parent_id, depth + 1)?;
        version = merge_child(parent, version);
    }
    Ok(version)
}

fn rule_matches(rule: &Rule, features: &HashMap<String, bool>) -> bool {
    if let Some(os) = &rule.os {
        if let Some(name) = &os.name {
            if name != "windows" {
                return false;
            }
        }
        if let Some(arch) = &os.arch {
            if arch != "x86_64" {
                return false;
            }
        }
    }
    if let Some(required) = &rule.features {
        for (key, value) in required {
            if features.get(key).copied().unwrap_or(false) != *value {
                return false;
            }
        }
    }
    true
}

fn rules_allow(rules: &Option<Vec<Rule>>, features: &HashMap<String, bool>) -> bool {
    let Some(rules) = rules else {
        return true;
    };
    if rules.is_empty() {
        return true;
    }
    let mut allowed = false;
    for rule in rules {
        if rule_matches(rule, features) {
            allowed = rule.action == "allow";
        }
    }
    allowed
}

fn collect_args(items: &[Value], features: &HashMap<String, bool>) -> Vec<String> {
    let mut result = Vec::new();
    for item in items {
        if let Some(value) = item.as_str() {
            result.push(value.to_string());
            continue;
        }
        let rules = item.get("rules").and_then(|value| value.as_array());
        let allowed = match rules {
            None => true,
            Some(rules) => {
                let mut allowed = false;
                for rule in rules {
                    let action = rule
                        .get("action")
                        .and_then(|value| value.as_str())
                        .unwrap_or("allow");
                    let matches = rule
                        .get("os")
                        .and_then(|os| os.get("name"))
                        .and_then(|value| value.as_str())
                        .map(|name| name == "windows")
                        .unwrap_or(true)
                        && rule
                            .get("features")
                            .and_then(|value| value.as_object())
                            .map(|required| {
                                required.iter().all(|(key, value)| {
                                    features.get(key).copied().unwrap_or(false)
                                        == value.as_bool().unwrap_or(false)
                                })
                            })
                            .unwrap_or(true);
                    if matches {
                        allowed = action == "allow";
                    }
                }
                allowed
            }
        };
        if !allowed {
            continue;
        }
        if let Some(value) = item.get("value") {
            if let Some(text) = value.as_str() {
                result.push(text.to_string());
            } else if let Some(values) = value.as_array() {
                for text in values {
                    if let Some(text) = text.as_str() {
                        result.push(text.to_string());
                    }
                }
            }
        }
    }
    result
}

fn maven_path(name: &str) -> Option<String> {
    let parts: Vec<&str> = name.split(':').collect();
    if parts.len() < 3 {
        return None;
    }
    let group = parts[0].replace('.', "/");
    let artifact = parts[1];
    let version = parts[2];
    let classifier = parts.get(3).filter(|value| !value.is_empty());
    let file = match classifier {
        Some(classifier) => format!("{artifact}-{version}-{classifier}.jar"),
        None => format!("{artifact}-{version}.jar"),
    };
    Some(format!("{group}/{artifact}/{version}/{file}"))
}

fn library_artifact(library: &Library, classifier: Option<&str>) -> Option<Artifact> {
    if let Some(downloads) = &library.downloads {
        if let Some(classifier) = classifier {
            if let Some(artifact) = downloads
                .classifiers
                .as_ref()
                .and_then(|values| values.get(classifier))
            {
                return Some(artifact.clone());
            }
        } else if let Some(artifact) = &downloads.artifact {
            return Some(artifact.clone());
        }
    }
    let path = match classifier {
        Some(classifier) => maven_path(&format!("{}:{}", library.name, classifier))?,
        None => maven_path(&library.name)?,
    };
    let base = library
        .url
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "https://libraries.minecraft.net/".into());
    Some(Artifact {
        url: format!("{}{}", base.trim_end_matches('/'), format!("/{path}")),
        path: Some(path),
        sha1: None,
        size: None,
    })
}

fn native_classifier(library: &Library) -> Option<String> {
    let template = library.natives.as_ref()?.get("windows")?;
    Some(template.replace("${arch}", "64"))
}

fn resolve_existing(ctx: &LaunchContext, relative: &str) -> Option<PathBuf> {
    let instance = ctx.libraries_root.join(relative);
    if instance.exists() {
        return Some(instance);
    }
    if let Some(shared) = &ctx.shared_root {
        let path = shared.join("libraries").join(relative);
        if path.exists() {
            return Some(path);
        }
    }
    None
}

fn existing_version_file(ctx: &LaunchContext, id: &str, name: &str) -> Option<PathBuf> {
    let instance = ctx.game_dir.join("versions").join(id).join(name);
    if instance.exists() {
        return Some(instance);
    }
    if let Some(shared) = &ctx.shared_root {
        let path = shared.join("versions").join(id).join(name);
        if path.exists() {
            return Some(path);
        }
    }
    None
}

/// 把官方 URL 映射到自定义镜像（BMCLAPI 兼容布局）上的路径。
/// 返回 None 表示该 URL 在镜像上没有对应路径，调用方应只用官方。
pub(crate) fn mirror_url_at(base: &str, original: &str, version_id: &str) -> Option<String> {
    let base = base.trim_end_matches('/');
    let url = reqwest::Url::parse(original).ok()?;
    let host = url.host_str()?;
    let path = url.path();
    let mapped = match host {
        // 版本清单 / 版本 JSON / asset index：piston-meta → /mc/game、/v1/packages
        "piston-meta.mojang.com" | "launchermeta.mojang.com" => {
            Some(format!("{base}{path}"))
        }
        // client jar 等：piston-data /v1/objects/{hash}/client.jar
        "piston-data.mojang.com" => Some(format!("{base}{path}")),
        // assets：/assets/{2}/{hash}
        "resources.download.minecraft.net" => {
            let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
            if parts.len() == 2 {
                Some(format!("{base}/assets/{}/{}", parts[0], parts[1]))
            } else {
                None
            }
        }
        // libraries 与 Forge：/maven/...（Forge 官方 URL 自带 /maven/ 前缀）
        "libraries.minecraft.net" => Some(format!("{base}/maven{path}")),
        "maven.minecraftforge.net" => Some(format!("{base}{path}")),
        // NeoForge：镜像布局去掉 /releases 前缀，补 /maven/
        "maven.neoforged.net" => Some(format!(
            "{base}/maven{}",
            path.trim_start_matches("/releases")
        )),
        _ => None,
    };
    let _ = version_id;
    mapped
}

pub(crate) fn mirror_url(original: &str, version_id: &str) -> Option<String> {
    let url = reqwest::Url::parse(original).ok()?;
    let host = url.host_str()?;
    let path = url.path();
    match host {
        "piston-meta.mojang.com" | "launchermeta.mojang.com" => {
            Some(format!("https://bmclapi2.bangbang93.com{path}"))
        }
        "piston-data.mojang.com" => {
            if path.ends_with("/client.jar") {
                Some(format!(
                    "https://bmclapi2.bangbang93.com/version/{version_id}/client"
                ))
            } else {
                Some(format!("https://bmclapi2.bangbang93.com{path}"))
            }
        }
        "resources.download.minecraft.net" => {
            let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
            if parts.len() == 2 {
                Some(format!(
                    "https://bmclapi2.bangbang93.com/assets/{}/{}",
                    parts[0], parts[1]
                ))
            } else {
                None
            }
        }
        "libraries.minecraft.net" => Some(format!("https://bmclapi2.bangbang93.com/maven{path}")),
        // BMCLAPI also mirrors the Forge/NeoForge maven repositories (verified
        // by redirect to their mirror nodes). NeoForge lives under the
        // repository's /releases/ prefix, which BMCLAPI drops when proxying.
        "maven.minecraftforge.net" => {
            Some(format!("https://bmclapi2.bangbang93.com/maven{path}"))
        }
        "maven.neoforged.net" => Some(format!(
            "https://bmclapi2.bangbang93.com/maven{}",
            path.trim_start_matches("/releases")
        )),
        _ => None,
    }
}

pub(crate) fn candidates(url: &str, source: &str, version_id: &str) -> Vec<String> {
    // custom:<base>：自定义镜像（BMCLAPI 兼容布局）优先，官方兜底。
    if let Some(base) = source.strip_prefix("custom:") {
        if !base.trim().is_empty() {
            return match mirror_url_at(base.trim(), url, version_id) {
                Some(mirror) => vec![mirror, url.to_string()],
                None => vec![url.to_string()],
            };
        }
    }
    let mirror = mirror_url(url, version_id);
    match source {
        "bmclapi" => mirror
            .map(|mirror| vec![mirror, url.to_string()])
            .unwrap_or_else(|| vec![url.to_string()]),
        "auto" => mirror
            .map(|mirror| vec![url.to_string(), mirror])
            .unwrap_or_else(|| vec![url.to_string()]),
        _ => vec![url.to_string()],
    }
}

/// Probe both download sources and return the effective one for `auto`.
/// Result is cached for 10 minutes so a launch only pays for one probe.
pub(crate) async fn resolve_download_source(
    client: &reqwest::Client,
    setting: &str,
) -> String {
    match setting {
        "bmclapi" => return "bmclapi".into(),
        "official" => return "official".into(),
        other => {
            if other.starts_with("custom:") {
                return other.to_string();
            }
        }
    }
    static CACHE: std::sync::OnceLock<Mutex<Option<(std::time::Instant, String)>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    if let Some((at, source)) = cache.lock().ok().and_then(|guard| guard.clone()) {
        if at.elapsed() < std::time::Duration::from_secs(600) {
            return source;
        }
    }
    let official_probe = probe_source(
        client,
        "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
    );
    let mirror_probe = probe_source(
        client,
        "https://bmclapi2.bangbang93.com/mc/game/version_manifest_v2.json",
    );
    let (official, mirror) = futures::join!(official_probe, mirror_probe);
    let source = pick_download_source(official, mirror);
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((std::time::Instant::now(), source.clone()));
    }
    source
}

async fn probe_source(client: &reqwest::Client, url: &str) -> Option<std::time::Duration> {
    let start = std::time::Instant::now();
    let response = client
        .get(url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    Some(start.elapsed())
}

/// Prefer whichever source answered (and answered faster); fall back to the
/// official endpoint when both fail.
fn pick_download_source(
    official: Option<std::time::Duration>,
    mirror: Option<std::time::Duration>,
) -> String {
    match (official, mirror) {
        (Some(a), Some(b)) => {
            if b < a {
                "bmclapi".into()
            } else {
                "official".into()
            }
        }
        (Some(_), None) => "official".into(),
        (None, Some(_)) => "bmclapi".into(),
        (None, None) => "official".into(),
    }
}

/// 单候选 URL 的流式下载结果：Terminal 表示该 URL 不值得重试（4xx），
/// Retryable 表示网络/校验类瞬时错误。
enum DownloadFail {
    Terminal(String),
    Retryable(String),
}

impl From<std::io::Error> for DownloadFail {
    fn from(error: std::io::Error) -> Self {
        DownloadFail::Retryable(error.to_string())
    }
}

/// 原生管线单 URL 下载：流式写 `.aurora.part` + 边写边算 SHA-1，
/// 4xx 终止、5xx/网络/校验错误重试（3 次，线性退避）。
/// 与 write_atomic 同名约定，rename 前文件不可见，无半成品落盘。
async fn download_streaming(
    client: &reqwest::Client,
    url: &str,
    item: &DownloadItem,
) -> Result<(), String> {
    let mut last_error = String::new();
    for attempt in 1..=3 {
        if attempt > 1 {
            tokio::time::sleep(std::time::Duration::from_millis(600 * attempt as u64)).await;
        }
        match try_download_stream(client, url, item).await {
            Ok(()) => return Ok(()),
            Err(DownloadFail::Terminal(message)) => return Err(message),
            Err(DownloadFail::Retryable(message)) => last_error = message,
        }
    }
    Err(last_error)
}

async fn try_download_stream(
    client: &reqwest::Client,
    url: &str,
    item: &DownloadItem,
) -> Result<(), DownloadFail> {
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|error| DownloadFail::Retryable(error.to_string()))?;
    let status = response.status();
    if !status.is_success() {
        let message = format!("HTTP {}", status.as_u16());
        return Err(if (400..500).contains(&status.as_u16()) {
            DownloadFail::Terminal(message)
        } else {
            DownloadFail::Retryable(message)
        });
    }
    let part_path = item.path.with_extension("aurora.part");
    if let Some(parent) = item.path.parent() {
        fs::create_dir_all(parent).map_err(DownloadFail::from)?;
    }
    let mut file = File::create(&part_path).map_err(|error| {
        DownloadFail::Retryable(format!("写入失败：{error}"))
    })?;
    let mut hasher = Sha1::new();
    let mut downloaded: u64 = 0;
    let mut response = response;
    loop {
        let chunk = response
            .chunk()
            .await
            .map_err(|error| DownloadFail::Retryable(error.to_string()))?;
        let Some(chunk) = chunk else { break };
        hasher.update(&chunk);
        downloaded += chunk.len() as u64;
        file.write_all(&chunk).map_err(|error| {
            DownloadFail::Retryable(format!("写入失败：{error}"))
        })?;
    }
    drop(file);
    if item.size > 0 && downloaded != item.size {
        let _ = fs::remove_file(&part_path);
        return Err(DownloadFail::Retryable("文件大小校验失败".into()));
    }
    if let Some(expected) = &item.sha1 {
        let actual = hex::encode(hasher.finalize());
        if actual != expected.to_lowercase() {
            let _ = fs::remove_file(&part_path);
            return Err(DownloadFail::Retryable("SHA-1 校验失败".into()));
        }
    }
    fs::rename(&part_path, &item.path)
        .map_err(|error| DownloadFail::Terminal(format!("写入失败：{error}")))?;
    Ok(())
}

async fn download_one(
    client: &reqwest::Client,
    item: &DownloadItem,
    source: &str,
    version_id: &str,
    pipeline: Pipeline,
) -> Result<bool, String> {
    if item.path.exists() {
        if item.size > 0 {
            if let Ok(meta) = fs::metadata(&item.path) {
                if meta.len() == item.size {
                    return Ok(false);
                }
            }
        } else if item.sha1.is_none() {
            return Ok(false);
        }
    }
    let urls = candidates(&item.url, source, version_id);
    let mut last_error = String::new();
    match pipeline {
        // 原生管线：每候选 URL 独立重试 + 流式校验；候选仍按顺序兜底
        //（镜像 404 立即切官方，不浪费时间重试坏 URL）。
        Pipeline::Native => {
            for url in urls {
                match download_streaming(client, &url, item).await {
                    Ok(()) => return Ok(true),
                    Err(error) => last_error = error,
                }
            }
        }
        // 兼容管线：0.6.9 及之前的旧逻辑——整块缓冲、每候选一次尝试。
        Pipeline::Compat => {
            for url in urls {
                match client.get(&url).send().await {
                    Ok(response) => {
                        if !response.status().is_success() {
                            last_error = format!("HTTP {}", response.status().as_u16());
                            continue;
                        }
                        let bytes = match response.bytes().await {
                            Ok(bytes) => bytes,
                            Err(error) => {
                                last_error = error.to_string();
                                continue;
                            }
                        };
                        if item.size > 0 && bytes.len() as u64 != item.size {
                            last_error = "文件大小校验失败".into();
                            continue;
                        }
                        if let Some(expected) = &item.sha1 {
                            let actual = hex::encode(Sha1::digest(&bytes));
                            if actual != expected.to_lowercase() {
                                last_error = "SHA-1 校验失败".into();
                                continue;
                            }
                        }
                        if let Some(parent) = item.path.parent() {
                            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                        }
                        write_atomic(&item.path, &bytes)?;
                        return Ok(true);
                    }
                    Err(error) => {
                        last_error = error.to_string();
                    }
                }
            }
        }
    }
    Err(format!(
        "下载失败：{}（{}）",
        item.path.display(),
        last_error
    ))
}

async fn download_all(
    client: &reqwest::Client,
    items: Vec<DownloadItem>,
    source: &str,
    version_id: &str,
    pipeline: Pipeline,
    report: &(dyn Fn(&str) + Send + Sync),
) -> Result<(), String> {
    let total = items.len();
    if total == 0 {
        return Ok(());
    }
    // 原生管线并发翻倍/提三倍（官方源文件小、数量多，瓶颈在请求数）；
    // 兼容管线保持旧值。bmclapi 有节点限流，适度提升。
    let concurrency = match pipeline {
        Pipeline::Native => {
            if source == "bmclapi" {
                6
            } else {
                16
            }
        }
        Pipeline::Compat => {
            if source == "bmclapi" {
                2
            } else {
                8
            }
        }
    };
    let done = Arc::new(AtomicUsize::new(0));
    report(&format!("需要检查/下载 {total} 个文件"));
    let results = stream::iter(items.into_iter().map(|item| {
        let client = client.clone();
        let done = done.clone();
        let source = source.to_string();
        let version_id = version_id.to_string();
        async move {
            let result = download_one(&client, &item, &source, &version_id, pipeline).await;
            let current = done.fetch_add(1, Ordering::Relaxed) + 1;
            if current % 25 == 0 || current == total {
                report(&format!("下载进度 {current}/{total}"));
            }
            result
        }
    }))
    .buffer_unordered(concurrency)
    .collect::<Vec<Result<bool, String>>>()
    .await;
    for result in results {
        result?;
    }
    Ok(())
}

fn extract_natives(
    archive_path: &Path,
    natives_dir: &Path,
    exclude: &[String],
    report: &(dyn Fn(&str) + Send + Sync),
) -> Result<(), String> {
    let file = File::open(archive_path).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|e| format!("natives 压缩包打开失败：{e}"))?;
    fs::create_dir_all(natives_dir).map_err(|e| e.to_string())?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        if name.ends_with('/') {
            continue;
        }
        if exclude
            .iter()
            .any(|prefix| name.starts_with(prefix.trim_start_matches('/')))
        {
            continue;
        }
        let Some(file_name) = safe_filename(&name) else {
            continue;
        };
        let target = natives_dir.join(file_name);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut output = File::create(&target).map_err(|e| e.to_string())?;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let read = entry.read(&mut buffer).map_err(|e| e.to_string())?;
            if read == 0 {
                break;
            }
            output
                .write_all(&buffer[..read])
                .map_err(|e| e.to_string())?;
        }
    }
    report(&format!("已解压 natives 到 {}", natives_dir.display()));
    Ok(())
}

fn java_major(path: &Path) -> Option<u32> {
    let mut command = Command::new(path);
    command.arg("-version");
    hide_console(&mut command);
    let output = command.output().ok()?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let start = text.find("version \"")? + "version \"".len();
    let rest = &text[start..];
    let end = rest.find('"')?;
    let version = &rest[..end];
    let mut parts = version.split('.');
    let first = parts.next()?;
    if first == "1" {
        parts.next()?.parse().ok()
    } else {
        first.parse().ok()
    }
}

fn collect_java_candidates(preferred: Option<&str>) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    let mut push = |path: PathBuf| {
        if path.exists() && !candidates.contains(&path) {
            candidates.push(path);
        }
    };
    if let Some(preferred) = preferred {
        push(PathBuf::from(preferred));
    }
    if let Ok(home) = std::env::var("JAVA_HOME") {
        push(PathBuf::from(home).join("bin").join("java.exe"));
    }
    let mut roots = vec![
        PathBuf::from("C:\\Program Files\\Java"),
        PathBuf::from("C:\\Program Files\\Eclipse Adoptium"),
        PathBuf::from("C:\\Program Files\\Microsoft"),
        PathBuf::from("C:\\Program Files\\BellSoft"),
    ];
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        roots.push(PathBuf::from(local).join("Programs").join("Java"));
    }
    for root in roots {
        if let Ok(entries) = fs::read_dir(root) {
            for entry in entries.flatten() {
                let java = entry.path().join("bin").join("java.exe");
                push(java);
            }
        }
    }
    candidates.push(PathBuf::from("java"));
    candidates
}

/// 候选 Java 探测缓存（5 分钟 TTL）：旧实现每次启动都串行 spawn 每个
/// 候选的 `java -version`，多 JDK 机器上白白花 1–3 秒；现并发探测且
/// 进程内缓存，命中后零开销。
fn probe_java_map(candidates: &[PathBuf]) -> HashMap<PathBuf, Option<u32>> {
    static CACHE: std::sync::OnceLock<
        Mutex<Option<(std::time::Instant, HashMap<PathBuf, Option<u32>>)>>,
    > = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let mut missing: Vec<PathBuf> = Vec::new();
    {
        let guard = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some((at, map)) = guard.as_ref() {
            if at.elapsed() < std::time::Duration::from_secs(300) {
                for candidate in candidates {
                    if !map.contains_key(candidate) {
                        missing.push(candidate.clone());
                    }
                }
            } else {
                missing.extend(candidates.iter().cloned());
            }
        } else {
            missing.extend(candidates.iter().cloned());
        }
    }
    if !missing.is_empty() {
        // 候选数量有限（个位数到十几），scoped 线程并发 spawn 探测。
        let results: Vec<(PathBuf, Option<u32>)> = std::thread::scope(|scope| {
            let handles: Vec<_> = missing
                .iter()
                .map(|path| {
                    let path = path.clone();
                    scope.spawn(move || (path.clone(), java_major(&path)))
                })
                .collect();
            handles
                .into_iter()
                .filter_map(|handle| handle.join().ok())
                .collect()
        });
        let mut guard = cache
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let expired = match guard.as_ref() {
            Some((at, _)) => at.elapsed() >= std::time::Duration::from_secs(300),
            None => true,
        };
        if expired {
            *guard = Some((std::time::Instant::now(), HashMap::new()));
        }
        let map = &mut guard.as_mut().unwrap().1;
        for (path, major) in results {
            map.entry(path).or_insert(major);
        }
    }
    let guard = cache
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let empty = HashMap::new();
    let map = guard
        .as_ref()
        .map(|(_, map)| map)
        .unwrap_or(&empty);
    candidates
        .iter()
        .map(|candidate| (candidate.clone(), map.get(candidate).copied().flatten()))
        .collect()
}

pub(crate) fn find_java(preferred: Option<&str>, required: u32) -> Result<PathBuf, String> {
    let candidates = collect_java_candidates(preferred);
    let probed = probe_java_map(&candidates);
    let mut detected: Vec<(u32, PathBuf)> = Vec::new();
    for candidate in candidates {
        if let Some(Some(major)) = probed.get(&candidate) {
            detected.push((*major, candidate));
        }
    }
    if detected.is_empty() {
        return Err("没有找到可用的 Java，请安装 Java 17/21 或在实例中指定 java.exe".into());
    }
    if let Some(preferred) = preferred {
        let preferred = PathBuf::from(preferred);
        if let Some((major, path)) = detected.iter().find(|(_, path)| {
            path == &preferred || path.to_string_lossy() == preferred.to_string_lossy()
        }) {
            if *major >= required {
                return Ok(path.clone());
            }
        }
    }
    if let Some((_, path)) = detected.iter().find(|(major, _)| *major == required) {
        return Ok(path.clone());
    }
    let mut higher: Vec<&(u32, PathBuf)> = detected
        .iter()
        .filter(|(major, _)| *major > required)
        .collect();
    higher.sort_by_key(|(major, _)| *major);
    if let Some((_, path)) = higher.first() {
        return Ok(path.clone());
    }
    let listed = detected
        .iter()
        .map(|(major, path)| format!("Java {major} ({})", path.display()))
        .collect::<Vec<_>>()
        .join("；");
    Err(format!(
        "需要 Java {required} 或更高版本，但只找到：{listed}"
    ))
}

fn replace_placeholders(value: &str, map: &HashMap<&str, String>) -> String {
    let mut result = value.to_string();
    for (key, replacement) in map {
        result = result.replace(&format!("${{{key}}}"), replacement);
    }
    result
}

fn build_classpath(client_jar: Option<&Path>, libraries: &[PathBuf]) -> String {
    let mut entries = Vec::new();
    if let Some(jar) = client_jar {
        entries.push(jar.display().to_string());
    }
    for library in libraries {
        entries.push(library.display().to_string());
    }
    entries.join(";")
}

/// 继承型版本 JSON（inheritsFrom，即 Forge/NeoForge/Fabric 安装器生成）
/// 是否需要把原版 client jar 放上 classpath。
///
/// 答案是否：Forge 的 BootstrapLauncher 用 --ignoreList + 模块层自己装配
/// 客户端（SRG 化 client 不在 libraries 里，由 installer 的 patch 体系在
/// 运行时生成）；原版 jar 若在 classpath 上会被当作自动模块
/// `_1._20._1`，与生成的 `minecraft` 模块导出同样的包，触发
/// ResolutionException。原版（无 inheritsFrom）才需要显式放上 client jar。
fn json_provides_client(version: &VersionJson) -> bool {
    // 继承型一律不放原版 jar（BootstrapLauncher 自行装配客户端）。
    if version.inherits_from.is_some() {
        return false;
    }
    // 原版 JSON 需要 client jar；防御：若其 libraries 已引用
    // net.minecraft:client（未来格式变化），也不重复放。
    !version
        .libraries
        .iter()
        .flatten()
        .any(|library| library.name.starts_with("net.minecraft:client"))
}

fn authlib_args(authlib: Option<&crate::yggdrasil::LaunchAuthlib>) -> Vec<String> {
    let Some(authlib) = authlib else {
        return Vec::new();
    };
    let mut args = vec![format!(
        "-javaagent:{}={}",
        authlib.jar.display(),
        authlib.server
    )];
    if !authlib.prefetched.is_empty() {
        args.push(format!(
            "-Dauthlibinjector.yggdrasil.prefetched={}",
            authlib.prefetched
        ));
    }
    args
}

/// GC 优化预设（HMCL 社区长期验证的 MC 调优集合，含 fml 证书容忍
/// flags，修复著名的老 Forge 警告）。ZGC 在 Windows 需 JDK 15+，
/// 21+ 追加 ZGenerational，不满足时回退 G1。返回 None 表示不注入。
pub(crate) fn gc_preset_args(gc: &str, java_major: u32) -> Option<Vec<String>> {
    let common: Vec<String> = vec![
        "-XX:-OmitStackTraceInFastThrow".into(),
        "-Dfml.ignoreInvalidMinecraftCertificates=true".into(),
        "-Dfml.ignorePatchDiscrepancies=true".into(),
    ];
    match gc {
        "g1" => {
            let mut flags = vec![
                // G1NewSizePercent/G1ReservePercent 是实验选项，解锁开关必须
                // 在其之前（冒烟实测 JVM 拒绝启动：The unlock option must
                // precede）。
                "-XX:+UnlockExperimentalVMOptions".into(),
                "-XX:+UseG1GC".into(),
                "-XX:-UseAdaptiveSizePolicy".into(),
                "-XX:G1NewSizePercent=20".into(),
                "-XX:G1ReservePercent=20".into(),
                "-XX:MaxGCPauseMillis=50".into(),
            ];
            flags.extend(common);
            Some(flags)
        }
        "zgc" => {
            if java_major < 15 {
                return gc_preset_args("g1", java_major);
            }
            let mut flags = vec!["-XX:+UseZGC".into()];
            if java_major >= 21 {
                flags.push("-XX:+ZGenerational".into());
            }
            flags.extend(common);
            Some(flags)
        }
        _ => None,
    }
}

/// 已存在的 asset index 解析；不存在返回 None（调用方按旧行为兜底）。
fn parse_asset_index_if_exists(path: &Path) -> Result<Option<AssetObjects>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| format!("资源索引解析失败：{e}"))
}

/// 从索引对象生成缺失的资源下载清单（已存在的按哈希路径跳过）。
fn build_asset_items(parsed: &AssetObjects, assets_root: &Path) -> Vec<DownloadItem> {
    let mut items = Vec::new();
    for (name, object) in &parsed.objects {
        if object.hash.len() < 2 {
            continue;
        }
        let relative = format!("objects/{}/{}", &object.hash[..2], object.hash);
        let target = assets_root.join(&relative);
        if target.exists() {
            continue;
        }
        items.push(DownloadItem {
            url: format!(
                "https://resources.download.minecraft.net/{}/{}",
                &object.hash[..2],
                object.hash
            ),
            path: target,
            sha1: Some(object.hash.clone()),
            size: object.size,
        });
        let _ = name;
    }
    items
}

pub async fn prepare(
    ctx: &LaunchContext,
    report: &(dyn Fn(&str) + Send + Sync),
) -> Result<Prepared, String> {
    report(&format!("读取版本 {}", ctx.version_id));
    let bootstrap_client = if ctx.proxy.trim().is_empty() {
        reqwest::Client::builder()
            .user_agent("AuroraLauncher/0.3")
            .timeout(std::time::Duration::from_secs(20))
            .build()
            .map_err(|e| e.to_string())?
    } else {
        reqwest::Client::builder()
            .user_agent("AuroraLauncher/0.3")
            .timeout(std::time::Duration::from_secs(20))
            .proxy(reqwest::Proxy::all(&ctx.proxy).map_err(|e| e.to_string())?)
            .build()
            .map_err(|e| e.to_string())?
    };
    let effective_source = resolve_download_source(&bootstrap_client, &ctx.download_source).await;
    if ctx.download_source == "auto" {
        report(&format!("自动下载源：{}", effective_source));
    }
    let pipeline = pipeline_of(&ctx.pipeline);
    if pipeline == Pipeline::Compat {
        report("兼容模式：沿用旧版启动管线（无并行/重试/JVM 优化）");
    }
    let version = load_version(ctx, &ctx.version_id, 0)?;
    let required_java = version
        .java_version
        .as_ref()
        .map(|value| value.major_version)
        .unwrap_or(17);
    let java_preferred = ctx
        .java
        .as_ref()
        .map(|path| path.to_string_lossy().to_string());
    let java_path = find_java(java_preferred.as_deref(), required_java)?;
    report(&format!(
        "使用 Java {}：{}",
        required_java,
        java_path.display()
    ));

    let client = if ctx.proxy.trim().is_empty() {
        reqwest::Client::builder()
            .user_agent("AuroraLauncher/0.3")
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| e.to_string())?
    } else {
        reqwest::Client::builder()
            .user_agent("AuroraLauncher/0.3")
            .timeout(std::time::Duration::from_secs(120))
            .proxy(reqwest::Proxy::all(ctx.proxy.trim()).map_err(|e| e.to_string())?)
            .build()
            .map_err(|e| e.to_string())?
    };

    let mut downloads: Vec<DownloadItem> = Vec::new();
    let client_jar_id = version
        .client_jar_id
        .clone()
        .unwrap_or_else(|| ctx.version_id.clone());
    // client jar 的 SHA-1 用于 natives 缓存标记（见下方 marker 逻辑）。
    let client_jar_sha = version
        .downloads
        .as_ref()
        .and_then(|value| value.client.as_ref())
        .and_then(|artifact| artifact.sha1.clone());
    let client_jar =
        match existing_version_file(ctx, &client_jar_id, &format!("{client_jar_id}.jar")) {
            Some(path) => path,
            None => {
                let artifact = version
                    .downloads
                    .as_ref()
                    .and_then(|value| value.client.clone())
                    .ok_or_else(|| "版本 JSON 没有 client jar 下载信息".to_string())?;
                let target = ctx
                    .game_dir
                    .join("versions")
                    .join(&client_jar_id)
                    .join(format!("{client_jar_id}.jar"));
                downloads.push(DownloadItem {
                    url: artifact.url,
                    path: target.clone(),
                    sha1: artifact.sha1,
                    size: artifact.size.unwrap_or(0),
                });
                target
            }
        };

    let features: HashMap<String, bool> = HashMap::new();
    let mut classpath: Vec<PathBuf> = Vec::new();
    let mut natives_jobs: Vec<(PathBuf, Vec<String>)> = Vec::new();
    for library in version.libraries.clone().unwrap_or_default() {
        if !rules_allow(&library.rules, &features) {
            continue;
        }
        if let Some(artifact) = library_artifact(&library, None) {
            let relative = artifact
                .path
                .clone()
                .or_else(|| maven_path(&library.name))
                .ok_or_else(|| format!("无法解析库路径：{}", library.name))?;
            let existing = resolve_existing(ctx, &relative);
            let path = match existing {
                Some(path) => path,
                None => {
                    let target = ctx.libraries_root.join(&relative);
                    downloads.push(DownloadItem {
                        url: artifact.url,
                        path: target.clone(),
                        sha1: artifact.sha1,
                        size: artifact.size.unwrap_or(0),
                    });
                    target
                }
            };
            classpath.push(path);
        }
        if let Some(classifier) = native_classifier(&library) {
            if let Some(artifact) = library_artifact(&library, Some(&classifier)) {
                let relative = artifact
                    .path
                    .clone()
                    .or_else(|| maven_path(&format!("{}:{classifier}", library.name)))
                    .ok_or_else(|| format!("无法解析 natives 路径：{}", library.name))?;
                let existing = resolve_existing(ctx, &relative);
                let path = match existing {
                    Some(path) => path,
                    None => {
                        let target = ctx.libraries_root.join(&relative);
                        downloads.push(DownloadItem {
                            url: artifact.url,
                            path: target.clone(),
                            sha1: artifact.sha1,
                            size: artifact.size.unwrap_or(0),
                        });
                        target
                    }
                };
                let exclude = library
                    .extract
                    .as_ref()
                    .and_then(|value| value.exclude.clone())
                    .unwrap_or_default();
                natives_jobs.push((path, exclude));
            }
        }
    }

    let asset_index_id = version
        .asset_index
        .as_ref()
        .map(|value| value.id.clone())
        .or_else(|| version.assets.clone())
        .ok_or_else(|| "版本 JSON 没有 assetIndex".to_string())?;
    let assets_root = ctx.assets_root.clone();
    let asset_index_path = assets_root
        .join("indexes")
        .join(format!("{asset_index_id}.json"));
    let mut asset_index_item: Option<DownloadItem> = None;
    if !asset_index_path.exists() {
        let index = version
            .asset_index
            .as_ref()
            .ok_or_else(|| "版本 JSON 缺少 assetIndex 下载信息".to_string())?;
        asset_index_item = Some(DownloadItem {
            url: index.url.clone(),
            path: asset_index_path.clone(),
            sha1: index.sha1.clone(),
            size: index.size.unwrap_or(0),
        });
    }

    if let Some(logging) = &version.logging {
        if let Some(client_logging) = &logging.client {
            if let Some(file) = &client_logging.file {
                let target = assets_root
                    .join("log_configs")
                    .join(format!("{}.xml", file.id));
                if !target.exists() {
                    downloads.push(DownloadItem {
                        url: file.url.clone(),
                        path: target,
                        sha1: file.sha1.clone(),
                        size: file.size.unwrap_or(0),
                    });
                }
            }
        }
    }

    // 下载阶段。native：资源索引单独先取回（绝大多数启动已缓存，
    // miss 时仅 ~1 MB），随后 client jar、libraries 与几千个资源对象
    // 合成一个大批次并行下载，消除旧两阶段间的串行等待；compat：
    // 完全沿用旧的两阶段顺序。
    let parsed_assets = match pipeline {
        Pipeline::Native => {
            if let Some(item) = &asset_index_item {
                download_one(
                    &client,
                    item,
                    &effective_source,
                    &client_jar_id,
                    pipeline,
                )
                .await?;
            }
            let parsed = parse_asset_index_if_exists(&asset_index_path)?;
            let mut batch = downloads;
            if let Some(parsed) = &parsed {
                batch.extend(build_asset_items(parsed, &assets_root));
            }
            download_all(
                &client,
                batch,
                &effective_source,
                &client_jar_id,
                pipeline,
                report,
            )
            .await?;
            parsed
        }
        Pipeline::Compat => {
            let mut batch = downloads;
            if let Some(item) = asset_index_item {
                batch.push(item);
            }
            download_all(
                &client,
                batch,
                &effective_source,
                &client_jar_id,
                pipeline,
                report,
            )
            .await?;
            let parsed = parse_asset_index_if_exists(&asset_index_path)?;
            if let Some(parsed) = &parsed {
                download_all(
                    &client,
                    build_asset_items(parsed, &assets_root),
                    &effective_source,
                    &client_jar_id,
                    pipeline,
                    report,
                )
                .await?;
            }
            parsed
        }
    };
    if let Some(parsed) = &parsed_assets {
        if parsed.virtual_ || parsed.map_to_resources {
            let virtual_dir = assets_root.join(if parsed.map_to_resources {
                "resources"
            } else {
                "virtual/legacy"
            });
            for (name, object) in &parsed.objects {
                let source = assets_root
                    .join("objects")
                    .join(&object.hash[..2])
                    .join(&object.hash);
                let target = virtual_dir.join(name);
                if !target.exists() && source.exists() {
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                    }
                    fs::copy(&source, &target).map_err(|e| e.to_string())?;
                }
            }
        }
    }

    let natives_dir = ctx.game_dir.join("natives").join(&ctx.version_id);
    if !natives_jobs.is_empty() {
        let marker = natives_dir.join(".aurora-natives");
        // marker 记录 client jar 的 SHA-1：同版本号但 jar 变化（修复重装
        // 等）时自动重解压。旧版 marker 内容固定 "ok"，升级后首次启动会
        // 重解压一次并迁移到新格式；client jar 无 sha1 信息时保持 "ok"。
        let jar_stamp = client_jar_sha.clone().unwrap_or_else(|| "ok".into());
        let stale = match fs::read_to_string(&marker) {
            Ok(content) => content.trim() != jar_stamp,
            Err(_) => true,
        };
        if stale {
            let _ = fs::remove_dir_all(&natives_dir);
            fs::create_dir_all(&natives_dir).map_err(|e| e.to_string())?;
            for (archive, exclude) in &natives_jobs {
                extract_natives(archive, &natives_dir, exclude, report)?;
            }
            fs::write(&marker, jar_stamp).map_err(|e| e.to_string())?;
        }
    }

    let assets_dir = assets_root.display().to_string();
    let natives = natives_dir.display().to_string();
    let libraries_dir = ctx.libraries_root.display().to_string();
    let classpath = if json_provides_client(&version) {
        build_classpath(Some(&client_jar), &classpath)
    } else {
        // 继承型版本（Forge/NeoForge/Fabric）：不放原版 jar，避免模块冲突。
        build_classpath(None, &classpath)
    };
    let mut placeholders: HashMap<&str, String> = HashMap::new();
    placeholders.insert("auth_player_name", ctx.username.clone());
    placeholders.insert("version_name", ctx.version_id.clone());
    placeholders.insert("game_directory", ctx.instance_dir.display().to_string());
    placeholders.insert("assets_root", assets_dir.clone());
    placeholders.insert("assets_index_name", asset_index_id.clone());
    placeholders.insert("auth_uuid", ctx.uuid.clone());
    placeholders.insert("auth_access_token", ctx.access_token.clone());
    placeholders.insert("clientid", ctx.client_id.clone());
    placeholders.insert("auth_xuid", ctx.xuid.clone());
    placeholders.insert("user_type", ctx.user_type.clone());
    placeholders.insert(
        "version_type",
        version
            .version_type
            .clone()
            .unwrap_or_else(|| "release".into()),
    );
    placeholders.insert("natives_directory", natives.clone());
    placeholders.insert("launcher_name", "Aurora".into());
    placeholders.insert("launcher_version", env!("CARGO_PKG_VERSION").into());
    placeholders.insert("classpath", classpath.clone());
    placeholders.insert("classpath_separator", ";".into());
    placeholders.insert("library_directory", libraries_dir.clone());
    placeholders.insert("user_properties", "{}".into());
    placeholders.insert("resolution_width", ctx.window_width.to_string());
    placeholders.insert("resolution_height", ctx.window_height.to_string());

    let mut args: Vec<String> = Vec::new();
    if let Some(jvm) = version
        .arguments
        .as_ref()
        .and_then(|value| value.jvm.as_ref())
    {
        for arg in collect_args(jvm, &features) {
            args.push(replace_placeholders(&arg, &placeholders));
        }
    }
    if !args.iter().any(|arg| arg == "-cp" || arg == "-classpath") {
        args.push("-cp".into());
        args.push(classpath.clone());
    }
    args.extend(authlib_args(ctx.authlib.as_ref()));
    args.extend(ctx.ai_args.iter().cloned());
    if !args
        .iter()
        .any(|arg| arg.starts_with("-Djava.library.path="))
    {
        args.push(format!("-Djava.library.path={natives}"));
    }
    if !args
        .iter()
        .any(|arg| arg.starts_with("-Dminecraft.launcher.brand="))
    {
        args.push("-Dminecraft.launcher.brand=Aurora".into());
    }
    if !args
        .iter()
        .any(|arg| arg.starts_with("-Dminecraft.launcher.version="))
    {
        args.push(format!("-Dminecraft.launcher.version={}", env!("CARGO_PKG_VERSION")));
    }
    if let Some(logging) = version
        .logging
        .as_ref()
        .and_then(|value| value.client.as_ref())
    {
        if let (Some(argument), Some(file)) = (&logging.argument, &logging.file) {
            let path = assets_root
                .join("log_configs")
                .join(format!("{}.xml", file.id));
            let arg = argument.replace("${path}", &path.display().to_string());
            if !args.iter().any(|value| value == &arg) {
                args.push(arg);
            }
        }
    }
    // JVM 优化预设（仅 native 管线注入）：收集器组合冲突会让 JVM 直接
    // 拒绝启动，注入前先移除版本 JSON 自带的收集器 flags，再补预设；
    // 额外 JVM 参数随后追加。
    if pipeline == Pipeline::Native && ctx.jvm_optimize {
        if let Some(preset) = gc_preset_args(&ctx.jvm_gc, required_java) {
            const COLLECTORS: [&str; 5] = [
                "-XX:+UseG1GC",
                "-XX:+UseZGC",
                "-XX:+UseSerialGC",
                "-XX:+UseParallelGC",
                "-XX:+UseConcMarkSweepGC",
            ];
            args.retain(|arg| !COLLECTORS.contains(&arg.as_str()));
            for flag in preset {
                if !args.contains(&flag) {
                    args.push(flag);
                }
            }
        }
        for arg in ctx.extra_jvm_args.split_whitespace() {
            args.push(arg.to_string());
        }
    }
    args.push(format!("-Xmx{}M", ctx.memory_mb.max(512)));
    args.push(
        version
            .main_class
            .clone()
            .ok_or_else(|| "版本 JSON 缺少 mainClass".to_string())?,
    );
    if let Some(game_args) = version
        .arguments
        .as_ref()
        .and_then(|value| value.game.as_ref())
    {
        for arg in collect_args(game_args, &features) {
            args.push(replace_placeholders(&arg, &placeholders));
        }
    } else if let Some(legacy) = &version.minecraft_arguments {
        for arg in legacy.split_whitespace() {
            args.push(replace_placeholders(arg, &placeholders));
        }
    }
    if pipeline == Pipeline::Native {
        for arg in ctx.extra_game_args.split_whitespace() {
            args.push(arg.to_string());
        }
        if ctx.fullscreen {
            args.push("--fullscreen".into());
        }
    }

    report("启动参数已生成");
    Ok(Prepared {
        java: java_path,
        args,
        cwd: ctx.instance_dir.clone(),
    })
}

pub fn spawn(
    prepared: &Prepared,
    on_line: Arc<dyn Fn(String) + Send + Sync>,
    on_exit: Arc<dyn Fn(Option<i32>) + Send + Sync>,
) -> Result<u32, String> {
    let mut command = Command::new(&prepared.java);
    command
        .args(&prepared.args)
        .current_dir(&prepared.cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    hide_console(&mut command);
    let mut child = command.spawn().map_err(|e| format!("无法启动 Java：{e}"))?;
    let pid = child.id();
    let output_lines = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    if let Some(stdout) = child.stdout.take() {
        let on_line = on_line.clone();
        let output_lines = Arc::clone(&output_lines);
        std::thread::spawn(move || {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                output_lines.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                on_line(line);
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        let on_line = on_line.clone();
        let output_lines = Arc::clone(&output_lines);
        std::thread::spawn(move || {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                output_lines.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                on_line(line);
            }
        });
    }
    std::thread::spawn(move || {
        let status = child.wait();
        let code = status.as_ref().ok().and_then(|value| value.code());
        let _ = on_line(format!(
            "[Aurora] 游戏进程已退出：{}",
            status
                .map(|value| value.to_string())
                .unwrap_or_else(|error| error.to_string())
        ));
        // 非零退出但全程没有游戏输出时，给出排查方向而不是只留一个退出码。
        let failed = code.map(|value| value != 0).unwrap_or(true);
        if failed && output_lines.load(std::sync::atomic::Ordering::Relaxed) == 0 {
            let _ = on_line(
                "[Aurora] 进程退出前没有产生任何输出。常见原因：JVM 参数格式错误（例如 -D 缺少 = 值）、                 Java 与实例不兼容、或崩溃发生在日志系统初始化之前；可在实例管理中检查 Java 与内存设置。"
                    .to_string(),
            );
        }
        on_exit(code);
    });
    Ok(pid)
}

pub async fn smoke(ctx: &LaunchContext, seconds: u64) -> Result<String, String> {
    let lines: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let report_lines = lines.clone();
    let prepared = prepare(ctx, &move |message| {
        report_lines.lock().unwrap().push(message.to_string());
    })
    .await?;
    let output = lines.clone();
    let pid = spawn(
        &prepared,
        Arc::new(move |line| {
            let mut output = output.lock().unwrap();
            if output.len() < 400 {
                output.push(line);
            }
        }),
        Arc::new(|_| {}),
    )?;
    std::thread::sleep(std::time::Duration::from_secs(seconds));
    let mut command = Command::new("taskkill");
    command.args(["/PID", &pid.to_string(), "/T", "/F"]);
    hide_console(&mut command);
    let _ = command.output();
    let lines = lines.lock().unwrap().clone();
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gc_presets_follow_settings_and_java() {
        // g1：完整预设，实验选项解锁开关必须在前。
        let g1 = gc_preset_args("g1", 21).unwrap();
        assert!(g1.contains(&"-XX:+UseG1GC".to_string()));
        assert!(g1.contains(&"-XX:MaxGCPauseMillis=50".to_string()));
        assert!(g1.contains(&"-Dfml.ignoreInvalidMinecraftCertificates=true".to_string()));
        let unlock = g1
            .iter()
            .position(|arg| arg == "-XX:+UnlockExperimentalVMOptions")
            .unwrap();
        let experimental = g1
            .iter()
            .position(|arg| arg.starts_with("-XX:G1NewSizePercent"))
            .unwrap();
        assert!(unlock < experimental);
        // off：不注入。
        assert!(gc_preset_args("off", 21).is_none());
        // zgc：21+ 带 ZGenerational；老 JVM（Java 8）回退 G1。
        let zgc21 = gc_preset_args("zgc", 21).unwrap();
        assert!(zgc21.contains(&"-XX:+UseZGC".to_string()));
        assert!(zgc21.contains(&"-XX:+ZGenerational".to_string()));
        let zgc17 = gc_preset_args("zgc", 17).unwrap();
        assert!(zgc17.contains(&"-XX:+UseZGC".to_string()));
        assert!(!zgc17.contains(&"-XX:+ZGenerational".to_string()));
        assert_eq!(
            gc_preset_args("zgc", 8).unwrap(),
            gc_preset_args("g1", 8).unwrap()
        );
    }

    #[test]
    fn pipeline_mode_parsing() {
        assert_eq!(pipeline_of("native"), Pipeline::Native);
        assert_eq!(pipeline_of("compat"), Pipeline::Compat);
        // 未知值一律回落 native（设置项已做白名单归一，这里只是防御）。
        assert_eq!(pipeline_of(""), Pipeline::Native);
        assert_eq!(pipeline_of("whatever"), Pipeline::Native);
    }

    #[test]
    fn maven_path_matches_repository_layout() {
        assert_eq!(
            maven_path("net.fabricmc:fabric-loader:0.16.0"),
            Some("net/fabricmc/fabric-loader/0.16.0/fabric-loader-0.16.0.jar".into())
        );
        assert_eq!(
            maven_path("org.lwjgl:lwjgl:3.3.1:natives-windows"),
            Some("org/lwjgl/lwjgl/3.3.1/lwjgl-3.3.1-natives-windows.jar".into())
        );
        assert_eq!(maven_path("broken"), None);
    }

    #[test]
    fn mirror_maps_mojang_hosts() {
        assert_eq!(
            mirror_url(
                "https://resources.download.minecraft.net/ab/abcdef",
                "1.20.1"
            ),
            Some("https://bmclapi2.bangbang93.com/assets/ab/abcdef".into())
        );
        assert_eq!(
            mirror_url("https://libraries.minecraft.net/a/b/c.jar", "1.20.1"),
            Some("https://bmclapi2.bangbang93.com/maven/a/b/c.jar".into())
        );
        assert!(mirror_url("https://example.com/a", "1.20.1").is_none());
    }

    #[test]
    fn rules_respect_windows_and_features() {
        let features = HashMap::new();
        let windows = vec![Rule {
            action: "allow".into(),
            os: Some(OsRule {
                name: Some("windows".into()),
                arch: None,
            }),
            features: None,
        }];
        assert!(rules_allow(&Some(windows), &features));
        let linux = vec![Rule {
            action: "allow".into(),
            os: Some(OsRule {
                name: Some("linux".into()),
                arch: None,
            }),
            features: None,
        }];
        assert!(!rules_allow(&Some(linux), &features));
        let disallow = vec![Rule {
            action: "disallow".into(),
            os: Some(OsRule {
                name: Some("windows".into()),
                arch: None,
            }),
            features: None,
        }];
        assert!(!rules_allow(&Some(disallow), &features));
    }

    #[test]
    fn placeholders_are_replaced() {
        let mut map = HashMap::new();
        map.insert("auth_player_name", "Steve".to_string());
        map.insert("classpath", "a.jar;b.jar".to_string());
        assert_eq!(
            replace_placeholders("--username ${auth_player_name} -cp ${classpath}", &map),
            "--username Steve -cp a.jar;b.jar"
        );
    }

    #[test]
    fn json_provides_client_detection() {
        let mk = |name: &str| Library {
            name: name.into(),
            downloads: None,
            natives: None,
            rules: None,
            extract: None,
            url: None,
        };
        let vanilla = |libs: Vec<Library>| VersionJson {
            inherits_from: None,
            libraries: Some(libs),
            ..Default::default()
        };
        let inherited = |libs: Vec<Library>| VersionJson {
            inherits_from: Some("1.20.1".into()),
            libraries: Some(libs),
            ..Default::default()
        };
        // 原版 JSON：需要放原版 jar
        assert!(json_provides_client(&vanilla(vec![mk("com.google.code.gson:gson:2.10")])));
        assert!(json_provides_client(&vanilla(vec![])));
        // Forge/NeoForge/Fabric（inheritsFrom）：BootstrapLauncher 自己装配
        // 客户端，原版 jar 绝不能上 classpath（_1._20._1 模块冲突）。
        assert!(!json_provides_client(&inherited(vec![
            mk("net.minecraftforge:forge:1.20.1-47.4.28:universal"),
        ])));
        // Fabric 继承型即使列出 client library，也一视同仁不放原版 jar：
        // 防止 _1._20._1 自动模块再次出现。
        assert!(!json_provides_client(&inherited(vec![
            mk("net.fabricmc:fabric-loader:0.16.0"),
            mk("net.minecraft:client:1.20.1"),
        ])));
    }

    #[test]
    fn merged_inherited_version_keeps_inherits_from() {
        let mk = |name: &str| Library {
            name: name.into(),
            downloads: None,
            natives: None,
            rules: None,
            extract: None,
            url: None,
        };
        let vanilla = VersionJson {
            id: "1.20.1".into(),
            inherits_from: None,
            libraries: Some(vec![mk("com.google.code.gson:gson:2.10")]),
            ..Default::default()
        };
        let child = VersionJson {
            id: "1.20.1-forge-47.4.28".into(),
            inherits_from: Some("1.20.1".into()),
            libraries: Some(vec![mk("net.minecraftforge:forge:1.20.1-47.4.28:universal")]),
            ..Default::default()
        };
        // 真实启动路径是 load_version -> merge_child -> json_provides_client；
        // 合并后必须仍被识别为继承型，否则原版 jar 会被放上 classpath，
        // 重新触发 _1._20._1 模块冲突。
        let merged = merge_child(vanilla, child);
        assert_eq!(merged.inherits_from.as_deref(), Some("1.20.1"));
        assert!(!json_provides_client(&merged));
    }

    #[test]
    fn build_classpath_omits_absent_client_jar() {
        let libs = vec![PathBuf::from("C:/libs/a.jar")];
        assert_eq!(
            build_classpath(Some(Path::new("C:/versions/1.20.1/1.20.1.jar")), &libs),
            "C:/versions/1.20.1/1.20.1.jar;C:/libs/a.jar"
        );
        assert_eq!(build_classpath(None, &libs), "C:/libs/a.jar");
    }

    #[test]
    fn mirror_url_at_maps_bmclapi_compatible_layout() {
        let base = "https://mc.elingfeng.com";
        assert_eq!(
            mirror_url_at(base, "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json", "1.20.1").as_deref(),
            Some("https://mc.elingfeng.com/mc/game/version_manifest_v2.json")
        );
        assert_eq!(
            mirror_url_at(base, "https://piston-data.mojang.com/v1/objects/abc/client.jar", "1.20.1").as_deref(),
            Some("https://mc.elingfeng.com/v1/objects/abc/client.jar")
        );
        assert_eq!(
            mirror_url_at(base, "https://libraries.minecraft.net/com/google/code/gson/gson/2.10/gson-2.10.jar", "1.20.1").as_deref(),
            Some("https://mc.elingfeng.com/maven/com/google/code/gson/gson/2.10/gson-2.10.jar")
        );
        assert_eq!(
            mirror_url_at(base, "https://maven.minecraftforge.net/maven/net/minecraftforge/forge/1.20.1-47.4.28/forge-1.20.1-47.4.28-installer.jar", "1.20.1").as_deref(),
            Some("https://mc.elingfeng.com/maven/net/minecraftforge/forge/1.20.1-47.4.28/forge-1.20.1-47.4.28-installer.jar")
        );
        // NeoForge 去掉 /releases 前缀
        assert_eq!(
            mirror_url_at(base, "https://maven.neoforged.net/releases/net/neoforged/neoforge/20.4.237/neoforge-20.4.237-installer.jar", "1.20.4").as_deref(),
            Some("https://mc.elingfeng.com/maven/net/neoforged/neoforge/20.4.237/neoforge-20.4.237-installer.jar")
        );
        // assets 双段映射
        assert_eq!(
            mirror_url_at(base, "https://resources.download.minecraft.net/0f/0fabc", "1.20.1").as_deref(),
            Some("https://mc.elingfeng.com/assets/0f/0fabc")
        );
        // 无映射的 host 返回 None
        assert_eq!(mirror_url_at(base, "https://example.com/x", "1.20.1"), None);
    }

    #[test]
    fn candidates_custom_mirror_first_official_fallback() {
        let urls = candidates(
            "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json",
            "custom:https://mc.elingfeng.com",
            "1.20.1",
        );
        assert_eq!(urls.len(), 2);
        assert!(urls[0].starts_with("https://mc.elingfeng.com/"));
        assert!(urls[1].starts_with("https://piston-meta.mojang.com/"));
        // 不可映射的 host 只剩官方
        let urls = candidates("https://example.com/x", "custom:https://mc.elingfeng.com", "1.20.1");
        assert_eq!(urls, vec!["https://example.com/x".to_string()]);
    }

    #[test]
    fn download_source_pick_prefers_faster_alive_source() {
        use std::time::Duration;
        assert_eq!(pick_download_source(Some(Duration::from_millis(50)), Some(Duration::from_millis(20))), "bmclapi");
        assert_eq!(pick_download_source(Some(Duration::from_millis(20)), Some(Duration::from_millis(50))), "official");
        assert_eq!(pick_download_source(Some(Duration::from_millis(20)), None), "official");
        assert_eq!(pick_download_source(None, Some(Duration::from_millis(20))), "bmclapi");
        assert_eq!(pick_download_source(None, None), "official");
    }

    #[test]
    fn forge_and_neoforge_maven_are_mirrored() {
        let url = "https://maven.minecraftforge.net/net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-installer.jar";
        assert_eq!(
            mirror_url(url, "1.20.1").as_deref(),
            Some("https://bmclapi2.bangbang93.com/maven/net/minecraftforge/forge/1.20.1-47.3.0/forge-1.20.1-47.3.0-installer.jar")
        );
        let url = "https://maven.neoforged.net/releases/net/neoforged/neoforge/20.4.237/neoforge-20.4.237-installer.jar";
        assert_eq!(
            mirror_url(url, "20.4.237").as_deref(),
            Some("https://bmclapi2.bangbang93.com/maven/net/neoforged/neoforge/20.4.237/neoforge-20.4.237-installer.jar")
        );
        assert_eq!(mirror_url("https://maven.fabricmc.net/net/fabricmc/fabric-installer/1.0.3/fabric-installer-1.0.3.jar", "x"), None);
    }

    #[test]
    fn authlib_args_inject_agent_and_metadata() {
        let info = crate::yggdrasil::LaunchAuthlib {
            jar: PathBuf::from("C:\\aurora\\authlib-injector-1.2.8.jar"),
            server: "https://littleskin.cn/api/yggdrasil/".into(),
            prefetched: "eyJtZXRhIjp7fX0=".into(),
        };
        let args = authlib_args(Some(&info));
        assert_eq!(args.len(), 2);
        assert!(args[0].starts_with("-javaagent:"));
        assert!(args[0].ends_with("authlib-injector-1.2.8.jar=https://littleskin.cn/api/yggdrasil/"));
        assert!(args[1].starts_with("-Dauthlibinjector.yggdrasil.prefetched="));
        assert!(authlib_args(None).is_empty());
    }
}
