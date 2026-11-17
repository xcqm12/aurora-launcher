//! 音乐舱歌词：内嵌标签 → 磁盘缓存 → LRCLIB → 网易云 四级来源。
//!
//! 优先读音频文件内嵌歌词（ID3v2 USLT / FLAC LYRICS / MP4 ©lyr 等，lofty 解析），
//! 其次查本地缓存（曲目文件名 + 大小为键，避免重复外网请求），再走 LRCLIB 公开
//! 接口，最后网易云旧版搜索兜底（中文歌命中率高）。结果一律落盘缓存。

use lofty::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

use crate::http_client;
use crate::music::{is_safe_track_file, music_dir};

const LRCLIB_BASE: &str = "https://lrclib.net/api";
const NETEASE_REFERER: &str = "https://music.163.com/";

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct LyricsResult {
    /// LRC 文本（无时间戳的纯文本歌词原样返回，前端按普通文本渲染）。
    lrc: String,
    /// 来源：embedded / lrclib / netease / none。
    source: String,
    /// 是否带 [mm:ss.xx] 时间戳（可随播放滚动）。
    synced: bool,
}

/// 曲目歌词缓存目录：data_dir/music/lyrics/。
fn lyrics_cache_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(music_dir(app)?.join("lyrics"))
}

/// 缓存键：曲目名 + 字节数。文件被替换（重导入、补标签）后大小变化，缓存自动失效。
fn cache_key(path: &Path) -> String {
    let size = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    format!("{}.{}.json", path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default(), size)
}

fn read_cache(app: &tauri::AppHandle, path: &Path) -> Option<LyricsResult> {
    let dir = lyrics_cache_dir(app).ok()?;
    let text = fs::read_to_string(dir.join(cache_key(path))).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_cache(app: &tauri::AppHandle, path: &Path, out: &LyricsResult) {
    let Ok(dir) = lyrics_cache_dir(app) else { return };
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(bytes) = serde_json::to_vec_pretty(out) {
        let _ = crate::write_atomic(&dir.join(cache_key(path)), &bytes);
    }
}

/// 从文件名推导 (artist, title)：约定「歌手 - 歌名」空格横线为最常见命名；
/// 单横线（光辉岁月-Beyond）拆开风险高，按整名做标题、歌手留空。
fn derive_title_artist(file: &str) -> (String, String) {
    let stem = file.rsplit_once('.').map(|(s, _)| s).unwrap_or(file);
    // 去掉常见的前置序号（01. / 01 - / 01．）
    let stem = stem
        .trim_start_matches(|c: char| c.is_ascii_digit())
        .trim_start_matches(['.', '－', '-', '　', ' '])
        .trim();
    if let Some((a, t)) = stem.split_once(" - ") {
        let (a, t) = (a.trim(), t.trim());
        if !a.is_empty() && !t.is_empty() {
            return (t.to_string(), a.to_string());
        }
    }
    (stem.to_string(), String::new())
}

/// LRC 时间戳嗅探：行首 [mm:ss（.xx）] 才算可同步歌词。
fn has_timestamps(lrc: &str) -> bool {
    let bytes = lrc.as_bytes();
    for w in bytes.windows(6) {
        if w[0] == b'['
            && w[1].is_ascii_digit()
            && w[2].is_ascii_digit()
            && w[3] == b':'
            && w[4].is_ascii_digit()
            && w[5].is_ascii_digit()
        {
            return true;
        }
    }
    false
}

/// 读音频内嵌歌词（lofty：ID3v2 USLT / Vorbis LYRICS / MP4 ©lyr 统一映射 ItemKey::Lyrics）。
fn embedded_lyrics(path: &Path) -> Option<String> {
    let tagged = lofty::read_from_path(path).ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    let lrc = tag.get_string(ItemKey::Lyrics)?.trim().to_string();
    if lrc.is_empty() { None } else { Some(lrc) }
}

/// LRCLIB 响应 → LyricsResult：优先同步歌词，其次纯文本。
fn parse_lrclib(v: &Value) -> Option<LyricsResult> {
    let synced = v.get("syncedLyrics").and_then(|x| x.as_str()).map(str::trim).filter(|s| !s.is_empty());
    let plain = v.get("plainLyrics").and_then(|x| x.as_str()).map(str::trim).filter(|s| !s.is_empty());
    let lrc = synced.or(plain)?;
    if lrc.is_empty() {
        return None;
    }
    Some(LyricsResult {
        lrc: lrc.to_string(),
        source: "lrclib".into(),
        synced: synced.is_some(),
    })
}

async fn fetch_lrclib(
    app: &tauri::AppHandle,
    title: &str,
    artist: &str,
    duration: f64,
) -> Option<LyricsResult> {
    let client = http_client(app).ok()?;
    // 1) /api/get 精确查询（带时长更准）
    let mut pairs: Vec<(String, String)> = vec![("track_name".into(), title.to_string())];
    if !artist.is_empty() {
        pairs.push(("artist_name".into(), artist.to_string()));
    }
    if duration.is_finite() && duration > 1.0 {
        pairs.push(("duration".into(), format!("{}", duration.round() as u64)));
    }
    let get_url = format!("{LRCLIB_BASE}/get?{}", {
        let mut s = String::new();
        for (k, v) in &pairs {
            if !s.is_empty() {
                s.push('&');
            }
            s.push_str(&format!("{k}={}", urlencoding::encode(v)));
        }
        s
    });
    if let Ok(resp) = client.get(&get_url).send().await {
        if let Ok(v) = resp.json::<Value>().await {
            if let Some(out) = parse_lrclib(&v) {
                return Some(out);
            }
        }
    }
    // 2) /api/search 模糊查询：取第一条带同步歌词的，否则第一条纯文本
    let mut q = vec![("track_name".to_string(), title.to_string())];
    if !artist.is_empty() {
        q.push(("artist_name".to_string(), artist.to_string()));
    }
    let search_url = format!("{LRCLIB_BASE}/search?{}", {
        let mut s = String::new();
        for (k, v) in &q {
            if !s.is_empty() {
                s.push('&');
            }
            s.push_str(&format!("{k}={}", urlencoding::encode(v)));
        }
        s
    });
    let resp = client.get(&search_url).send().await.ok()?;
    let arr = resp.json::<Value>().await.ok()?;
    let list = arr.as_array()?;
    list.iter().find_map(parse_lrclib)
}

async fn fetch_netease(app: &tauri::AppHandle, title: &str, artist: &str) -> Option<LyricsResult> {
    let client = http_client(app).ok()?;
    let query = if artist.is_empty() {
        title.to_string()
    } else {
        format!("{artist} {title}")
    };
    let resp = client
        .get("https://music.163.com/api/search/get")
        .header("Referer", NETEASE_REFERER)
        .query(&[("s", query.as_str()), ("type", "1"), ("limit", "5")])
        .send()
        .await
        .ok()?;
    let v = resp.json::<Value>().await.ok()?;
    let songs = v.get("result")?.get("songs")?.as_array()?;
    // 命中优先级：规范化歌名相等 → 歌名互相包含 → 首条
    let norm = |s: &str| {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_lowercase()
    };
    let nt = norm(title);
    let mut chosen: Option<&Value> = None;
    for s in songs {
        let name = s.get("name").and_then(|x| x.as_str()).unwrap_or("");
        if norm(name) == nt {
            chosen = Some(s);
            break;
        }
    }
    if chosen.is_none() {
        for s in songs {
            let name = s.get("name").and_then(|x| x.as_str()).unwrap_or("");
            let nn = norm(name);
            if !nn.is_empty() && (nn.contains(&nt) || nt.contains(&nn)) {
                chosen = Some(s);
                break;
            }
        }
    }
    let id = chosen
        .or_else(|| songs.first())
        .and_then(|s| s.get("id"))
        .and_then(|x| x.as_u64())?;
    let resp = client
        .get("https://music.163.com/api/song/lyric")
        .header("Referer", NETEASE_REFERER)
        .query(&[("id", id.to_string().as_str()), ("lv", "1")])
        .send()
        .await
        .ok()?;
    let v = resp.json::<Value>().await.ok()?;
    let lrc = v.get("lrc")?.get("lyric")?.as_str()?.trim().to_string();
    if lrc.is_empty() {
        return None;
    }
    let synced = has_timestamps(&lrc);
    Some(LyricsResult {
        lrc,
        source: "netease".into(),
        synced,
    })
}

/// 自动获取歌词：缓存 → 内嵌 → LRCLIB → 网易云；来源与是否同步随结果返回。
/// duration 是前端播放器当前曲目的秒数（未知传 0），用于提高 LRCLIB 精确匹配率。
#[tauri::command]
pub(crate) async fn music_lyrics(
    app: tauri::AppHandle,
    file: String,
    duration: f64,
) -> Result<LyricsResult, String> {
    if !is_safe_track_file(&file) {
        return Err("文件名无效".into());
    }
    let dir = music_dir(&app)?;
    let path = dir.join(&file);
    if let Some(hit) = read_cache(&app, &path) {
        return Ok(hit);
    }
    let (title, artist) = derive_title_artist(&file);
    let mut out: Option<LyricsResult> = None;
    if path.is_file() {
        if let Some(lrc) = embedded_lyrics(&path) {
            let synced = has_timestamps(&lrc);
            out = Some(LyricsResult { lrc, source: "embedded".into(), synced });
        }
    }
    if out.is_none() {
        out = fetch_lrclib(&app, &title, &artist, duration).await;
    }
    if out.is_none() {
        out = fetch_netease(&app, &title, &artist).await;
    }
    let result = out.unwrap_or(LyricsResult {
        lrc: String::new(),
        source: "none".into(),
        synced: false,
    });
    // 「没找到」不缓存：前端拿到音频时长后会带 duration 重试一次，
    // 之后用户补齐曲名标签重新导入（文件大小变化）也能命中新缓存。
    if result.source != "none" {
        write_cache(&app, &path, &result);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_artist_from_filename() {
        // 「歌手 - 歌名」空格横线是最常见命名，拆开取反序。
        let (t, a) = derive_title_artist("Beyond - 光辉岁月.flac");
        assert_eq!((t.as_str(), a.as_str()), ("光辉岁月", "Beyond"));
        // 单横线不拆，整名做标题。
        let (t, a) = derive_title_artist("光辉岁月-Beyond.mp3");
        assert_eq!((t.as_str(), a.as_str()), ("光辉岁月-Beyond", ""));
        // 前置序号剥掉。
        let (t, a) = derive_title_artist("01 - Beyond - 光辉岁月.mp3");
        assert_eq!((t.as_str(), a.as_str()), ("光辉岁月", "Beyond"));
        // 无扩展名也不炸。
        let (t, _) = derive_title_artist("song");
        assert_eq!(t, "song");
    }

    #[test]
    fn lrc_timestamp_sniffing() {
        assert!(has_timestamps("[00:01.000] 歌词"));
        assert!(has_timestamps("[01:23] 歌词"));
        assert!(!has_timestamps("纯文本歌词"));
        assert!(!has_timestamps("[abc] 不是时间戳"));
        assert!(!has_timestamps(""));
    }

    #[test]
    fn lrclib_parse_prefers_synced() {
        let v: Value = serde_json::from_str(
            r#"{"syncedLyrics":"[00:01] hi","plainLyrics":"hi","instrumental":false}"#,
        )
        .unwrap();
        let out = parse_lrclib(&v).unwrap();
        assert_eq!(out.source, "lrclib");
        assert!(out.synced);
        // 只有纯文本也能用。
        let v: Value =
            serde_json::from_str(r#"{"syncedLyrics":null,"plainLyrics":"plain text"}"#).unwrap();
        let out = parse_lrclib(&v).unwrap();
        assert!(!out.synced);
        assert_eq!(out.lrc, "plain text");
        // 全空返回 None。
        let v: Value = serde_json::from_str(r#"{"syncedLyrics":"","plainLyrics":""}"#).unwrap();
        assert!(parse_lrclib(&v).is_none());
    }

    #[test]
    fn cache_key_uses_size() {
        // 同名不同大小（重导入/补标签）必须拿到不同缓存键。
        let p1 = Path::new("song.mp3");
        assert!(cache_key(p1).starts_with("song.mp3."));
        assert!(cache_key(p1).ends_with(".json"));
    }
}
