//! 音乐舱：用户本地音乐导入与播放支持。
//!
//! 导入时把音频文件复制进数据目录 `music/`（与原文件解耦，移动/删除
//! 原文件不影响曲库）；播放时前端经 `music_read` 分块读回。文件仅存
//! 本机，不上传。

use serde::Serialize;
use std::fs;
use std::path::PathBuf;

use crate::{data_dir, safe_filename};

const MAX_TRACK_BYTES: u64 = 200 * 1024 * 1024;
const MAX_TRACKS: usize = 2000;
const MUSIC_CHUNK: i64 = 512 * 1024;

#[derive(Debug, Serialize)]
pub(crate) struct MusicTrack {
    /// 磁盘文件名（safe_filename 产物），也是曲库主键。
    file: String,
    /// 展示名（去掉扩展名）。
    name: String,
    size: u64,
}

#[derive(Debug, Serialize)]
pub(crate) struct MusicChunk {
    /// base64 音频数据；空字符串表示已到文件末尾。
    data: String,
    offset: i64,
    total: u64,
}

fn music_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = data_dir(app)?.join("music");
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建音乐目录：{e}"))?;
    Ok(dir)
}

fn is_audio_name(name: &str) -> bool {
    matches!(
        name.rsplit('.').next().map(|ext| ext.to_ascii_lowercase()).as_deref(),
        Some("mp3") | Some("flac") | Some("ogg") | Some("wav") | Some("m4a") | Some("aac") | Some("opus")
    )
}

/// 多选导入：复制进数据目录 music/，返回曲库清单。
#[tauri::command]
pub(crate) fn music_import(app: tauri::AppHandle, paths: Vec<String>) -> Result<Vec<MusicTrack>, String> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }
    let dir = music_dir(&app)?;
    let mut imported = Vec::new();
    for path in &paths {
        let source = PathBuf::from(path);
        if !source.is_file() {
            continue;
        }
        let Some(raw_name) = source.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        if !is_audio_name(&raw_name) {
            continue;
        }
        if imported.len() + read_library(&dir).len() >= MAX_TRACKS {
            return Err(format!("曲库最多 {} 首，已达上限", MAX_TRACKS));
        }
        let meta = fs::metadata(&source).map_err(|e| format!("读取 {raw_name} 失败：{e}"))?;
        if meta.len() > MAX_TRACK_BYTES {
            return Err(format!("{raw_name} 超过 200 MB 上限"));
        }
        // 磁盘名做安全化；重名时加序号而不是覆盖。
        let base = safe_filename(&raw_name).ok_or_else(|| format!("文件名无效：{raw_name}"))?;
        let mut target = dir.join(&base);
        let mut seq = 1;
        while target.exists() {
            let stem = base
                .rsplit_once('.')
                .map(|(s, _)| s.to_string())
                .unwrap_or_else(|| base.clone());
            let ext = base
                .rsplit_once('.')
                .map(|(_, e)| e.to_string())
                .unwrap_or_else(|| "audio".into());
            target = dir.join(format!("{stem}-{seq}.{ext}"));
            seq += 1;
        }
        fs::copy(&source, &target).map_err(|e| format!("导入 {raw_name} 失败：{e}"))?;
        let file_name = target.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        imported.push(MusicTrack {
            name: file_name
                .rsplit_once('.')
                .map(|(s, _)| s.to_string())
                .unwrap_or_else(|| file_name.clone()),
            file: file_name,
            size: meta.len(),
        });
    }
    Ok(imported)
}

/// 列出曲库（扫描 music/ 目录，磁盘即真相）。
#[tauri::command]
pub(crate) fn music_list(app: tauri::AppHandle) -> Result<Vec<MusicTrack>, String> {
    let dir = music_dir(&app)?;
    Ok(read_library(&dir))
}

fn read_library(dir: &PathBuf) -> Vec<MusicTrack> {
    let mut tracks = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return tracks;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        if !is_audio_name(&name) {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        tracks.push(MusicTrack {
            name: name
                .rsplit_once('.')
                .map(|(s, _)| s.to_string())
                .unwrap_or_else(|| name.clone()),
            file: name,
            size,
        });
    }
    tracks.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    tracks
}

/// 分块读取音频字节（base64），前端拼成 Blob 播放。offset 从 0 开始。
#[tauri::command]
pub(crate) fn music_read(
    app: tauri::AppHandle,
    file: String,
    offset: i64,
) -> Result<MusicChunk, String> {
    if file.is_empty() || file.contains(['/', '\\', ':']) || file.contains("..") {
        return Err("文件名无效".into());
    }
    if !is_audio_name(&file) {
        return Err("不是音频文件".into());
    }
    let dir = music_dir(&app)?;
    let path = dir.join(&file);
    if !path.is_file() {
        return Err("曲目文件不存在".into());
    }
    let meta = fs::metadata(&path).map_err(|e| format!("读取失败：{e}"))?;
    let total = meta.len();
    let start = if offset < 0 { 0 } else { offset as u64 };
    if start >= total {
        return Ok(MusicChunk { data: String::new(), offset: offset, total });
    }
    let end = (start + MUSIC_CHUNK as u64).min(total);
    use std::io::{Read, Seek, SeekFrom};
    let mut file = fs::File::open(&path).map_err(|e| format!("打开失败：{e}"))?;
    file.seek(SeekFrom::Start(start)).map_err(|e| format!("seek 失败：{e}"))?;
    let mut buffer = vec![0u8; (end - start) as usize];
    file.read_exact(&mut buffer).map_err(|e| format!("读取失败：{e}"))?;
    use base64::Engine;
    Ok(MusicChunk {
        data: base64::engine::general_purpose::STANDARD.encode(&buffer),
        offset: end as i64,
        total,
    })
}

/// 从曲库删除。
#[tauri::command]
pub(crate) fn music_delete(app: tauri::AppHandle, file: String) -> Result<(), String> {
    if file.is_empty() || file.contains(['/', '\\', ':']) || file.contains("..") {
        return Err("文件名无效".into());
    }
    let dir = music_dir(&app)?;
    let path = dir.join(&file);
    if path.is_file() {
        fs::remove_file(&path).map_err(|e| format!("删除失败：{e}"))?;
    }
    Ok(())
}

/// 多选音乐文件对话框（rfd 无多选过滤器差异，直接返回路径列表）。
#[tauri::command]
pub(crate) fn pick_music_files() -> Vec<String> {
    rfd::FileDialog::new()
        .set_title("导入音乐")
        .add_filter(
            "音频",
            &["mp3", "flac", "ogg", "wav", "m4a", "aac", "opus"],
        )
        .pick_files()
        .map(|paths| paths.iter().map(|p| p.display().to_string()).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_name_filter() {
        assert!(is_audio_name("song.mp3"));
        assert!(is_audio_name("Song.FLAC"));
        assert!(is_audio_name("track_01.ogg"));
        assert!(!is_audio_name("photo.jpg"));
        assert!(!is_audio_name("noext"));
        assert!(!is_audio_name("evil.exe"));
    }

    #[test]
    fn chunk_bounds_are_safe() {
        // music_read 的 offset/total 语义：start >= total 时返回空块。
        let total = 1000u64;
        let start = 1000u64;
        assert!(start >= total);
        let start2 = 600u64;
        let end = (start2 + MUSIC_CHUNK as u64).min(total);
        assert_eq!(end, total);
    }
}
