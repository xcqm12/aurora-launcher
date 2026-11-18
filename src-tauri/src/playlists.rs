//! 音乐舱歌单：解析其他平台（网易云音乐 / QQ 音乐）分享链接，抓取曲目清单并
//! 收藏到本机。只保存曲目元数据（歌名 / 歌手 / 时长），不抓音频流——播放仍走
//! 本地曲库，前端用歌单做「本地匹配 / 缺歌清单」。
//!
//! 支持整段分享口令文本（自动抠出链接）与短链（跟随重定向取最终地址）。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

use crate::data_dir;
use crate::http_client;
use crate::write_atomic;

const MAX_PLAYLIST_TRACKS: usize = 1000;
const MAX_SAVED_PLAYLISTS: usize = 50;
const NETEASE_REFERER: &str = "https://music.163.com/";
const QQ_REFERER: &str = "https://y.qq.com/";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(crate) struct PlaylistTrack {
    name: String,
    artist: String,
    duration_sec: u64,
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
    Some(PlaylistTrack { name, artist, duration_sec: dt / 1000 })
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
    Some(PlaylistTrack { name, artist, duration_sec: dur })
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
            r#"{"name":"光辉岁月","ar":[{"id":1,"name":"Beyond"}],"dt":298560}"#,
        )
        .unwrap();
        let tk = netease_track(&t).unwrap();
        assert_eq!(tk.name, "光辉岁月");
        assert_eq!(tk.artist, "Beyond");
        assert_eq!(tk.duration_sec, 298);
        // 缺字段拒绝。
        let bad: Value = serde_json::from_str(r#"{"ar":[]}"#).unwrap();
        assert!(netease_track(&bad).is_none());
    }

    #[test]
    fn qq_track_parse() {
        let s: Value = serde_json::from_str(
            r#"{"name":"侧脸","singer":[{"name":"于果"}],"interval":217}"#,
        )
        .unwrap();
        let tk = qq_track(&s).unwrap();
        assert_eq!(tk.name, "侧脸");
        assert_eq!(tk.artist, "于果");
        assert_eq!(tk.duration_sec, 217);
    }
}
