//! 实例内容管理：模组、光影包、资源包、存档、截图、投影原理图、
//! 服务器列表（servers.dat）与配置文件编辑。
//!
//! servers.dat / level.dat 为 gzip 压缩的 NBT，这里实现了一个最小
//! 的 NBT 解析与编码器，只用于读写这两类文件。

use base64::Engine;
use serde::Serialize;
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::UNIX_EPOCH,
};

use crate::instance_root;

const NBT_END: u8 = 0;
const NBT_BYTE: u8 = 1;
const NBT_SHORT: u8 = 2;
const NBT_INT: u8 = 3;
const NBT_LONG: u8 = 4;
const NBT_FLOAT: u8 = 5;
const NBT_DOUBLE: u8 = 6;
const NBT_BYTE_ARRAY: u8 = 7;
const NBT_STRING: u8 = 8;
const NBT_LIST: u8 = 9;
const NBT_COMPOUND: u8 = 10;
const NBT_INT_ARRAY: u8 = 11;
const NBT_LONG_ARRAY: u8 = 12;

const MAX_CONFIG_BYTES: u64 = 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 30 * 1024 * 1024;
const MAX_CONFIG_ENTRIES: usize = 4000;

const EDITABLE_EXTENSIONS: [&str; 19] = [
    "txt", "json", "toml", "cfg", "conf", "properties", "ini", "yml", "yaml", "mcmeta",
    "mcfunction", "log", "csv", "xml", "md", "hjson", "litematic_info", "instructions", "lang",
];

#[derive(Debug, Serialize)]
pub(crate) struct ContentItem {
    name: String,
    is_dir: bool,
    size: u64,
    modified: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    level_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_played: Option<u64>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ContentSummary {
    mod_count: u64,
    shader_count: u64,
    resourcepack_count: u64,
    save_count: u64,
    screenshot_count: u64,
    schematic_count: u64,
    server_count: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct ServerView {
    name: String,
    ip: String,
    hidden: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon_data_url: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct ServerInput {
    name: String,
    ip: String,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    icon_base64: Option<String>,
}

#[derive(Debug, Serialize)]
pub(crate) struct ConfigEntry {
    path: String,
    size: u64,
    modified: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct ConfigFile {
    path: String,
    content: String,
}

#[derive(Debug, Clone)]
enum NbtValue {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<u8>),
    String(String),
    List(u8, Vec<NbtValue>),
    Compound(Vec<(String, NbtValue)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

struct NbtReader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> NbtReader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, count: usize) -> Result<&'a [u8], String> {
        let end = self
            .pos
            .checked_add(count)
            .ok_or_else(|| "NBT 解析溢出".to_string())?;
        if end > self.data.len() {
            return Err("NBT 数据不完整".into());
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }

    fn i16(&mut self) -> Result<i16, String> {
        let b = self.take(2)?;
        Ok(i16::from_be_bytes([b[0], b[1]]))
    }

    fn i32(&mut self) -> Result<i32, String> {
        let b = self.take(4)?;
        Ok(i32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn i64(&mut self) -> Result<i64, String> {
        let b = self.take(8)?;
        Ok(i64::from_be_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn string(&mut self) -> Result<String, String> {
        let len = self.i16()? as usize;
        let bytes = self.take(len)?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }

    fn value(&mut self, tag: u8) -> Result<NbtValue, String> {
        match tag {
            NBT_BYTE => Ok(NbtValue::Byte(self.u8()? as i8)),
            NBT_SHORT => Ok(NbtValue::Short(self.i16()?)),
            NBT_INT => Ok(NbtValue::Int(self.i32()?)),
            NBT_LONG => Ok(NbtValue::Long(self.i64()?)),
            NBT_FLOAT => Ok(NbtValue::Float(f32::from_bits(self.i32()? as u32))),
            NBT_DOUBLE => Ok(NbtValue::Double(f64::from_bits(self.i64()? as u64))),
            NBT_BYTE_ARRAY => {
                let len = self.i32()?;
                let len = usize::try_from(len).map_err(|_| "NBT 字节数组长度无效".to_string())?;
                if len > self.data.len() {
                    return Err("NBT 字节数组超出范围".into());
                }
                Ok(NbtValue::ByteArray(self.take(len)?.to_vec()))
            }
            NBT_STRING => Ok(NbtValue::String(self.string()?)),
            NBT_LIST => {
                let item = self.u8()?;
                let len = self.i32()?;
                let len = usize::try_from(len).map_err(|_| "NBT 列表长度无效".to_string())?;
                let mut items = Vec::with_capacity(len.min(4096));
                for _ in 0..len {
                    items.push(self.value(item)?);
                }
                Ok(NbtValue::List(item, items))
            }
            NBT_COMPOUND => {
                let mut fields = Vec::new();
                loop {
                    let child = self.u8()?;
                    if child == NBT_END {
                        break;
                    }
                    // 复合字段名是带 u16 长度前缀的字符串，直接读即可。
                    let name = self.string()?;
                    fields.push((name, self.value(child)?));
                }
                Ok(NbtValue::Compound(fields))
            }
            NBT_INT_ARRAY => {
                let len = self.i32()?;
                let len = usize::try_from(len).map_err(|_| "NBT 整型数组长度无效".to_string())?;
                let mut items = Vec::with_capacity(len.min(4096));
                for _ in 0..len {
                    items.push(self.i32()?);
                }
                Ok(NbtValue::IntArray(items))
            }
            NBT_LONG_ARRAY => {
                let len = self.i32()?;
                let len = usize::try_from(len).map_err(|_| "NBT 长整型数组长度无效".to_string())?;
                let mut items = Vec::with_capacity(len.min(4096));
                for _ in 0..len {
                    items.push(self.i64()?);
                }
                Ok(NbtValue::LongArray(items))
            }
            other => Err(format!("未知 NBT 标签 {other}")),
        }
    }
}

fn nbt_write_string(out: &mut Vec<u8>, value: &str) {
    let bytes = value.as_bytes();
    out.extend_from_slice(&(bytes.len() as u16).to_be_bytes());
    out.extend_from_slice(bytes);
}

fn nbt_write_value(out: &mut Vec<u8>, value: &NbtValue) {
    match value {
        NbtValue::Byte(v) => out.push(*v as u8),
        NbtValue::Short(v) => out.extend_from_slice(&v.to_be_bytes()),
        NbtValue::Int(v) => out.extend_from_slice(&v.to_be_bytes()),
        NbtValue::Long(v) => out.extend_from_slice(&v.to_be_bytes()),
        NbtValue::Float(v) => out.extend_from_slice(&v.to_bits().to_be_bytes()),
        NbtValue::Double(v) => out.extend_from_slice(&v.to_bits().to_be_bytes()),
        NbtValue::ByteArray(items) => {
            out.extend_from_slice(&(items.len() as i32).to_be_bytes());
            out.extend_from_slice(items);
        }
        NbtValue::String(v) => nbt_write_string(out, v),
        NbtValue::List(item_tag, items) => {
            out.push(*item_tag);
            out.extend_from_slice(&(items.len() as i32).to_be_bytes());
            for item in items {
                nbt_write_value(out, item);
            }
        }
        NbtValue::Compound(fields) => {
            for (name, child) in fields {
                let child_tag = nbt_tag_of(child);
                out.push(child_tag);
                nbt_write_string(out, name);
                nbt_write_value(out, child);
            }
            out.push(NBT_END);
        }
        NbtValue::IntArray(items) => {
            out.extend_from_slice(&(items.len() as i32).to_be_bytes());
            for item in items {
                out.extend_from_slice(&item.to_be_bytes());
            }
        }
        NbtValue::LongArray(items) => {
            out.extend_from_slice(&(items.len() as i32).to_be_bytes());
            for item in items {
                out.extend_from_slice(&item.to_be_bytes());
            }
        }
    }
}

fn nbt_tag_of(value: &NbtValue) -> u8 {
    match value {
        NbtValue::Byte(_) => NBT_BYTE,
        NbtValue::Short(_) => NBT_SHORT,
        NbtValue::Int(_) => NBT_INT,
        NbtValue::Long(_) => NBT_LONG,
        NbtValue::Float(_) => NBT_FLOAT,
        NbtValue::Double(_) => NBT_DOUBLE,
        NbtValue::ByteArray(_) => NBT_BYTE_ARRAY,
        NbtValue::String(_) => NBT_STRING,
        // 列表值本身的标签是 NBT_LIST；元素类型写在列表负载里。
        NbtValue::List(_, _) => NBT_LIST,
        NbtValue::Compound(_) => NBT_COMPOUND,
        NbtValue::IntArray(_) => NBT_INT_ARRAY,
        NbtValue::LongArray(_) => NBT_LONG_ARRAY,
    }
}

#[cfg(test)]
fn write_named(out: &mut Vec<u8>, name: &str, value: &NbtValue) {
    let tag = nbt_tag_of(value);
    out.push(tag);
    nbt_write_string(out, name);
    nbt_write_value(out, value);
}

fn parse_nbt(data: &[u8]) -> Result<NbtValue, String> {
    let mut reader = NbtReader::new(data);
    let tag = reader.u8()?;
    if tag != NBT_COMPOUND {
        return Err("NBT 根节点不是 Compound".into());
    }
    let _root_name = reader.string()?;
    reader.value(NBT_COMPOUND)
}

fn encode_nbt(root: &NbtValue) -> Vec<u8> {
    let mut out = Vec::new();
    out.push(NBT_COMPOUND);
    nbt_write_string(&mut out, "");
    nbt_write_value(&mut out, root);
    out
}

fn read_gzip_or_raw(path: &Path) -> Result<Vec<u8>, String> {
    let bytes = fs::read(path).map_err(|e| format!("无法读取 {}：{e}", path.display()))?;
    if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        let mut decoder = flate2::read::GzDecoder::new(&bytes[..]);
        let mut plain = Vec::new();
        decoder
            .read_to_end(&mut plain)
            .map_err(|e| format!("gzip 解压失败：{e}"))?;
        Ok(plain)
    } else {
        Ok(bytes)
    }
}

fn write_gzip(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("无法创建目录：{e}"))?;
    }
    let mut encoder = flate2::write::GzEncoder::new(
        fs::File::create(path).map_err(|e| format!("无法写入文件：{e}"))?,
        flate2::Compression::default(),
    );
    std::io::Write::write_all(&mut encoder, bytes)
        .and_then(|()| encoder.finish().map(|_| ()))
        .map_err(|e| format!("gzip 压缩失败：{e}"))
}

fn field<'a>(compound: &'a [(String, NbtValue)], name: &str) -> Option<&'a NbtValue> {
    compound.iter().find(|(key, _)| key == name).map(|(_, v)| v)
}

fn as_string(value: &NbtValue) -> Option<String> {
    match value {
        NbtValue::String(text) => Some(text.clone()),
        _ => None,
    }
}

fn as_long(value: &NbtValue) -> Option<i64> {
    match value {
        NbtValue::Long(v) => Some(*v),
        _ => None,
    }
}

/// 从 level.dat 提取 LevelName 与 LastPlayed。
fn read_level_dat(path: &Path) -> Option<(String, Option<u64>)> {
    let root = parse_nbt(&read_gzip_or_raw(path).ok()?).ok()?;
    let compound = match &root {
        NbtValue::Compound(fields) => fields,
        _ => return None,
    };
    let data = field(compound, "Data")?;
    let data = match data {
        NbtValue::Compound(fields) => fields,
        _ => return None,
    };
    let name = field(data, "LevelName").and_then(as_string).unwrap_or_default();
    let played = field(data, "LastPlayed")
        .and_then(as_long)
        .filter(|ms| *ms > 0)
        .map(|ms| (ms / 1000) as u64);
    Some((name, played))
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    let stack = vec![path.to_path_buf()];
    let mut queue = stack;
    while let Some(current) = queue.pop() {
        let Ok(entries) = fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                queue.push(entry.path());
            } else {
                total = total.saturating_add(meta.len());
            }
        }
    }
    total
}

fn file_meta(path: &Path) -> (u64, u64) {
    let Ok(meta) = fs::metadata(path) else { return (0, 0) };
    let modified = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    (meta.len(), modified)
}

fn list_folder(root: &Path, folder: &str) -> Vec<ContentItem> {
    let dir = root.join(folder);
    if !dir.is_dir() {
        return Vec::new();
    }
    let mut items = Vec::new();
    let Ok(entries) = fs::read_dir(&dir) else {
        return items;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name() {
            Some(name) => name.to_string_lossy().into_owned(),
            None => continue,
        };
        let Ok(meta) = entry.metadata() else { continue };
        let is_dir = meta.is_dir();
        let size = if is_dir { dir_size(&path) } else { meta.len() };
        let modified = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        items.push(ContentItem {
            name,
            is_dir,
            size,
            modified,
            enabled: None,
            level_name: None,
            last_played: None,
        });
    }
    items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    items
}

fn kind_dir(root: &Path, kind: &str) -> Result<PathBuf, String> {
    let folder = match kind {
        "mod" => "mods",
        "shader" => "shaderpacks",
        "resourcepack" => "resourcepacks",
        "datapack" => "datapacks",
        "screenshot" => "screenshots",
        "schematic" => "schematics",
        "save" => "saves",
        _ => return Err("未知的内容类型".into()),
    };
    Ok(root.join(folder))
}

fn resolve_inside(dir: &Path, name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        return Err("文件名无效".into());
    }
    let target = dir.join(name);
    let canonical_dir = dir
        .canonicalize()
        .map_err(|_| "目录不存在".to_string())?;
    let canonical_target = target
        .canonicalize()
        .map_err(|_| "目标不存在".to_string())?;
    if !canonical_target.starts_with(&canonical_dir) {
        return Err("路径越界".into());
    }
    Ok(target)
}

pub(crate) fn servers_path(root: &Path) -> PathBuf {
    root.join("servers.dat")
}

fn read_servers(root: &Path) -> Vec<ServerView> {
    let path = servers_path(root);
    if !path.is_file() {
        return Vec::new();
    }
    let Ok(bytes) = read_gzip_or_raw(&path) else {
        return Vec::new();
    };
    let Ok(NbtValue::Compound(fields)) = parse_nbt(&bytes) else {
        return Vec::new();
    };
    let Some(NbtValue::List(_, servers)) = field(&fields, "servers") else {
        return Vec::new();
    };
    servers
        .iter()
        .filter_map(|value| {
            let fields = match value {
                NbtValue::Compound(fields) => fields,
                _ => return None,
            };
            let name = field(fields, "name").and_then(as_string).unwrap_or_default();
            let ip = field(fields, "ip").and_then(as_string).unwrap_or_default();
            let hidden = matches!(field(fields, "hidden"), Some(NbtValue::Byte(1)));
            let icon = field(fields, "icon").and_then(|value| match value {
                NbtValue::ByteArray(bytes) => Some(format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                )),
                _ => None,
            });
            Some(ServerView {
                name,
                ip,
                hidden,
                icon_data_url: icon,
            })
        })
        .collect()
}

fn decode_icon(icon_base64: &Option<String>) -> Option<Vec<u8>> {
    let text = icon_base64.as_deref()?.trim();
    if text.is_empty() {
        return None;
    }
    let payload = text.split_once(',').map(|(_, rest)| rest).unwrap_or(text);
    base64::engine::general_purpose::STANDARD.decode(payload).ok()
}

fn write_servers(root: &Path, servers: &[ServerInput]) -> Result<(), String> {
    let path = servers_path(root);
    // 读取现有文件以保留未知字段（例如 BungeeCord 的额外标签）。
    let mut root_value = match fs::read(&path) {
        Ok(bytes) => {
            let raw = read_gzip_or_raw(&path).unwrap_or(bytes);
            parse_nbt(&raw).unwrap_or_else(|_| NbtValue::Compound(Vec::new()))
        }
        Err(_) => NbtValue::Compound(Vec::new()),
    };
    let entries: Vec<NbtValue> = servers
        .iter()
        .map(|server| {
            let mut fields: Vec<(String, NbtValue)> = Vec::new();
            fields.push(("hidden".into(), NbtValue::Byte(server.hidden as i8)));
            fields.push(("icon".into(), NbtValue::ByteArray(
                decode_icon(&server.icon_base64).unwrap_or_default(),
            )));
            fields.push(("name".into(), NbtValue::String(server.name.clone())));
            fields.push(("ip".into(), NbtValue::String(server.ip.clone())));
            NbtValue::Compound(fields)
        })
        .collect();
    if let NbtValue::Compound(fields) = &mut root_value {
        fields.retain(|(key, _)| key != "servers");
        fields.push((
            "servers".into(),
            NbtValue::List(NBT_COMPOUND, entries),
        ));
    }
    let stamp = crate::chrono_like_stamp();
    if path.is_file() {
        let backup = root.join(format!("servers.dat.aurora-bak-{stamp}"));
        let _ = fs::copy(&path, &backup);
        // 只保留最近 3 份备份。
        let mut backups: Vec<PathBuf> = fs::read_dir(root)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| {
                        path.file_name()
                            .and_then(|name| name.to_str())
                            .map(|name| {
                                name.starts_with("servers.dat.aurora-bak-")
                            })
                            .unwrap_or(false)
                    })
                    .collect()
            })
            .unwrap_or_default();
        backups.sort();
        while backups.len() > 3 {
            let _ = fs::remove_file(backups.remove(0));
        }
    }
    write_gzip(&path, &encode_nbt(&root_value))
}

fn config_entry(root: &Path, relative: &str) -> Result<ConfigEntry, String> {
    let path = root.join(relative);
    let (size, modified) = file_meta(&path);
    Ok(ConfigEntry {
        path: relative.replace('\\', "/"),
        size,
        modified,
    })
}

fn list_config_dir(root: &Path, relative: &str, out: &mut Vec<ConfigEntry>, depth: usize) {
    if out.len() >= MAX_CONFIG_ENTRIES || depth > 6 {
        return;
    }
    let dir = root.join(relative);
    let Ok(entries) = fs::read_dir(&dir) else { return };
    let mut children: Vec<(String, bool)> = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let is_dir = entry.path().is_dir();
        children.push((name, is_dir));
    }
    children.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    for (name, is_dir) in children {
        let child_rel = if relative.is_empty() {
            name.clone()
        } else {
            format!("{relative}/{name}")
        };
        if is_dir {
            list_config_dir(root, &child_rel, out, depth + 1);
        } else if config_editable(&name) {
            if let Ok(entry) = config_entry(root, &child_rel) {
                out.push(entry);
            }
        }
        if out.len() >= MAX_CONFIG_ENTRIES {
            return;
        }
    }
}

fn config_editable(name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((_, ext)) => EDITABLE_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()),
        None => true,
    }
}

/// 确保配置路径位于实例根目录下且是允许编辑的文本文件。
fn resolve_config_path(root: &Path, relative: &str, for_write: bool) -> Result<PathBuf, String> {
    let cleaned = relative.replace('\\', "/");
    if cleaned.is_empty() || cleaned.contains("..") || cleaned.starts_with('/') {
        return Err("配置路径无效".into());
    }
    let segments: Vec<&str> = cleaned.split('/').filter(|s| !s.is_empty() && *s != ".").collect();
    if segments.is_empty() {
        return Err("配置路径无效".into());
    }
    // 只允许根目录下的顶层文件与 config/ 目录树。
    let allowed = segments.len() == 1 || segments[0] == "config";
    if !allowed {
        return Err("只允许编辑实例根目录下的顶层文件或 config/ 内的文件".into());
    }
    let name = segments[segments.len() - 1];
    if !config_editable(name) {
        return Err("该文件类型不支持在线编辑".into());
    }
    if for_write && segments.len() == 1 && name != "options.txt" {
        return Err("根目录下只允许修改 options.txt".into());
    }
    let target = segments.iter().fold(root.to_path_buf(), |acc, part| acc.join(part));
    if target.exists() {
        let canonical_root = root.canonicalize().map_err(|_| "实例目录不存在".to_string())?;
        let canonical_target = target
            .canonicalize()
            .map_err(|_| "文件不存在".to_string())?;
        if !canonical_target.starts_with(&canonical_root) {
            return Err("路径越界".into());
        }
    }
    Ok(target)
}

#[tauri::command]
pub(crate) fn instance_content_summary(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<ContentSummary, String> {
    let root = instance_root(&app, &instance_id)?;
    let count = |folder: &str, filter: fn(&ContentItem) -> bool| -> u64 {
        list_folder(&root, folder).iter().filter(|item| filter(item)).count() as u64
    };
    let is_jar = |item: &ContentItem| item.name.to_lowercase().ends_with(".jar");
    let any = |_: &ContentItem| true;
    Ok(ContentSummary {
        mod_count: count("mods", is_jar) + count("mods", |item| item.name.to_lowercase().ends_with(".jar.disabled")),
        shader_count: count("shaderpacks", any),
        resourcepack_count: count("resourcepacks", any),
        save_count: count("saves", |item| item.is_dir),
        screenshot_count: count("screenshots", |item| !item.is_dir),
        schematic_count: count("schematics", |item| !item.is_dir),
        server_count: read_servers(&root).len() as u64,
    })
}

#[tauri::command]
pub(crate) fn instance_content_list(
    app: tauri::AppHandle,
    instance_id: String,
    kind: String,
) -> Result<Vec<ContentItem>, String> {
    let root = instance_root(&app, &instance_id)?;
    let mut items = match kind.as_str() {
        "server" => {
            return Ok(read_servers(&root)
                .into_iter()
                .map(|server| ContentItem {
                    name: server.name,
                    is_dir: false,
                    size: 0,
                    modified: 0,
                    enabled: None,
                    level_name: Some(server.ip),
                    last_played: None,
                })
                .collect());
        }
        "save" => {
            let mut items = list_folder(&root, "saves")
                .into_iter()
                .filter(|item| item.is_dir)
                .collect::<Vec<_>>();
            for item in &mut items {
                let level = root.join("saves").join(&item.name).join("level.dat");
                if let Some((level_name, last_played)) = read_level_dat(&level) {
                    if !level_name.is_empty() {
                        item.level_name = Some(level_name);
                    }
                    item.last_played = last_played;
                }
            }
            items.sort_by(|a, b| b.last_played.cmp(&a.last_played));
            return Ok(items);
        }
        "mod" | "shader" | "resourcepack" | "datapack" | "screenshot" | "schematic" => {
            list_folder(&root, match kind.as_str() {
                "mod" => "mods",
                "shader" => "shaderpacks",
                "resourcepack" => "resourcepacks",
                "datapack" => "datapacks",
                "screenshot" => "screenshots",
                _ => "schematics",
            })
        }
        _ => return Err("未知的内容类型".into()),
    };
    if kind == "mod" {
        for item in &mut items {
            let lower = item.name.to_lowercase();
            if lower.ends_with(".jar") {
                item.enabled = Some(true);
            } else if lower.ends_with(".jar.disabled") {
                item.enabled = Some(false);
            }
        }
        items.retain(|item| item.enabled.is_some() || item.is_dir);
        items.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    }
    if kind == "shader" || kind == "resourcepack" {
        items.retain(|item| {
            item.is_dir
                || [".zip", ".jar"]
                    .iter()
                    .any(|ext| item.name.to_lowercase().ends_with(ext))
        });
    }
    if kind == "schematic" {
        items.retain(|item| {
            item.is_dir
                || [".litematic", ".schem", ".schematic", ".nbt", ".mcstructure"]
                    .iter()
                    .any(|ext| item.name.to_lowercase().ends_with(ext))
        });
    }
    if kind == "screenshot" {
        items.retain(|item| {
            !item.is_dir
                && [".png", ".jpg", ".jpeg"]
                    .iter()
                    .any(|ext| item.name.to_lowercase().ends_with(ext))
        });
        items.sort_by(|a, b| b.modified.cmp(&a.modified));
    }
    Ok(items)
}

#[tauri::command]
pub(crate) fn instance_content_delete(
    app: tauri::AppHandle,
    instance_id: String,
    kind: String,
    name: String,
) -> Result<(), String> {
    let root = instance_root(&app, &instance_id)?;
    let dir = kind_dir(&root, &kind)?;
    let target = resolve_inside(&dir, &name)?;
    if target.is_dir() {
        fs::remove_dir_all(&target).map_err(|e| format!("删除失败：{e}"))
    } else if target.is_file() {
        fs::remove_file(&target).map_err(|e| format!("删除失败：{e}"))
    } else {
        Err("目标不存在".into())
    }
}

#[tauri::command]
pub(crate) fn instance_mod_toggle(
    app: tauri::AppHandle,
    instance_id: String,
    name: String,
    enable: bool,
) -> Result<(), String> {
    let root = instance_root(&app, &instance_id)?;
    let dir = kind_dir(&root, "mod")?;
    toggle_one(&dir, &name, enable)
}

fn toggle_one(dir: &Path, name: &str, enable: bool) -> Result<(), String> {
    let source = resolve_inside(dir, name)?;
    if source.is_dir() {
        return Err("文件夹不能启停".into());
    }
    let target_name = toggle_name(name, enable).ok_or_else(|| {
        if enable {
            "文件未处于禁用状态".to_string()
        } else {
            "只有 .jar 文件可以禁用".to_string()
        }
    })?;
    // 已处于目标状态的文件跳过，便于批量操作时直接全选。
    let target = dir.join(&target_name);
    if target.exists() && target != source {
        return Err(format!("目标已存在：{target_name}"));
    }
    fs::rename(&source, target).map_err(|e| format!("重命名失败：{e}"))
}

#[tauri::command]
pub(crate) fn instance_mod_toggle_many(
    app: tauri::AppHandle,
    instance_id: String,
    names: Vec<String>,
    enable: bool,
) -> Result<usize, String> {
    let root = instance_root(&app, &instance_id)?;
    let dir = kind_dir(&root, "mod")?;
    if names.len() > 2000 {
        return Err("一次最多处理 2000 个文件".into());
    }
    let mut changed = 0;
    let mut skipped = 0;
    let mut failures: Vec<String> = Vec::new();
    for name in &names {
        match toggle_one(&dir, name, enable) {
            Ok(()) => changed += 1,
            // 名称未带目标后缀说明本来就处于目标状态，算跳过而不是失败。
            Err(error) if error.starts_with("文件未处于") || error.starts_with("只有 .jar") => {
                skipped += 1;
            }
            Err(error) => failures.push(format!("{name}：{error}")),
        }
    }
    if !failures.is_empty() {
        let detail = failures.iter().take(5).cloned().collect::<Vec<_>>().join("；");
        return Err(format!(
            "{detail}{}",
            if failures.len() > 5 {
                format!("；等 {} 个文件失败", failures.len())
            } else {
                String::new()
            }
        ));
    }
    let _ = skipped;
    Ok(changed)
}

/// 生成启停后的文件名：只忽略大小写地匹配后缀，保留原文件名的其余部分。
fn toggle_name(name: &str, enable: bool) -> Option<String> {    let lower = name.to_lowercase();
    if enable {
        if !lower.ends_with(".jar.disabled") {
            return None;
        }
        // 去掉尾部 .disabled 即可，原 .JAR 的大小写保持不变。
        let prefix = name.get(..name.len().checked_sub(".disabled".len())?)?;
        Some(prefix.to_string())
    } else {
        if !lower.ends_with(".jar") {
            return None;
        }
        Some(format!("{name}.disabled"))
    }
}

#[tauri::command]
pub(crate) fn instance_content_open_folder(
    app: tauri::AppHandle,
    instance_id: String,
    kind: String,
) -> Result<(), String> {
    let root = instance_root(&app, &instance_id)?;
    let dir = if kind == "server" {
        root.clone()
    } else {
        let dir = kind_dir(&root, &kind)?;
        fs::create_dir_all(&dir).map_err(|e| format!("无法创建目录：{e}"))?;
        dir
    };
    tauri_plugin_opener::open_path(dir, None::<&str>)
        .map_err(|e| format!("无法打开目录：{e}"))
}

#[tauri::command]
pub(crate) fn instance_image_preview(
    app: tauri::AppHandle,
    instance_id: String,
    kind: String,
    name: String,
) -> Result<String, String> {
    let root = instance_root(&app, &instance_id)?;
    let dir = kind_dir(&root, &kind)?;
    let target = resolve_inside(&dir, &name)?;
    if target.is_dir() {
        return Err("不支持预览文件夹".into());
    }
    let lower = name.to_lowercase();
    let mime = if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else {
        return Err("只支持预览 PNG/JPG 图片".into());
    };
    let meta = fs::metadata(&target).map_err(|e| format!("无法读取文件：{e}"))?;
    if meta.len() > MAX_IMAGE_BYTES {
        return Err("图片过大，无法预览".into());
    }
    let bytes = fs::read(&target).map_err(|e| format!("无法读取文件：{e}"))?;
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

#[tauri::command]
pub(crate) fn instance_server_list(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<Vec<ServerView>, String> {
    let root = instance_root(&app, &instance_id)?;
    Ok(read_servers(&root))
}

#[tauri::command]
pub(crate) fn instance_server_set(
    app: tauri::AppHandle,
    instance_id: String,
    servers: Vec<ServerInput>,
) -> Result<(), String> {
    let root = instance_root(&app, &instance_id)?;
    for server in &servers {
        if server.name.trim().is_empty() || server.ip.trim().is_empty() {
            return Err("服务器名称与地址不能为空".into());
        }
        if server.name.len() > 64 || server.ip.len() > 255 {
            return Err("服务器名称或地址过长".into());
        }
    }
    if servers.len() > 200 {
        return Err("服务器数量过多".into());
    }
    write_servers(&root, &servers)
}

#[tauri::command]
pub(crate) fn instance_config_list(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<Vec<ConfigEntry>, String> {
    let root = instance_root(&app, &instance_id)?;
    let mut entries: Vec<ConfigEntry> = Vec::new();
    if config_editable("options.txt") {
        let options = root.join("options.txt");
        if options.is_file() {
            if let Ok(entry) = config_entry(&root, "options.txt") {
                entries.push(entry);
            }
        }
    }
    list_config_dir(&root, "config", &mut entries, 0);
    Ok(entries)
}

#[tauri::command]
pub(crate) fn instance_config_read(
    app: tauri::AppHandle,
    instance_id: String,
    path: String,
) -> Result<ConfigFile, String> {
    let root = instance_root(&app, &instance_id)?;
    let target = resolve_config_path(&root, &path, false)?;
    if !target.is_file() {
        return Err("文件不存在".into());
    }
    let (size, _) = file_meta(&target);
    if size > MAX_CONFIG_BYTES {
        return Err("文件超过 1 MB，请在编辑器中打开".into());
    }
    let bytes = fs::read(&target).map_err(|e| format!("无法读取文件：{e}"))?;
    let content = String::from_utf8_lossy(&bytes).into_owned();
    Ok(ConfigFile {
        path: path.replace('\\', "/"),
        content,
    })
}

#[tauri::command]
pub(crate) fn instance_config_write(
    app: tauri::AppHandle,
    instance_id: String,
    path: String,
    content: String,
) -> Result<(), String> {
    let root = instance_root(&app, &instance_id)?;
    let target = resolve_config_path(&root, &path, true)?;
    if content.len() as u64 > MAX_CONFIG_BYTES {
        return Err("内容超过 1 MB".into());
    }
    if target.is_file() {
        let stamp = crate::chrono_like_stamp();
        let backup = target.with_file_name(format!(
            "{}.aurora-bak-{stamp}",
            target
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "config".into())
        ));
        let _ = fs::copy(&target, &backup);
    } else if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("无法创建目录：{e}"))?;
    }
    fs::write(&target, content).map_err(|e| format!("写入失败：{e}"))
}

#[tauri::command]
pub(crate) fn instance_content_upload(
    app: tauri::AppHandle,
    instance_id: String,
    kind: String,
    source: String,
) -> Result<String, String> {
    // 把磁盘上的文件复制到对应内容目录（光影包 / 资源包 / 原理图）。
    let root = instance_root(&app, &instance_id)?;
    let dir = kind_dir(&root, &kind)?;
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建目录：{e}"))?;
    let source_path = PathBuf::from(&source);
    if !source_path.is_file() {
        return Err("所选文件不存在".into());
    }
    let Some(name) = safe_display_name(&source_path) else {
        return Err("文件名无效".into());
    };
    let target = dir.join(&name);
    if target.exists() {
        return Err(format!("已存在同名文件：{name}"));
    }
    fs::copy(&source_path, &target).map_err(|e| format!("复制失败：{e}"))?;
    Ok(name)
}

fn safe_display_name(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\']) {
        return None;
    }
    Some(name)
}

/// 把 saves/<name> 打包成 zip（备份/分享用），zip 内保留世界目录名。
#[tauri::command]
pub(crate) fn instance_save_export(
    app: tauri::AppHandle,
    instance_id: String,
    name: String,
    path: String,
) -> Result<String, String> {
    let root = instance_root(&app, &instance_id)?;
    let dir = kind_dir(&root, "save")?;
    let source = resolve_inside(&dir, &name)?;
    if !source.is_dir() {
        return Err("世界目录不存在".into());
    }
    let target = PathBuf::from(&path);
    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|e| format!("无法创建目录：{e}"))?;
        }
    }
    let file = fs::File::create(&target).map_err(|e| format!("无法创建文件：{e}"))?;
    let mut zip = zip::ZipWriter::new(file);
    let prefix = format!("{}/", name);
    let count = crate::packs::add_dir_to_zip(&mut zip, &source, &prefix)?;
    zip.finish().map_err(|e| format!("写入 zip 失败：{e}"))?;
    Ok(format!("{count} 个文件"))
}

#[tauri::command]
pub(crate) fn instance_save_open(
    app: tauri::AppHandle,
    instance_id: String,
    name: String,
) -> Result<(), String> {
    let root = instance_root(&app, &instance_id)?;
    let dir = kind_dir(&root, "save")?;
    let target = resolve_inside(&dir, &name)?;
    tauri_plugin_opener::open_path(target, None::<&str>)
        .map_err(|e| format!("无法打开目录：{e}"))
}

/// 模组元数据：从 jar 内的 fabric.mod.json / quilt.mod.json /
/// META-INF/mods.toml / mcmod.info 本地解析，不联网。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct ModMeta {
    title: Option<String>,
    version: Option<String>,
    mod_id: Option<String>,
    loader: Option<String>,
    description: Option<String>,
}

const MAX_META_ENTRY: u64 = 256 * 1024;
const MAX_META_FILES: usize = 800;

type MetaCache = Mutex<HashMap<PathBuf, (u64, u64, Option<ModMeta>)>>;

fn meta_cache() -> &'static MetaCache {
    static CACHE: OnceLock<MetaCache> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn clean_text(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Fabric 常把版本号写成 ${version}，构建时未替换的占位符没有意义。
    if trimmed.starts_with("${") && trimmed.ends_with('}') {
        return None;
    }
    let mut out: String = trimmed.chars().take(220).collect();
    if trimmed.chars().count() > 220 {
        out.push('…');
    }
    Some(out)
}

fn read_zip_text(zip: &mut zip::ZipArchive<fs::File>, name: &str) -> Option<String> {
    let mut entry = zip.by_name(name).ok()?;
    if entry.size() > MAX_META_ENTRY {
        return None;
    }
    let mut buffer = Vec::new();
    entry.read_to_end(&mut buffer).ok()?;
    Some(String::from_utf8_lossy(&buffer).into_owned())
}

/// 取 TOML 字符串字面量的内容，支持 """ 与 ''' 多行写法。
fn toml_unquote(value: &str) -> Option<&str> {
    for quote in ["'''", "\"\"\"", "\"", "'"] {
        if let Some(rest) = value.strip_prefix(quote) {
            if let Some(inner) = rest.strip_suffix(quote) {
                return Some(inner);
            }
        }
    }
    None
}

/// 从 TOML 文本里取 `key = "value"`（Forge/NeoForge 的 mods.toml 用这种写法）。
fn toml_string(text: &str, key: &str) -> Option<String> {
    for raw in text.lines() {
        let line = raw.trim();
        if line.starts_with('#') || line.starts_with("[[") {
            continue;
        }
        let Some((name, rest)) = line.split_once('=') else {
            continue;
        };
        if name.trim() != key {
            continue;
        }
        return toml_unquote(rest.trim()).and_then(clean_text);
    }
    None
}

fn parse_fabric(text: &str) -> Option<ModMeta> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    Some(ModMeta {
        title: value
            .get("name")
            .and_then(|v| v.as_str())
            .and_then(clean_text),
        version: value
            .get("version")
            .and_then(|v| v.as_str())
            .and_then(clean_text),
        mod_id: value
            .get("id")
            .and_then(|v| v.as_str())
            .and_then(clean_text),
        loader: Some("fabric".into()),
        description: value
            .get("description")
            .and_then(|v| v.as_str())
            .and_then(clean_text),
    })
}

fn parse_quilt(text: &str) -> Option<ModMeta> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let loader = value.get("quilt_loader")?;
    let metadata = loader.get("metadata");
    Some(ModMeta {
        title: metadata
            .and_then(|m| m.get("name"))
            .and_then(|v| v.as_str())
            .and_then(clean_text),
        version: loader
            .get("version")
            .and_then(|v| v.as_str())
            .and_then(clean_text),
        mod_id: loader.get("id").and_then(|v| v.as_str()).and_then(clean_text),
        loader: Some("quilt".into()),
        description: metadata
            .and_then(|m| m.get("description"))
            .and_then(|v| v.as_str())
            .and_then(clean_text),
    })
}

fn parse_forge(text: &str, loader: &str) -> Option<ModMeta> {
    // 取第一个 [[mods]] 块；modId 与 displayName 都在其中。
    let block = text.split("[[mods]]").nth(1).unwrap_or(text);
    Some(ModMeta {
        title: toml_string(block, "displayName"),
        version: toml_string(block, "version"),
        mod_id: toml_string(block, "modId"),
        loader: Some(loader.into()),
        description: toml_string(block, "description"),
    })
}

fn parse_mcmod_info(text: &str) -> Option<ModMeta> {
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    let first = value
        .as_array()
        .and_then(|items| items.first())
        .or_else(|| value.get("modList").and_then(|list| list.get(0)))?;
    Some(ModMeta {
        title: first
            .get("name")
            .and_then(|v| v.as_str())
            .and_then(clean_text),
        version: first
            .get("version")
            .and_then(|v| v.as_str())
            .and_then(clean_text),
        mod_id: first
            .get("modid")
            .and_then(|v| v.as_str())
            .and_then(clean_text),
        loader: Some("forge".into()),
        description: first
            .get("description")
            .and_then(|v| v.as_str())
            .and_then(clean_text),
    })
}

fn read_mod_meta(path: &Path) -> Option<ModMeta> {
    let file = fs::File::open(path).ok()?;
    let mut zip = zip::ZipArchive::new(file).ok()?;

    let candidates: [(&str, fn(&str) -> Option<ModMeta>); 4] = [
        ("fabric.mod.json", parse_fabric),
        ("quilt.mod.json", parse_quilt),
        ("META-INF/neoforge.mods.toml", |text| {
            parse_forge(text, "neoforge")
        }),
        ("META-INF/mods.toml", |text| parse_forge(text, "forge")),
    ];
    for (name, parser) in candidates {
        if let Some(text) = read_zip_text(&mut zip, name) {
            if let Some(meta) = parser(&text) {
                if meta.title.is_some() || meta.mod_id.is_some() {
                    return Some(meta);
                }
            }
        }
    }
    if let Some(text) = read_zip_text(&mut zip, "mcmod.info") {
        if let Some(meta) = parse_mcmod_info(&text) {
            if meta.title.is_some() || meta.mod_id.is_some() {
                return Some(meta);
            }
        }
    }
    None
}

/// 带 (大小, 修改时间) 校验的缓存：重复打开面板不会反复解压 jar。
fn cached_mod_meta(path: &Path) -> Option<ModMeta> {
    let (size, modified) = file_meta(path);
    if let Ok(cache) = meta_cache().lock() {
        if let Some((cached_size, cached_time, meta)) = cache.get(path) {
            if *cached_size == size && *cached_time == modified {
                return meta.clone();
            }
        }
    }
    let meta = read_mod_meta(path);
    if let Ok(mut cache) = meta_cache().lock() {
        cache.insert(path.to_path_buf(), (size, modified, meta.clone()));
    }
    meta
}

#[tauri::command]
pub(crate) fn instance_mod_metadata(
    app: tauri::AppHandle,
    instance_id: String,
) -> Result<HashMap<String, ModMeta>, String> {
    let root = instance_root(&app, &instance_id)?;
    let dir = kind_dir(&root, "mod")?;
    let mut out = HashMap::new();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Ok(out);
    };
    for entry in entries.flatten().take(MAX_META_FILES) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        if !name.to_lowercase().ends_with(".jar") && !name.to_lowercase().ends_with(".jar.disabled")
        {
            continue;
        }
        if let Some(meta) = cached_mod_meta(&path) {
            out.insert(name, meta);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_servers_nbt() -> Vec<u8> {
        let mut out = Vec::new();
        out.push(NBT_COMPOUND);
        nbt_write_string(&mut out, "");
        let servers: Vec<NbtValue> = vec![
            NbtValue::Compound(vec![
                ("hidden".into(), NbtValue::Byte(0)),
                (
                    "icon".into(),
                    NbtValue::ByteArray(vec![0x89, b'P', b'N', b'G']),
                ),
                ("name".into(), NbtValue::String("生存服".into())),
                ("ip".into(), NbtValue::String("mc.example.com".into())),
            ]),
            NbtValue::Compound(vec![
                ("hidden".into(), NbtValue::Byte(1)),
                ("icon".into(), NbtValue::ByteArray(Vec::new())),
                ("name".into(), NbtValue::String("空岛".into())),
                ("ip".into(), NbtValue::String("10.0.0.1:25565".into())),
            ]),
        ];
        write_named(
            &mut out,
            "servers",
            &NbtValue::List(NBT_COMPOUND, servers),
        );
        out.push(NBT_END);
        out
    }

    #[test]
    fn nbt_servers_roundtrip() {
        let raw = sample_servers_nbt();
        let parsed = parse_nbt(&raw).expect("parse");
        let encoded = encode_nbt(&parsed);
        let reparsed = parse_nbt(&encoded).expect("reparse");
        assert_eq!(format!("{parsed:?}"), format!("{reparsed:?}"));
        if let NbtValue::Compound(fields) = &parsed {
            if let Some(NbtValue::List(NBT_COMPOUND, items)) = field(fields, "servers") {
                assert_eq!(items.len(), 2);
                if let NbtValue::Compound(first) = &items[0] {
                    assert_eq!(
                        field(first, "name").and_then(as_string).as_deref(),
                        Some("生存服")
                    );
                } else {
                    panic!("first server not compound");
                }
            } else {
                panic!("servers not list");
            }
        }
    }

    #[test]
    fn config_path_rejects_traversal() {
        let temp = std::env::temp_dir().join(format!("aurora-cfg-test-{}", std::process::id()));
        fs::create_dir_all(temp.join("config")).unwrap();
        let root = temp.clone();
        assert!(resolve_config_path(&root, "../options.txt", false).is_err());
        assert!(resolve_config_path(&root, "mods/foo.jar", false).is_err());
        assert!(resolve_config_path(&root, "config/server.properties", false).is_ok());
        assert!(resolve_config_path(&root, "options.txt", true).is_ok());
        assert!(resolve_config_path(&root, "something.toml", true).is_err());
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn mod_toggle_names() {
        assert_eq!(toggle_name("foo.jar", false), Some("foo.jar.disabled".into()));
        assert_eq!(toggle_name("foo.jar.disabled", true), Some("foo.jar".into()));
        // 大小写必须保留，否则重命名会改变模组文件名。
        assert_eq!(
            toggle_name("JEI-1.20.1-Fabric.jar", false),
            Some("JEI-1.20.1-Fabric.jar.disabled".into())
        );
        assert_eq!(
            toggle_name("Sodium-0.5.jar.disabled", true),
            Some("Sodium-0.5.jar".into())
        );
        // 后缀大小写不敏感，但仍然只改后缀。
        assert_eq!(toggle_name("Mod.JAR", false), Some("Mod.JAR.disabled".into()));
        assert_eq!(toggle_name("Mod.JAR.disabled", true), Some("Mod.JAR".into()));
        // 非 jar 与已启用/已禁用的错误状态。
        assert_eq!(toggle_name("readme.txt", false), None);
        assert_eq!(toggle_name("foo.jar", true), None);
        assert_eq!(toggle_name("foo.jar.disabled", false), None);
    }

    #[test]
    fn parses_fabric_metadata() {
        let meta = parse_fabric(
            r#"{"schemaVersion":1,"id":"sodium","name":"Sodium","version":"0.5.11+mc1.20.1","description":"Rendering"}"#,
        )
        .unwrap();
        assert_eq!(meta.title.as_deref(), Some("Sodium"));
        assert_eq!(meta.version.as_deref(), Some("0.5.11+mc1.20.1"));
        assert_eq!(meta.mod_id.as_deref(), Some("sodium"));
        assert_eq!(meta.loader.as_deref(), Some("fabric"));
    }

    #[test]
    fn skips_unresolved_version_placeholder() {
        let meta = parse_fabric(r#"{"id":"x","name":"X","version":"${version}"}"#).unwrap();
        assert_eq!(meta.version, None);
    }

    #[test]
    fn parses_forge_toml_metadata() {
        let text = "modLoader=\"javafml\"\nloaderVersion=\"[47,)\"\n[[mods]]\nmodId=\"examplemod\"\nversion=\"1.2.3\"\ndisplayName=\"Example Mod\"\ndescription='''A demo mod'''\n";
        let meta = parse_forge(text, "forge").unwrap();
        assert_eq!(meta.title.as_deref(), Some("Example Mod"));
        assert_eq!(meta.version.as_deref(), Some("1.2.3"));
        assert_eq!(meta.mod_id.as_deref(), Some("examplemod"));
        assert_eq!(meta.description.as_deref(), Some("A demo mod"));
    }

    #[test]
    fn parses_quilt_and_mcmod_info() {
        let quilt = parse_quilt(
            r#"{"quilt_loader":{"id":"q","version":"2.0.0","metadata":{"name":"Quilt Mod","description":"d"}}}"#,
        )
        .unwrap();
        assert_eq!(quilt.title.as_deref(), Some("Quilt Mod"));
        assert_eq!(quilt.loader.as_deref(), Some("quilt"));

        let legacy =
            parse_mcmod_info(r#"[{"modid":"old","name":"Old Mod","version":"1.7.10"}]"#).unwrap();
        assert_eq!(legacy.title.as_deref(), Some("Old Mod"));
        assert_eq!(legacy.mod_id.as_deref(), Some("old"));
        assert_eq!(legacy.loader.as_deref(), Some("forge"));
    }

    #[test]
    fn toggle_one_renames_on_disk() {
        let temp = std::env::temp_dir().join(format!("aurora-mod-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp);
        fs::create_dir_all(&temp).unwrap();
        for name in ["Sodium.jar", "JEI.jar.disabled"] {
            fs::write(temp.join(name), b"jar").unwrap();
        }
        // 禁用 → 追加 .disabled，文件名大小写不变。
        toggle_one(&temp, "Sodium.jar", false).unwrap();
        assert!(temp.join("Sodium.jar.disabled").exists());
        assert!(!temp.join("Sodium.jar").exists());
        // 启用 → 去掉 .disabled。
        toggle_one(&temp, "JEI.jar.disabled", true).unwrap();
        assert!(temp.join("JEI.jar").exists());
        // 已处于目标状态时返回可识别的错误，供批量操作按“跳过”处理。
        let error = toggle_one(&temp, "JEI.jar", true).unwrap_err();
        assert!(error.starts_with("文件未处于"), "{error}");
        // 目录不能启停。
        fs::create_dir_all(temp.join("folder.jar")).unwrap();
        assert!(toggle_one(&temp, "folder.jar", false).is_err());
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn invalid_metadata_is_ignored() {
        assert!(parse_fabric("not json").is_none());
        assert!(parse_mcmod_info("{}").is_none());
        // 没有 [[mods]] 块时不应 panic。
        let meta = parse_forge("modLoader=\"javafml\"", "forge").unwrap();
        assert_eq!(meta.title, None);
        assert_eq!(meta.mod_id, None);
    }
}
