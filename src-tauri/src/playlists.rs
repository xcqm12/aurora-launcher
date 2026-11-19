//! 音乐舱歌单：解析其他平台（网易云音乐 / QQ 音乐）分享链接，抓取曲目清单并
//! 收藏到本机。保存曲目元数据（歌名 / 歌手 / 时长 / 平台 id），点曲目即播放：
//! 有本地文件走本地曲库，没有就走平台在线试听流（music_stream_open/chunk）。
//!
//! 在线流按 LRU 缓存在内存（复用 music_read 的分块 base64 协议，前端拼 Blob），
//! QQ 无版权/付费曲自动换网易云同名曲兜底；VIP/下架歌曲明确报错不硬播。
//!
//! 支持整段分享口令文本（自动抠出链接）与短链（跟随重定向取最终地址）。

use base64::Engine;
use reqwest::header::CONTENT_TYPE;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use crate::data_dir;
use crate::http_client;
use crate::music::{MusicChunk, MUSIC_CHUNK};
use crate::write_atomic;

/// 在线流单文件上限（无损几乎拿不到，128k mp3 每首约 4MB）。
const STREAM_MAX_BYTES: usize = 96 * 1024 * 1024;
/// 流缓存上限：总字节 / 条目数，超出按最久未用淘汰。
const STREAM_CACHE_BYTES: usize = 160 * 1024 * 1024;
const STREAM_CACHE_ENTRIES: usize = 16;

const MAX_PLAYLIST_TRACKS: usize = 1000;
const MAX_SAVED_PLAYLISTS: usize = 50;
const NETEASE_REFERER: &str = "https://music.163.com/";
const QQ_REFERER: &str = "https://y.qq.com/";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct PlaylistTrack {
    name: String,
    artist: String,
    duration_sec: u64,
    /// 平台曲目 id（netease 数字 id / qq songmid）。旧版 playlists.json 没有
    /// 此字段，serde default 兼容；为空时在线播放退化为按歌名搜索匹配。
    #[serde(default)]
    id: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct RemotePlaylist {
    /// netease | qq
    platform: String,
    name: String,
    cover: String,
    tracks: Vec<PlaylistTrack>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct SavedPlaylist {
    id: String,
    platform: String,
    name: String,
    cover: String,
    source_url: String,
    created_at: u64,
    tracks: Vec<PlaylistTrack>,
}

fn playlists_file(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(data_dir(app)?.join("music").join("playlists.json"))
}

fn load_playlists(app: &tauri::AppHandle) -> Vec<SavedPlaylist> {
    let Ok(path) = playlists_file(app) else {
        return Vec::new();
    };
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_playlists(app: &tauri::AppHandle, list: &[SavedPlaylist]) -> Result<(), String> {
    let path = playlists_file(app)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let bytes = serde_json::to_vec_pretty(list).map_err(|e| e.to_string())?;
    write_atomic(&path, &bytes)
}

/// 从整段分享文本里抠出第一条 http(s) 链接（中文口令常用「」《》等括号包住）。
/// 先做全角→半角归一化（中文输入法把 https：// 打成全角是常见坑），并剔除零宽字符。
fn extract_url(input: &str) -> String {
    let normalized: String = input
        .chars()
        .filter_map(|c| match c {
            '：' => Some(':'),
            '／' => Some('/'),
            '？' => Some('?'),
            '＆' => Some('&'),
            '＝' => Some('='),
            '％' => Some('%'),
            '＃' => Some('#'),
            '\u{200b}' | '\u{200c}' | '\u{200d}' | '\u{feff}' => None,
            _ => Some(c),
        })
        .collect();
    let pos = match normalized.find("https://").or_else(|| normalized.find("http://")) {
        None => return String::new(),
        Some(p) => p,
    };
    let rest = &normalized[pos..];
    let end = rest
        .find(|c: char| {
            c.is_whitespace()
                || matches!(
                    c,
                    '「' | '」' | '『' | '』' | '《' | '》' | '（' | '）' | '(' | ')' | '[' | ']'
                        | '<' | '>' | '"' | '\'' | '，' | '。' | '！' | '；'
                )
        })
        .unwrap_or(rest.len());
    rest[..end].to_string()
}

/// 链接指向的资源类型。
#[derive(Debug, PartialEq)]
enum LinkKind {
    /// 歌单（id 为数字）。
    Playlist(String),
    /// 单曲（netease 数字 id / qq songmid 字母数字）。
    Song(String),
    /// 短链/中转页：需跟随重定向后再分类（163cn.tv、c6.y.qq.com/base/fcgi-bin/u）。
    Follow,
    /// 识别到平台但没有 id。
    NoId,
    /// 不是已支持的平台。
    Unknown,
}

/// 平台中文名（错误提示用）。
fn platform_label(platform: &str) -> &'static str {
    match platform {
        "netease" => "网易云音乐",
        "qq" => "QQ 音乐",
        _ => "未知平台",
    }
}

/// 查询参数（含 SPA 片段 query）取值。
fn query_value(url: &reqwest::Url, key: &str) -> Option<String> {
    for (k, v) in url.query_pairs() {
        if k == key {
            return Some(v.into_owned());
        }
    }
    if let Some(frag) = url.fragment() {
        if let Some(qpos) = frag.find('?') {
            if let Ok(u) = reqwest::Url::parse(&format!("http://q/{}", &frag[qpos..])) {
                for (k, v) in u.query_pairs() {
                    if k == key {
                        return Some(v.into_owned());
                    }
                }
            }
        }
    }
    None
}

/// path 与片段路径拼合（music.163.com/#/playlist 的关键词藏在片段里）。
fn path_with_fragment(url: &reqwest::Url) -> String {
    let mut s = url.path().to_string();
    if let Some(frag) = url.fragment() {
        s.push('/');
        s.push_str(frag.split('?').next().unwrap_or(""));
    }
    s
}

/// 识别链接所属平台与资源。注意：`music.163.com/#/…?id=` 这类 SPA 片段里的
/// id 不会随 HTTP 请求发出（片段永不发往服务器），调用方必须先用原链接分类，
/// 只有 Follow（短链）与 Unknown 才跟随重定向。
fn classify(url: &reqwest::Url) -> (String, LinkKind) {
    let host = url.host_str().unwrap_or("");
    if host == "163cn.tv" {
        return ("netease".into(), LinkKind::Follow);
    }
    if host.ends_with("music.163.com") {
        // 单曲优先（song?id= 与 playlist?id= 都用 id 参数，靠路径区分）。
        if path_with_fragment(url).contains("song") {
            if let Some(id) = query_value(url, "id").filter(|v| !v.is_empty()) {
                return ("netease".into(), LinkKind::Song(id));
            }
            return ("netease".into(), LinkKind::NoId);
        }
        if let Some(id) = query_value(url, "id").filter(|v| !v.is_empty()) {
            return ("netease".into(), LinkKind::Playlist(id));
        }
        return ("netease".into(), LinkKind::NoId);
    }
    if host.ends_with("qq.com") {
        // QQ 短链中转端点（c6.y.qq.com/base/fcgi-bin/u?__=…）：跟随重定向。
        if url.path().contains("fcgi-bin/u") {
            return ("qq".into(), LinkKind::Follow);
        }
        // 单曲：query songmid/mid 优先，其次 songDetail/…/&lt;mid&gt; 路径段。
        for key in ["songmid", "mid"] {
            if let Some(v) = query_value(url, key).filter(|v| !v.is_empty()) {
                return ("qq".into(), LinkKind::Song(v));
            }
        }
        let combined = path_with_fragment(url);
        if combined.contains("song") {
            if let Some(mut segs) = url.path_segments() {
                if let Some(last) = segs.next_back() {
                    if !last.is_empty() {
                        return ("qq".into(), LinkKind::Song(last.to_string()));
                    }
                }
            }
            return ("qq".into(), LinkKind::NoId);
        }
        if let Some(id) = query_value(url, "id").filter(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit())) {
            return ("qq".into(), LinkKind::Playlist(id));
        }
        if let Some(mut segs) = url.path_segments() {
            if let Some(last) = segs.next_back() {
                if !last.is_empty() && last.chars().all(|c| c.is_ascii_digit()) {
                    return ("qq".into(), LinkKind::Playlist(last.to_string()));
                }
            }
        }
        return ("qq".into(), LinkKind::NoId);
    }
    (String::new(), LinkKind::Unknown)
}

fn netease_track(t: &Value) -> Option<PlaylistTrack> {
    let name = t.get("name")?.as_str()?.trim().to_string();
    if name.is_empty() {
        return None;
    }
    let artist = t
        .get("ar")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.get("name").and_then(|n| n.as_str()))
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    let dt = t.get("dt").and_then(|x| x.as_u64()).unwrap_or(0);
    let id = t
        .get("id")
        .and_then(|x| x.as_u64())
        .map(|i| i.to_string())
        .unwrap_or_default();
    Some(PlaylistTrack { name, artist, duration_sec: dt / 1000, id })
}

async fn fetch_netease_playlist(client: &reqwest::Client, id: &str) -> Result<RemotePlaylist, String> {
    let resp = client
        .get("https://music.163.com/api/v6/playlist/detail")
        .header("Referer", NETEASE_REFERER)
        .query(&[("id", id), ("n", MAX_PLAYLIST_TRACKS.to_string().as_str())])
        .send()
        .await
        .map_err(|e| format!("请求网易云音乐失败：{e}"))?;
    let v: Value = resp.json().await.map_err(|e| format!("解析歌单数据失败：{e}"))?;
    let pl = v
        .get("playlist")
        .ok_or_else(|| "歌单不存在或已删除（请确认分享的是歌单而非单曲）".to_string())?;
    let name = pl.get("name").and_then(|x| x.as_str()).unwrap_or("未命名歌单").to_string();
    let cover = pl.get("coverImgUrl").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let mut tracks = Vec::new();
    if let Some(arr) = pl.get("tracks").and_then(|x| x.as_array()) {
        for t in arr {
            if let Some(tk) = netease_track(t) {
                tracks.push(tk);
            }
        }
    }
    // trackIds 多于已取曲目（接口单次上限）时用 v3 song/detail 分批补齐。
    if let Some(ids) = pl.get("trackIds").and_then(|x| x.as_array()) {
        let all: Vec<u64> = ids
            .iter()
            .filter_map(|x| x.get("id").and_then(|i| i.as_u64()))
            .collect();
        if all.len() > tracks.len() {
            for chunk in all[tracks.len()..].chunks(500) {
                let c_json = format!(
                    "[{}]",
                    chunk
                        .iter()
                        .map(|i| format!(r#"{{"id":{i}}}"#))
                        .collect::<Vec<_>>()
                        .join(",")
                );
                let Ok(resp) = client
                    .get("https://music.163.com/api/v3/song/detail")
                    .header("Referer", NETEASE_REFERER)
                    .query(&[("c", c_json.as_str())])
                    .send()
                    .await
                else {
                    break;
                };
                let Ok(v) = resp.json::<Value>().await else { break };
                if let Some(arr) = v.get("songs").and_then(|x| x.as_array()) {
                    for t in arr {
                        if let Some(tk) = netease_track(t) {
                            tracks.push(tk);
                        }
                    }
                }
            }
        }
    }
    tracks.truncate(MAX_PLAYLIST_TRACKS);
    Ok(RemotePlaylist { platform: "netease".into(), name, cover, tracks })
}

fn qq_track(s: &Value) -> Option<PlaylistTrack> {
    let name = s
        .get("name")
        .or_else(|| s.get("title"))
        .and_then(|x| x.as_str())?
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }
    let artist = s
        .get("singer")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a.get("name").and_then(|n| n.as_str()))
                .collect::<Vec<_>>()
                .join("/")
        })
        .unwrap_or_default();
    let dur = s.get("interval").and_then(|x| x.as_u64()).unwrap_or(0);
    // songlist（new_format=1）里是 mid，单曲接口里是 songmid / id。
    let id = ["mid", "songmid"]
        .iter()
        .find_map(|k| {
            s.get(*k)
                .and_then(|x| x.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty())
        })
        .unwrap_or_default()
        .to_string();
    Some(PlaylistTrack { name, artist, duration_sec: dur, id })
}

async fn fetch_qq_playlist(client: &reqwest::Client, id: &str) -> Result<RemotePlaylist, String> {
    let resp = client
        .get("https://c.y.qq.com/qzone/fcg-bin/fcg_ucc_getcdinfo_byids_cp.fcg")
        .header("Referer", QQ_REFERER)
        .query(&[
            ("type", "1"),
            ("json", "1"),
            ("utf8", "1"),
            ("onlysong", "0"),
            ("new_format", "1"),
            ("disstid", id),
            ("format", "json"),
        ])
        .send()
        .await
        .map_err(|e| format!("请求 QQ 音乐失败：{e}"))?;
    let v: Value = resp.json().await.map_err(|e| format!("解析歌单数据失败：{e}"))?;
    let cd = v
        .get("cdlist")
        .and_then(|x| x.as_array())
        .and_then(|a| a.first())
        .ok_or_else(|| "歌单不存在或已删除（请确认分享的是歌单而非单曲）".to_string())?;
    let name = cd.get("dissname").and_then(|x| x.as_str()).unwrap_or("未命名歌单").to_string();
    let cover = cd.get("logo").and_then(|x| x.as_str()).unwrap_or("").to_string();
    let mut tracks = Vec::new();
    if let Some(arr) = cd.get("songlist").and_then(|x| x.as_array()) {
        for s in arr {
            if let Some(tk) = qq_track(s) {
                tracks.push(tk);
            }
        }
    }
    tracks.truncate(MAX_PLAYLIST_TRACKS);
    Ok(RemotePlaylist { platform: "qq".into(), name, cover, tracks })
}

/// 抓取分享链接指向的歌单或单曲：接受整段口令文本；短链自动跟随重定向。
#[tauri::command]
pub(crate) async fn music_playlist_fetch(
    app: tauri::AppHandle,
    url: String,
) -> Result<RemotePlaylist, String> {
    let raw = extract_url(&url);
    if raw.is_empty() {
        return Err("未在输入中找到分享链接".into());
    }
    let parsed = reqwest::Url::parse(&raw).map_err(|_| "链接格式无效".to_string())?;
    let client = http_client(&app)?;
    // 先用原链接本身分类：#/…?id= 片段里的 id 永远不会发给服务器，若直接
    // GET 再看 resp.url 会把整段片段丢掉（my/m/music/playlist 链接抓不到的根因）。
    // 只有短链（Follow）与未知域才跟随重定向取落地页。
    let first = classify(&parsed);
    let (platform, kind) = match first.1 {
        LinkKind::Follow => {
            let resp = client
                .get(parsed)
                .send()
                .await
                .map_err(|e| format!("打开链接失败：{e}"))?;
            classify(&resp.url().clone())
        }
        _ => first,
    };
    match (platform.as_str(), kind) {
        ("netease", LinkKind::Playlist(id)) => fetch_netease_playlist(&client, &id).await,
        ("netease", LinkKind::Song(id)) => fetch_netease_song(&client, &id).await,
        ("qq", LinkKind::Playlist(id)) => fetch_qq_playlist(&client, &id).await,
        ("qq", LinkKind::Song(mid)) => fetch_qq_song(&client, &mid).await,
        (_, LinkKind::NoId) => Err(format!(
            "识别到{}链接，但没有找到歌单/单曲 id——请从 App 重新复制完整分享链接",
            platform_label(&platform)
        )),
        _ => Err("暂只支持网易云音乐与 QQ 音乐的分享链接（酷狗/酷我等平台未开放公开接口）".into()),
    }
}

/// 网易云单曲 → 单曲目「歌单」。
async fn fetch_netease_song(client: &reqwest::Client, id: &str) -> Result<RemotePlaylist, String> {
    let resp = client
        .get("https://music.163.com/api/song/detail/")
        .header("Referer", NETEASE_REFERER)
        .query(&[("id", id), ("ids", &format!("[{id}]"))])
        .send()
        .await
        .map_err(|e| format!("请求网易云音乐失败：{e}"))?;
    let v: Value = resp.json().await.map_err(|e| format!("解析歌曲数据失败：{e}"))?;
    let song = v
        .get("songs")
        .and_then(|x| x.as_array())
        .and_then(|a| a.first())
        .ok_or_else(|| "歌曲不存在或已下架".to_string())?;
    let track = netease_track(song).ok_or_else(|| "歌曲数据不完整".to_string())?;
    let name = track.name.clone();
    let cover = song
        .get("album")
        .and_then(|x| x.get("picUrl"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    Ok(RemotePlaylist {
        platform: "netease".into(),
        name: format!("单曲 · {name}"),
        cover,
        tracks: vec![track],
    })
}

/// QQ 音乐单曲（songmid）→ 单曲目「歌单」。
async fn fetch_qq_song(client: &reqwest::Client, mid: &str) -> Result<RemotePlaylist, String> {
    let resp = client
        .get("https://c.y.qq.com/v8/fcg-bin/fcg_play_single_song.fcg")
        .header("Referer", QQ_REFERER)
        .query(&[("songmid", mid), ("format", "json")])
        .send()
        .await
        .map_err(|e| format!("请求 QQ 音乐失败：{e}"))?;
    let v: Value = resp.json().await.map_err(|e| format!("解析歌曲数据失败：{e}"))?;
    let song = v
        .get("data")
        .and_then(|x| x.as_array())
        .and_then(|a| a.first())
        .ok_or_else(|| "歌曲不存在或已下架".to_string())?;
    let track = qq_track(song).ok_or_else(|| "歌曲数据不完整".to_string())?;
    let name = track.name.clone();
    let cover = song
        .get("album")
        .and_then(|x| x.get("mid"))
        .and_then(|x| x.as_str())
        .map(|album_mid| format!("https://y.gtimg.cn/music/photo_new/T002R500x500M000{album_mid}.jpg"))
        .unwrap_or_default();
    Ok(RemotePlaylist {
        platform: "qq".into(),
        name: format!("单曲 · {name}"),
        cover,
        tracks: vec![track],
    })
}

/// 收藏歌单：同平台同名覆盖更新，否则新建（上限 50 个）。
#[tauri::command]
pub(crate) fn music_playlist_save(
    app: tauri::AppHandle,
    playlist: RemotePlaylist,
    source_url: String,
) -> Result<SavedPlaylist, String> {
    let mut list = load_playlists(&app);
    let created = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Some(existing) = list
        .iter_mut()
        .find(|p| p.platform == playlist.platform && p.name == playlist.name)
    {
        existing.cover = playlist.cover.clone();
        existing.tracks = playlist.tracks.clone();
        existing.source_url = source_url;
        let id = existing.id.clone();
        save_playlists(&app, &list)?;
        return Ok(list.into_iter().find(|p| p.id == id).expect("刚更新过"));
    }
    if list.len() >= MAX_SAVED_PLAYLISTS {
        return Err(format!("收藏的歌单最多 {} 个，请先删除一些", MAX_SAVED_PLAYLISTS));
    }
    let id = format!("pl{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0));
    let saved = SavedPlaylist {
        id: id.clone(),
        platform: playlist.platform.clone(),
        name: playlist.name.clone(),
        cover: playlist.cover.clone(),
        source_url,
        created_at: created,
        tracks: playlist.tracks.clone(),
    };
    list.push(saved);
    save_playlists(&app, &list)?;
    Ok(list.into_iter().find(|p| p.id == id).expect("刚插入"))
}

/// 列出收藏的歌单。
#[tauri::command]
pub(crate) fn music_playlists_list(app: tauri::AppHandle) -> Vec<SavedPlaylist> {
    load_playlists(&app)
}

/// 删除收藏的歌单。
#[tauri::command]
pub(crate) fn music_playlist_delete(app: tauri::AppHandle, id: String) -> Result<(), String> {
    let mut list = load_playlists(&app);
    list.retain(|p| p.id != id);
    save_playlists(&app, &list)
}

/* ── 在线播放：解析平台试听流 → 内存 LRU 缓存 → 分块 base64 回传 ───────── */

/// 已缓存的在线流。`used` 供 LRU 淘汰排序。
struct CachedStream {
    mime: String,
    bytes: Vec<u8>,
    used: Instant,
}

fn stream_cache() -> &'static Mutex<HashMap<String, CachedStream>> {
    static CACHE: OnceLock<Mutex<HashMap<String, CachedStream>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// 缓存键：有平台 id 用「platform:id」；旧收藏歌单没有 id 时退化用歌名+歌手哈希，
/// 保证同一首曲重复点播/自动续播命中同一份缓存。
fn stream_key(platform: &str, id: &str, name: &str, artist: &str) -> String {
    if !id.is_empty() {
        return format!("{platform}:{id}");
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    name.hash(&mut h);
    artist.hash(&mut h);
    format!("{platform}:n{:016x}", h.finish())
}

#[derive(Debug, Serialize)]
pub(crate) struct StreamMeta {
    /// 前端后续 music_stream_chunk 取数据的句柄。
    stream_id: String,
    /// Blob MIME（audio/mpeg、audio/mp4 等）。
    mime: String,
    total: i64,
}

/// 从响应头拿 MIME，非音频类型时用平台默认值。
fn response_mime(headers: &reqwest::header::HeaderMap, fallback: &str) -> String {
    let ct = headers
        .get(CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    let base = ct.split(';').next().unwrap_or("").trim().to_lowercase();
    if base.starts_with("audio")
        || base == "application/ogg"
        || base == "application/octet-stream"
    {
        return base;
    }
    fallback.to_string()
}

/// 网易云直链：enhance/player/url 匿名接口（免费曲返回 CDN mp3，VIP/无版权 url 为空）。
async fn netease_api_url(client: &reqwest::Client, id: &str) -> Option<String> {
    let resp = client
        .get("https://music.163.com/api/song/enhance/player/url")
        .header("Referer", NETEASE_REFERER)
        .query(&[
            ("ids".to_string(), format!("[{id}]")),
            ("br".to_string(), "320000".to_string()),
        ])
        .send()
        .await
        .ok()?;
    let v = resp.json::<Value>().await.ok()?;
    let url = v
        .get("data")?
        .as_array()?
        .first()?
        .get("url")?
        .as_str()?
        .trim()
        .to_string();
    if url.is_empty() { None } else { Some(url) }
}

/// 网易云 outer/url 兜底：跟随重定向，只有落地是音频才算数
/// （VIP/下架会 302 到 404 页或版权提示，content-type 是 text/html）。
async fn netease_outer_bytes(client: &reqwest::Client, id: &str) -> Option<(Vec<u8>, String)> {
    let resp = client
        .get(format!("https://music.163.com/song/media/outer/url?id={id}.mp3"))
        .header("Referer", NETEASE_REFERER)
        .send()
        .await
        .ok()?;
    let mime = response_mime(resp.headers(), "");
    if mime.is_empty() {
        return None;
    }
    let bytes = resp.bytes().await.ok()?.to_vec();
    if bytes.is_empty() { None } else { Some((bytes, mime)) }
}

/// QQ vkey 匿名接口：purl 为空即无版权/付费。
async fn qq_vkey_url(client: &reqwest::Client, mid: &str) -> Option<String> {
    let body = serde_json::json!({
        "req_0": {
            "module": "vkey.GetVkey",
            "method": "CgiGetVkey",
            "param": {
                "guid": "7248189525",
                "songmid": [mid],
                "songtype": [0],
                "uin": "0",
                "loginflag": 1,
                "platform": "20",
            },
        }
    });
    let resp = client
        .post("https://u.y.qq.com/cgi-bin/musicu.fcg")
        .header("Referer", QQ_REFERER)
        .json(&body)
        .send()
        .await
        .ok()?;
    let v = resp.json::<Value>().await.ok()?;
    let data = v.get("req_0")?.get("data")?;
    let purl = data
        .get("midurlinfo")?
        .as_array()?
        .first()?
        .get("purl")?
        .as_str()?
        .trim()
        .to_string();
    if purl.is_empty() {
        return None;
    }
    let sip = data
        .get("sip")
        .and_then(|s| s.as_array())
        .and_then(|a| a.first())
        .and_then(|x| x.as_str())
        .unwrap_or("http://dl.stream.qqmusic.qq.com/")
        .trim_end_matches('/')
        .to_string();
    if sip.is_empty() {
        Some(format!("http://dl.stream.qqmusic.qq.com/{purl}"))
    } else {
        Some(format!("{sip}/{purl}"))
    }
}

/// 网易云按「歌手 歌名」搜索匹配曲目 id（换源兜底 / 无 id 旧歌单）。
/// 匹配从紧到宽：名称相等 + 时长 ±8s → 名称互相包含 + 时长 ±8s。
async fn netease_search_track(
    client: &reqwest::Client,
    name: &str,
    artist: &str,
    dur: f64,
) -> Option<u64> {
    if name.is_empty() {
        return None;
    }
    let query = if artist.is_empty() { name.to_string() } else { format!("{artist} {name}") };
    let resp = client
        .get("https://music.163.com/api/search/get")
        .header("Referer", NETEASE_REFERER)
        .query(&[("s", query.as_str()), ("type", "1"), ("limit", "8")])
        .send()
        .await
        .ok()?;
    let v = resp.json::<Value>().await.ok()?;
    let songs = v.get("result")?.get("songs")?.as_array()?;
    let norm = |s: &str| {
        s.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
            .to_lowercase()
    };
    let nt = norm(name);
    // 时长差容差：平台时长口径不一，±8s 内才算同曲。
    let dur_ok = |s: &Value| {
        if dur <= 0.0 {
            return true;
        }
        let sd = s.get("duration").and_then(|x| x.as_u64()).unwrap_or(0) as f64 / 1000.0;
        sd <= 0.0 || (sd - dur).abs() <= 8.0
    };
    let mut chosen: Option<&Value> = None;
    for strict in [true, false] {
        for s in songs {
            let n = s.get("name").and_then(|x| x.as_str()).unwrap_or("");
            let nn = norm(n);
            let name_ok = if strict {
                nn == nt
            } else {
                !nn.is_empty() && (nn.contains(&nt) || nt.contains(&nn))
            };
            if name_ok && dur_ok(s) {
                chosen = Some(s);
                break;
            }
        }
        if chosen.is_some() {
            break;
        }
    }
    chosen.and_then(|s| s.get("id")).and_then(|x| x.as_u64())
}

/// 网易云整链路：无 id 先按名搜索；API 直链失败再走 outer/url 探测。
async fn resolve_netease(
    client: &reqwest::Client,
    id: &str,
    name: &str,
    artist: &str,
    dur: f64,
) -> Result<(String, Vec<u8>), String> {
    let sid = if id.is_empty() {
        netease_search_track(client, name, artist, dur)
            .await
            .map(|i| i.to_string())
            .ok_or_else(|| "在网易云音乐搜不到这首歌，无法在线播放".to_string())?
    } else {
        id.to_string()
    };
    if let Some(url) = netease_api_url(client, &sid).await {
        if let Ok(resp) = client.get(&url).header("Referer", NETEASE_REFERER).send().await {
            let mime = response_mime(resp.headers(), "audio/mpeg");
            if let Ok(bytes) = resp.bytes().await {
                if !bytes.is_empty() {
                    return Ok((mime, bytes.to_vec()));
                }
            }
        }
    }
    if let Some((bytes, mime)) = netease_outer_bytes(client, &sid).await {
        return Ok((mime, bytes));
    }
    Err("该歌曲需 VIP / 无版权或已下架，无法在线播放".into())
}

/// QQ 整链路：vkey 拿不到（付费/无版权）或下载失败 → 换网易云同名曲兜底。
async fn resolve_qq(
    client: &reqwest::Client,
    mid: &str,
    name: &str,
    artist: &str,
    dur: f64,
) -> Result<(String, Vec<u8>), String> {
    if !mid.is_empty() {
        if let Some(url) = qq_vkey_url(client, mid).await {
            if let Ok(resp) = client.get(&url).header("Referer", QQ_REFERER).send().await {
                // purl 多为 .m4a（aac），次为 .mp3。
                let fallback = if url.contains(".mp3") { "audio/mpeg" } else { "audio/mp4" };
                let mime = response_mime(resp.headers(), fallback);
                if let Ok(bytes) = resp.bytes().await {
                    if !bytes.is_empty() {
                        return Ok((mime, bytes.to_vec()));
                    }
                }
            }
        }
    }
    let nid = netease_search_track(client, name, artist, dur)
        .await
        .map(|i| i.to_string())
        .ok_or_else(|| "QQ 音乐该曲目需付费或无版权，换源也未找到可播放版本".to_string())?;
    resolve_netease(client, &nid, name, artist, dur).await
}

/// 打开在线试听流：解析播放地址、下载缓存（LRU），返回分块读取句柄。
#[tauri::command]
pub(crate) async fn music_stream_open(
    app: tauri::AppHandle,
    platform: String,
    id: String,
    name: String,
    artist: String,
    duration_sec: f64,
) -> Result<StreamMeta, String> {
    let key = stream_key(&platform, &id, &name, &artist);
    {
        let mut cache = stream_cache().lock().expect("流缓存锁");
        if let Some(s) = cache.get_mut(&key) {
            s.used = Instant::now();
            return Ok(StreamMeta {
                stream_id: key,
                mime: s.mime.clone(),
                total: s.bytes.len() as i64,
            });
        }
    }
    let client = http_client(&app)?;
    let id = id.trim().to_string();
    let (mime, bytes) = match platform.as_str() {
        "netease" => resolve_netease(&client, &id, &name, &artist, duration_sec).await?,
        "qq" => resolve_qq(&client, &id, &name, &artist, duration_sec).await?,
        _ => return Err("不支持的平台".into()),
    };
    if bytes.is_empty() {
        return Err("音频流为空，无法播放".into());
    }
    if bytes.len() > STREAM_MAX_BYTES {
        return Err("音频文件过大，已取消在线播放".into());
    }
    let total = bytes.len() as i64;
    {
        let mut cache = stream_cache().lock().expect("流缓存锁");
        cache.insert(key.clone(), CachedStream { mime: mime.clone(), bytes, used: Instant::now() });
        loop {
            let total_bytes: usize = cache.values().map(|s| s.bytes.len()).sum();
            if cache.len() <= STREAM_CACHE_ENTRIES && total_bytes <= STREAM_CACHE_BYTES {
                break;
            }
            let oldest = cache
                .iter()
                .filter(|(k, _)| k.as_str() != key)
                .min_by_key(|(_, s)| s.used)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => {
                    cache.remove(&k);
                }
                None => break,
            }
        }
    }
    Ok(StreamMeta { stream_id: key, mime, total })
}

/// 分块读取在线流字节（与 music_read 同协议：base64 / offset / total）。
#[tauri::command]
pub(crate) fn music_stream_chunk(stream_id: String, offset: i64) -> Result<MusicChunk, String> {
    let mut cache = stream_cache().lock().expect("流缓存锁");
    let s = cache
        .get_mut(&stream_id)
        .ok_or_else(|| "在线流缓存已失效，请重新点播".to_string())?;
    s.used = Instant::now();
    let total = s.bytes.len();
    let start = (if offset < 0 { 0usize } else { offset as usize }).min(total);
    if start >= total {
        return Ok(MusicChunk::new(String::new(), offset, total as u64));
    }
    let end = (start + MUSIC_CHUNK as usize).min(total);
    Ok(MusicChunk::new(
        base64::engine::general_purpose::STANDARD.encode(&s.bytes[start..end]),
        end as i64,
        total as u64,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_extraction_from_share_text() {
        // 整段口令 + 中文括号包裹。
        assert_eq!(
            extract_url("【网易云音乐】分享歌单《热歌》: 「https://music.163.com/playlist?id=3778678」快来听听"),
            "https://music.163.com/playlist?id=3778678"
        );
        // 纯链接原样保留（含查询串）。
        assert_eq!(
            extract_url("https://y.qq.com/n/ryqq/playlist/7527130262"),
            "https://y.qq.com/n/ryqq/playlist/7527130262"
        );
        // 全角冒号（中文输入法）自动归一化。
        assert_eq!(
            extract_url("分享：https：//music.163.com/playlist?id=9 快听"),
            "https://music.163.com/playlist?id=9"
        );
        // 零宽字符剔除。
        assert_eq!(
            extract_url("https://music.163.com/playlist\u{200b}?id=8"),
            "https://music.163.com/playlist?id=8"
        );
        // 没有链接返回空。
        assert_eq!(extract_url("这是一段没有链接的文本"), "");
    }

    #[test]
    fn classification_by_landing_url() {
        let cases = [
            ("https://music.163.com/playlist?id=3778678&userid=1", ("netease", "Playlist", "3778678")),
            ("https://music.163.com/#/playlist?id=9", ("netease", "Playlist", "9")),
            // App 分享出的移动端路由：id 藏在 # 片段里（回归：曾被重定向跟随丢掉）。
            (
                "https://music.163.com/#/my/m/music/playlist?id=18264649445",
                ("netease", "Playlist", "18264649445"),
            ),
            ("https://music.163.com/song?id=346576", ("netease", "Song", "346576")),
            ("https://music.163.com/#/song?id=346576", ("netease", "Song", "346576")),
            ("https://y.qq.com/n/ryqq/playlist/7527130262", ("qq", "Playlist", "7527130262")),
            ("https://i.y.qq.com/n2/m/share/html/share.html?id=123", ("qq", "Playlist", "123")),
            (
                "https://y.qq.com/n/ryqq/songDetail/003WjRGk1b3FPN",
                ("qq", "Song", "003WjRGk1b3FPN"),
            ),
            (
                "https://i.y.qq.com/n2/m/share/html/share.html?songmid=003WjRGk1b3FPN&fmt=1",
                ("qq", "Song", "003WjRGk1b3FPN"),
            ),
        ];
        for (u, (p, kind, id)) in cases {
            let url = reqwest::Url::parse(u).unwrap();
            let (platform, got) = classify(&url);
            let expected = if kind == "Song" {
                LinkKind::Song(id.to_string())
            } else {
                LinkKind::Playlist(id.to_string())
            };
            assert_eq!(platform.as_str(), p, "case {u}");
            assert_eq!(got, expected, "case {u}");
        }
        // 识别到平台但无 id：NoId。
        let url = reqwest::Url::parse("https://music.163.com/").unwrap();
        assert_eq!(classify(&url).1, LinkKind::NoId);
        // 短链/中转：Follow（跟随重定向后再分类）。
        let url = reqwest::Url::parse("https://163cn.tv/AbCdEf").unwrap();
        assert_eq!(classify(&url).1, LinkKind::Follow);
        let url = reqwest::Url::parse("https://c6.y.qq.com/base/fcgi-bin/u?__=xyz").unwrap();
        assert_eq!(classify(&url).1, LinkKind::Follow);
        // QQ id 非数字且非单曲路径：NoId。
        let url = reqwest::Url::parse("https://y.qq.com/n/ryqq/playlist/abc").unwrap();
        assert_eq!(classify(&url).1, LinkKind::NoId);
        // 未知平台。
        let url = reqwest::Url::parse("https://example.com/x").unwrap();
        assert_eq!(classify(&url).1, LinkKind::Unknown);
    }

    #[test]
    fn netease_track_parse() {
        let t: Value = serde_json::from_str(
            r#"{"id":347230,"name":"光辉岁月","ar":[{"id":1,"name":"Beyond"}],"dt":298560}"#,
        )
        .unwrap();
        let tk = netease_track(&t).unwrap();
        assert_eq!(tk.name, "光辉岁月");
        assert_eq!(tk.artist, "Beyond");
        assert_eq!(tk.duration_sec, 298);
        assert_eq!(tk.id, "347230");
        // 缺字段拒绝。
        let bad: Value = serde_json::from_str(r#"{"ar":[]}"#).unwrap();
        assert!(netease_track(&bad).is_none());
    }

    #[test]
    fn qq_track_parse() {
        let s: Value = serde_json::from_str(
            r#"{"mid":"003WjRGk1b3FPN","name":"侧脸","singer":[{"name":"于果"}],"interval":217}"#,
        )
        .unwrap();
        let tk = qq_track(&s).unwrap();
        assert_eq!(tk.name, "侧脸");
        assert_eq!(tk.artist, "于果");
        assert_eq!(tk.duration_sec, 217);
        assert_eq!(tk.id, "003WjRGk1b3FPN");
        // songmid 字段名（旧接口形态）也能取到。
        let s2: Value = serde_json::from_str(
            r#"{"songmid":"003abc","name":"x","singer":[],"interval":0}"#,
        )
        .unwrap();
        assert_eq!(qq_track(&s2).unwrap().id, "003abc");
    }

    #[test]
    fn playlist_track_id_backcompat() {
        // 旧版 playlists.json 的曲目没有 id 字段，反序列化必须兼容。
        let t: PlaylistTrack =
            serde_json::from_str(r#"{"name":"侧脸","artist":"于果","duration_sec":217}"#).unwrap();
        assert_eq!(t.id, "");
    }

    #[test]
    fn stream_key_stable_without_id() {
        // 有 id 直接用；无 id 用歌名+歌手哈希，同一曲重复点播命中同一缓存。
        assert_eq!(stream_key("netease", "347230", "x", "y"), "netease:347230");
        let k1 = stream_key("netease", "", "光辉岁月", "Beyond");
        let k2 = stream_key("netease", "", "光辉岁月", "Beyond");
        assert_eq!(k1, k2);
        assert_ne!(k1, stream_key("netease", "", "光辉岁月", "Someone Else"));
        assert!(k1.starts_with("netease:n"));
    }
}
