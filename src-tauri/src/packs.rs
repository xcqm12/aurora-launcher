use crate::{
    curseforge_key, http_client, read_instances, safe_filename, write_atomic, write_instance_meta,
    write_instances,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha1::{Digest, Sha1};
use sha2::Sha512;
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

#[derive(Debug, Serialize)]
pub struct ImportResult {
    pub kind: String,
    pub name: String,
    pub files: usize,
    pub overrides: usize,
    pub loader: Option<String>,
    pub loader_version: Option<String>,
    pub minecraft: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MrpackIndex {
    name: Option<String>,
    #[serde(default)]
    files: Vec<MrpackFile>,
    #[serde(default)]
    dependencies: std::collections::HashMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct MrpackFile {
    path: String,
    #[serde(default)]
    hashes: std::collections::HashMap<String, String>,
    #[serde(default)]
    downloads: Vec<String>,
    #[serde(rename = "fileSize", default)]
    file_size: u64,
    #[serde(default)]
    env: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct CfManifest {
    name: Option<String>,
    minecraft: Option<Value>,
    #[serde(rename = "modLoaders", default)]
    mod_loaders: Vec<Value>,
    #[serde(default)]
    files: Vec<Value>,
    #[serde(default)]
    overrides: Option<String>,
}

fn safe_relative(value: &str) -> Result<PathBuf, String> {
    let normalized = value.replace('\\', "/");
    if normalized.starts_with('/') || normalized.contains(':') || normalized.contains("..") {
        return Err(format!("压缩包内路径不安全：{value}"));
    }
    let mut path = PathBuf::new();
    for part in normalized.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        let Some(name) = safe_filename(part) else {
            return Err(format!("压缩包内文件名无效：{part}"));
        };
        path.push(name);
    }
    if path.as_os_str().is_empty() {
        return Err("压缩包内路径为空".into());
    }
    Ok(path)
}

fn allowed_pack_host(url: &reqwest::Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    let host = url.host_str().unwrap_or_default();
    host == "cdn.modrinth.com"
        || host.ends_with(".modrinth.com")
        || host == "github.com"
        || host == "objects.githubusercontent.com"
        || host == "raw.githubusercontent.com"
        || host.ends_with(".forgecdn.net")
        || host == "mediafilez.forgecdn.net"
}

async fn download_pack_file(
    client: &reqwest::Client,
    urls: &[String],
    target: &Path,
    sha1: Option<&str>,
    sha512: Option<&str>,
    size: u64,
) -> Result<(), String> {
    if target.exists() {
        if size > 0 {
            if let Ok(meta) = fs::metadata(target) {
                if meta.len() == size {
                    return Ok(());
                }
            }
        }
    }
    let mut last_error = String::new();
    for raw in urls {
        let Ok(url) = reqwest::Url::parse(raw) else {
            last_error = "下载地址无效".into();
            continue;
        };
        if !allowed_pack_host(&url) {
            last_error = format!("来源不在白名单：{raw}");
            continue;
        }
        let response = match client.get(url).send().await {
            Ok(response) => response,
            Err(error) => {
                last_error = error.to_string();
                continue;
            }
        };
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
        if size > 0 && bytes.len() as u64 != size {
            last_error = "文件大小校验失败".into();
            continue;
        }
        if let Some(expected) = sha512 {
            if hex::encode(Sha512::digest(&bytes)) != expected.to_lowercase() {
                last_error = "SHA-512 校验失败".into();
                continue;
            }
        } else if let Some(expected) = sha1 {
            if hex::encode(Sha1::digest(&bytes)) != expected.to_lowercase() {
                last_error = "SHA-1 校验失败".into();
                continue;
            }
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        write_atomic(target, &bytes)?;
        return Ok(());
    }
    Err(format!(
        "整合包文件下载失败：{}（{}）",
        target.display(),
        last_error
    ))
}

fn copy_overrides(
    archive: &mut ZipArchive<File>,
    prefix: &str,
    instance_dir: &Path,
) -> Result<usize, String> {
    let mut count = 0;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
        let name = entry.name().to_string();
        if !name.starts_with(prefix) || name.ends_with('/') {
            continue;
        }
        let relative = &name[prefix.len()..];
        let target = instance_dir.join(safe_relative(relative)?);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut output = File::create(&target).map_err(|e| e.to_string())?;
        std::io::copy(&mut entry, &mut output).map_err(|e| e.to_string())?;
        count += 1;
    }
    Ok(count)
}

fn pack_loader(
    dependencies: &std::collections::HashMap<String, String>,
) -> Option<(String, String)> {
    for (key, loader) in [
        ("fabric-loader", "fabric"),
        ("forge", "forge"),
        ("neoforge", "neoforge"),
        ("quilt-loader", "quilt"),
    ] {
        if let Some(version) = dependencies.get(key) {
            return Some((loader.into(), version.clone()));
        }
    }
    None
}

async fn import_mrpack(
    app: &tauri::AppHandle,
    instance: &crate::Instance,
    path: &Path,
) -> Result<ImportResult, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|e| format!("不是有效 zip：{e}"))?;
    let mut index_text = String::new();
    archive
        .by_name("modrinth.index.json")
        .map_err(|_| "压缩包缺少 modrinth.index.json，不是 .mrpack".to_string())?
        .read_to_string(&mut index_text)
        .map_err(|e| e.to_string())?;
    let index: MrpackIndex = serde_json::from_str(&index_text).map_err(|e| e.to_string())?;
    let minecraft = index.dependencies.get("minecraft").cloned();
    if let Some(required) = &minecraft {
        if required != &instance.minecraft_version {
            return Err(format!(
                "该整合包面向 Minecraft {required}，当前实例是 {}；请新建对应版本实例",
                instance.minecraft_version
            ));
        }
    }
    let loader = pack_loader(&index.dependencies);
    let root = PathBuf::from(&instance.directory);
    let client = http_client(app)?;
    let mut files = 0;
    for entry in &index.files {
        if let Some(env) = &entry.env {
            if env.get("client").and_then(|value| value.as_str()) == Some("unsupported") {
                continue;
            }
        }
        let relative = safe_relative(&entry.path)?;
        let target = root.join(relative);
        let sha1 = entry.hashes.get("sha1").map(String::as_str);
        let sha512 = entry.hashes.get("sha512").map(String::as_str);
        download_pack_file(
            &client,
            &entry.downloads,
            &target,
            sha1,
            sha512,
            entry.file_size,
        )
        .await?;
        files += 1;
    }
    let overrides = copy_overrides(&mut archive, "overrides/", &root)?
        + copy_overrides(&mut archive, "client-overrides/", &root)?;
    Ok(ImportResult {
        kind: "mrpack".into(),
        name: index.name.unwrap_or_else(|| "未命名整合包".into()),
        files,
        overrides,
        loader: loader.as_ref().map(|value| value.0.clone()),
        loader_version: loader.map(|value| value.1),
        minecraft,
    })
}

async fn cf_file_info(
    app: &tauri::AppHandle,
    project_id: i64,
    file_id: i64,
) -> Result<(String, String, Option<String>, u64), String> {
    let key = curseforge_key(app)?;
    let client = http_client(app)?;
    let response = client
        .get(format!(
            "https://api.curseforge.com/v1/mods/{project_id}/files/{file_id}"
        ))
        .header("x-api-key", key)
        .header("User-Agent", "AuroraLauncher/0.4")
        .send()
        .await
        .map_err(|e| format!("CurseForge 请求失败：{e}"))?;
    if !response.status().is_success() {
        return Err(format!(
            "CurseForge 返回 HTTP {}",
            response.status().as_u16()
        ));
    }
    let value: Value = response.json().await.map_err(|e| e.to_string())?;
    let data = value
        .get("data")
        .ok_or_else(|| "CurseForge 响应缺少 data".to_string())?;
    let url = data
        .get("downloadUrl")
        .and_then(|item| item.as_str())
        .ok_or_else(|| "该文件没有可用直链".to_string())?
        .to_string();
    let file_name = data
        .get("fileName")
        .and_then(|item| item.as_str())
        .unwrap_or("mod.jar")
        .to_string();
    let sha1 = data
        .get("hashes")
        .and_then(|item| item.as_array())
        .into_iter()
        .flatten()
        .find(|hash| hash.get("algo").and_then(|value| value.as_u64()) == Some(1))
        .and_then(|hash| hash.get("value"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let size = data
        .get("fileLength")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    Ok((url, file_name, sha1, size))
}

async fn import_curseforge(
    app: &tauri::AppHandle,
    instance: &crate::Instance,
    path: &Path,
) -> Result<ImportResult, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|e| format!("不是有效 zip：{e}"))?;
    let mut manifest_text = String::new();
    archive
        .by_name("manifest.json")
        .map_err(|_| "压缩包缺少 manifest.json，不是 CurseForge 整合包".to_string())?
        .read_to_string(&mut manifest_text)
        .map_err(|e| e.to_string())?;
    let manifest: CfManifest = serde_json::from_str(&manifest_text).map_err(|e| e.to_string())?;
    let minecraft = manifest
        .minecraft
        .as_ref()
        .and_then(|value| value.get("version"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    if let Some(required) = &minecraft {
        if required != &instance.minecraft_version {
            return Err(format!(
                "该整合包面向 Minecraft {required}，当前实例是 {}",
                instance.minecraft_version
            ));
        }
    }
    let loader = manifest
        .mod_loaders
        .iter()
        .find_map(|value| value.get("id").and_then(|item| item.as_str()))
        .and_then(|id| {
            let lower = id.to_lowercase();
            let (loader, version) = if let Some(rest) = lower.strip_prefix("forge-") {
                ("forge", rest)
            } else if let Some(rest) = lower.strip_prefix("neoforge-") {
                ("neoforge", rest)
            } else if let Some(rest) = lower.strip_prefix("fabric-") {
                ("fabric", rest)
            } else if let Some(rest) = lower.strip_prefix("quilt-") {
                ("quilt", rest)
            } else {
                return None;
            };
            Some((loader.to_string(), version.to_string()))
        });
    let client = http_client(app)?;
    let root = PathBuf::from(&instance.directory);
    let mut files = 0;
    for entry in &manifest.files {
        let project_id = entry
            .get("projectID")
            .and_then(|value| value.as_i64())
            .unwrap_or(0);
        let file_id = entry
            .get("fileID")
            .and_then(|value| value.as_i64())
            .unwrap_or(0);
        if project_id == 0 || file_id == 0 {
            continue;
        }
        let (url, file_name, sha1, size) = cf_file_info(app, project_id, file_id).await?;
        let target = root
            .join("mods")
            .join(safe_filename(&file_name).ok_or_else(|| "文件名无效".to_string())?);
        download_pack_file(&client, &[url], &target, sha1.as_deref(), None, size).await?;
        files += 1;
    }
    let overrides_prefix = manifest
        .overrides
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("overrides");
    let overrides = copy_overrides(
        &mut archive,
        &format!("{}/", overrides_prefix.trim_end_matches('/')),
        &root,
    )?;
    Ok(ImportResult {
        kind: "curseforge".into(),
        name: manifest.name.unwrap_or_else(|| "未命名整合包".into()),
        files,
        overrides,
        loader: loader.as_ref().map(|value| value.0.clone()),
        loader_version: loader.map(|value| value.1),
        minecraft,
    })
}

pub async fn import_pack(
    app: tauri::AppHandle,
    instance_id: String,
    path: String,
) -> Result<ImportResult, String> {
    let instance = read_instances(&app)?
        .into_iter()
        .find(|item| item.id == instance_id)
        .ok_or_else(|| "实例不存在".to_string())?;
    let path = PathBuf::from(path);
    if !path.exists() {
        return Err("文件不存在".into());
    }
    let file = File::open(&path).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|e| format!("不是有效 zip：{e}"))?;
    let is_mrpack = archive.by_name("modrinth.index.json").is_ok();
    drop(archive);
    let result = if is_mrpack {
        import_mrpack(&app, &instance, &path).await?
    } else {
        import_curseforge(&app, &instance, &path).await?
    };
    if let Some(loader) = &result.loader {
        let mut all = read_instances(&app)?;
        if let Some(item) = all.iter_mut().find(|item| item.id == instance_id) {
            item.loader = loader.clone();
            item.loader_version = result.loader_version.clone();
            item.launch_version_id = None;
            let updated = item.clone();
            write_instance_meta(&updated)?;
        }
        write_instances(&app, &all)?;
    }
    Ok(result)
}

pub fn import_world(
    app: tauri::AppHandle,
    instance_id: String,
    path: String,
) -> Result<String, String> {
    let instance = read_instances(&app)?
        .into_iter()
        .find(|item| item.id == instance_id)
        .ok_or_else(|| "实例不存在".to_string())?;
    let file = File::open(&path).map_err(|e| e.to_string())?;
    let mut archive = ZipArchive::new(file).map_err(|e| format!("不是有效 zip：{e}"))?;
    let mut has_root_level = false;
    let mut top_folder: Option<String> = None;
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(|e| e.to_string())?;
        let name = entry.name();
        if name == "level.dat" {
            has_root_level = true;
            break;
        }
        if !has_root_level && top_folder.is_none() {
            if let Some((first, rest)) = name.split_once('/') {
                if rest == "level.dat" {
                    top_folder = Some(first.to_string());
                }
            }
        }
    }
    let name = Path::new(&path)
        .file_stem()
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| "world".into());
    let saves = PathBuf::from(&instance.directory).join("saves");
    let target = saves.join(safe_filename(&name).unwrap_or_else(|| "world".into()));
    if has_root_level {
        fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
            if entry.name().ends_with('/') {
                continue;
            }
            let relative = safe_relative(entry.name())?;
            let output = target.join(relative);
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut file = File::create(&output).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut file).map_err(|e| e.to_string())?;
        }
    } else if let Some(folder) = top_folder {
        let prefix = format!("{folder}/");
        let target = saves.join(safe_filename(&folder).unwrap_or_else(|| "world".into()));
        fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
            let name = entry.name().to_string();
            if !name.starts_with(&prefix) || name.ends_with('/') {
                continue;
            }
            let relative = safe_relative(&name[prefix.len()..])?;
            let output = target.join(relative);
            if let Some(parent) = output.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut file = File::create(&output).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut file).map_err(|e| e.to_string())?;
        }
    } else {
        return Err("压缩包中找不到 level.dat".into());
    }
    Ok(target.display().to_string())
}

fn add_dir_to_zip(zip: &mut ZipWriter<File>, source: &Path, prefix: &str) -> Result<usize, String> {
    if !source.exists() {
        return Ok(0);
    }
    let mut count = 0;
    fn walk(
        zip: &mut ZipWriter<File>,
        dir: &Path,
        prefix: &str,
        count: &mut usize,
    ) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())?.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            let next_prefix = format!("{prefix}{name}");
            if path.is_dir() {
                walk(zip, &path, &format!("{next_prefix}/"), count)?;
            } else if path.is_file() {
                let options = SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated);
                zip.start_file(next_prefix, options)
                    .map_err(|e| e.to_string())?;
                let mut file = File::open(&path).map_err(|e| e.to_string())?;
                let mut buffer = [0u8; 64 * 1024];
                loop {
                    let read = file.read(&mut buffer).map_err(|e| e.to_string())?;
                    if read == 0 {
                        break;
                    }
                    zip.write_all(&buffer[..read]).map_err(|e| e.to_string())?;
                }
                *count += 1;
            }
        }
        Ok(())
    }
    walk(zip, source, prefix, &mut count)?;
    Ok(count)
}

pub fn export_pack(
    app: tauri::AppHandle,
    instance_id: String,
    path: String,
) -> Result<String, String> {
    let instance = read_instances(&app)?
        .into_iter()
        .find(|item| item.id == instance_id)
        .ok_or_else(|| "实例不存在".to_string())?;
    let target = PathBuf::from(path);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let file = File::create(&target).map_err(|e| e.to_string())?;
    let mut zip = ZipWriter::new(file);
    let loader_key = match instance.loader.as_str() {
        "fabric" | "legacyfabric" => "fabric-loader",
        "forge" => "forge",
        "neoforge" => "neoforge",
        "quilt" => "quilt-loader",
        _ => "minecraft",
    };
    let mut dependencies = serde_json::Map::new();
    dependencies.insert("minecraft".into(), json!(instance.minecraft_version));
    if let Some(version) = &instance.loader_version {
        if loader_key != "minecraft" {
            dependencies.insert(loader_key.into(), json!(version));
        }
    }
    let index = json!({
        "formatVersion": 1,
        "game": "minecraft",
        "versionId": "1.0.0",
        "name": instance.name,
        "summary": "Exported by Aurora Launcher",
        "files": [],
        "dependencies": dependencies
    });
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    zip.start_file("modrinth.index.json", options)
        .map_err(|e| e.to_string())?;
    zip.write_all(
        serde_json::to_string_pretty(&index)
            .map_err(|e| e.to_string())?
            .as_bytes(),
    )
    .map_err(|e| e.to_string())?;
    let root = PathBuf::from(&instance.directory);
    let mut count = 0;
    for (source, prefix) in [
        (root.join("mods"), "overrides/mods/"),
        (root.join("config"), "overrides/config/"),
        (root.join("resourcepacks"), "overrides/resourcepacks/"),
        (root.join("shaderpacks"), "overrides/shaderpacks/"),
        (root.join("defaultconfigs"), "overrides/defaultconfigs/"),
    ] {
        count += add_dir_to_zip(&mut zip, &source, prefix)?;
    }
    let options_file = root.join("options.txt");
    if options_file.exists() {
        let options = SimpleFileOptions::default();
        zip.start_file("overrides/options.txt", options)
            .map_err(|e| e.to_string())?;
        let mut file = File::open(&options_file).map_err(|e| e.to_string())?;
        std::io::copy(&mut file, &mut zip).map_err(|e| e.to_string())?;
    }
    zip.finish().map_err(|e| e.to_string())?;
    Ok(format!(
        "{}（{} 个文件，本地 mod 放在 overrides/mods）",
        target.display(),
        count
    ))
}

pub fn pick_pack() -> Option<String> {
    rfd::FileDialog::new()
        .add_filter("整合包", &["mrpack", "zip"])
        .pick_file()
        .map(|path| path.display().to_string())
}

pub fn pick_world() -> Option<String> {
    rfd::FileDialog::new()
        .add_filter("世界压缩包", &["zip"])
        .pick_file()
        .map(|path| path.display().to_string())
}

pub fn pick_export(default_name: &str) -> Option<String> {
    rfd::FileDialog::new()
        .add_filter("Modrinth 整合包", &["mrpack"])
        .set_file_name(format!("{default_name}.mrpack"))
        .save_file()
        .map(|path| path.display().to_string())
}

pub fn pick_folder() -> Option<String> {
    rfd::FileDialog::new()
        .pick_folder()
        .map(|path| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_paths_reject_traversal() {
        assert!(safe_relative("../evil.jar").is_err());
        assert!(safe_relative("mods/../../evil.jar").is_err());
        assert!(safe_relative("/absolute.jar").is_err());
        assert!(safe_relative("C:\\evil.jar").is_err());
        assert_eq!(
            safe_relative("mods/ok.jar").unwrap(),
            PathBuf::from("mods").join("ok.jar")
        );
    }
}
