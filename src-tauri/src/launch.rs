use crate::{safe_filename, write_atomic};
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
        Arc,
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
    pub download_source: String,
    pub proxy: String,
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

fn mirror_url(original: &str, version_id: &str) -> Option<String> {
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
        _ => None,
    }
}

fn candidates(url: &str, source: &str, version_id: &str) -> Vec<String> {
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

async fn download_one(
    client: &reqwest::Client,
    item: &DownloadItem,
    source: &str,
    version_id: &str,
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
    let mut last_error = String::new();
    for url in candidates(&item.url, source, version_id) {
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
    report: &(dyn Fn(&str) + Send + Sync),
) -> Result<(), String> {
    let total = items.len();
    if total == 0 {
        return Ok(());
    }
    let concurrency = if source == "bmclapi" { 2 } else { 8 };
    let done = Arc::new(AtomicUsize::new(0));
    report(&format!("需要检查/下载 {total} 个文件"));
    let results = stream::iter(items.into_iter().map(|item| {
        let client = client.clone();
        let done = done.clone();
        let source = source.to_string();
        let version_id = version_id.to_string();
        async move {
            let result = download_one(&client, &item, &source, &version_id).await;
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
    let output = Command::new(path).arg("-version").output().ok()?;
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

pub(crate) fn find_java(preferred: Option<&str>, required: u32) -> Result<PathBuf, String> {
    let candidates = collect_java_candidates(preferred);
    let mut detected: Vec<(u32, PathBuf)> = Vec::new();
    for candidate in candidates {
        if let Some(major) = java_major(&candidate) {
            detected.push((major, candidate));
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

fn build_classpath(client_jar: &Path, libraries: &[PathBuf]) -> String {
    let mut entries = Vec::new();
    entries.push(client_jar.display().to_string());
    for library in libraries {
        entries.push(library.display().to_string());
    }
    entries.join(";")
}

pub async fn prepare(
    ctx: &LaunchContext,
    report: &(dyn Fn(&str) + Send + Sync),
) -> Result<Prepared, String> {
    report(&format!("读取版本 {}", ctx.version_id));
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
    if !asset_index_path.exists() {
        let index = version
            .asset_index
            .as_ref()
            .ok_or_else(|| "版本 JSON 缺少 assetIndex 下载信息".to_string())?;
        downloads.push(DownloadItem {
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

    download_all(
        &client,
        downloads,
        &ctx.download_source,
        &client_jar_id,
        report,
    )
    .await?;

    if asset_index_path.exists() {
        let text = fs::read_to_string(&asset_index_path).map_err(|e| e.to_string())?;
        let parsed: AssetObjects =
            serde_json::from_str(&text).map_err(|e| format!("资源索引解析失败：{e}"))?;
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
        download_all(&client, items, &ctx.download_source, &client_jar_id, report).await?;
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
        if !marker.exists() {
            let _ = fs::remove_dir_all(&natives_dir);
            fs::create_dir_all(&natives_dir).map_err(|e| e.to_string())?;
            for (archive, exclude) in &natives_jobs {
                extract_natives(archive, &natives_dir, exclude, report)?;
            }
            fs::write(&marker, b"ok").map_err(|e| e.to_string())?;
        }
    }

    let assets_dir = assets_root.display().to_string();
    let natives = natives_dir.display().to_string();
    let libraries_dir = ctx.libraries_root.display().to_string();
    let classpath = build_classpath(&client_jar, &classpath);
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
    placeholders.insert("launcher_version", "0.4.0".into());
    placeholders.insert("classpath", classpath.clone());
    placeholders.insert("classpath_separator", ";".into());
    placeholders.insert("library_directory", libraries_dir.clone());
    placeholders.insert("user_properties", "{}".into());
    placeholders.insert("resolution_width", "854".into());
    placeholders.insert("resolution_height", "480".into());

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
        args.push("-Dminecraft.launcher.version=0.4.0".into());
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

    report("启动参数已生成");
    Ok(Prepared {
        java: java_path,
        args,
        cwd: ctx.instance_dir.clone(),
    })
}

#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x08000000);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

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
    if let Some(stdout) = child.stdout.take() {
        let on_line = on_line.clone();
        std::thread::spawn(move || {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                on_line(line);
            }
        });
    }
    if let Some(stderr) = child.stderr.take() {
        let on_line = on_line.clone();
        std::thread::spawn(move || {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
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
    let _ = Command::new("taskkill")
        .args(["/PID", &pid.to_string(), "/T", "/F"])
        .output();
    let lines = lines.lock().unwrap().clone();
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
