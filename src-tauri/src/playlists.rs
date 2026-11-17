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
fn extract_url(input: &str) -> String {
    let pos = match input.find("https://").or_else(|| input.find("http://")) {
        None => return String::new(),
        Some(p) => p,
    };
    let rest = &input[pos..];
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

/// 取查询参数 id：先看常规 query，再看 SPA 风格片段（music.163.com/#/playlist?id=…）。
fn query_id(url: &reqwest::Url) -> String {
    for (k, v) in url.query_pairs() {
        if k == "id" {
            return v.into_owned();
        }
    }
    if let Some(frag) = url.fragment() {
        if let Some(qpos) = frag.find('?') {
            if let Ok(u) = reqwest::Url::parse(&format!("http://q/{}", &frag[qpos..])) {
                for (k, v) in u.query_pairs() {
                    if k == "id" {
                        return v.into_owned();
                    }
                }
            }
        }
    }
    String::new()
}

/// 识别落地页所属平台与歌单 id。163cn.tv 等短链由调用方先跟随重定向再进来。
fn classify(url: &reqwest::Url) -> (String, String) {
    let host = url.host_str().unwrap_or("");
    if host.ends_with("music.163.com") {
        let id = query_id(url);
        if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
            return ("netease".into(), id);
        }
        return (String::new(), String::new());
    }
    if host.ends_with("y.qq.com") {
        let id = query_id(url);
        if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
            return ("qq".into(), id);
        }
        if let Some(mut segs) = url.path_segments() {
            if let Some(last) = segs.next_back() {
                if !last.is_empty() && last.chars().all(|c| c.is_ascii_digit()) {
                    return ("qq".into(), last.to_string());
                }
            }
        }
    }
    (String::new(), String::new())
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

/// 抓取分享链接指向的歌单：接受整段口令文本；短链自动跟随重定向。
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
    // 短链（163cn.tv、c6.y.qq.com/base/fcgi-bin/u?__=…）跟随跳转拿最终地址。
    let resp = client
        .get(parsed)
        .send()
        .await
        .map_err(|e| format!("打开链接失败：{e}"))?;
    let landing = resp.url().clone();
    let (platform, id) = classify(&landing);
    match platform.as_str() {
        "netease" => fetch_netease_playlist(&client, &id).await,
        "qq" => fetch_qq_playlist(&client, &id).await,
        _ => Err("暂只支持网易云音乐与 QQ 音乐的分享链接".into()),
    }
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
        // 没有链接返回空。
        assert_eq!(extract_url("这是一段没有链接的文本"), "");
    }

    #[test]
    fn classification_by_landing_url() {
        let cases = [
            ("https://music.163.com/playlist?id=3778678&userid=1", ("netease", "3778678")),
            ("https://music.163.com/#/playlist?id=9", ("netease", "9")),
            ("https://y.qq.com/n/ryqq/playlist/7527130262", ("qq", "7527130262")),
            ("https://i.y.qq.com/n2/m/share/html/share.html?id=123", ("qq", "123")),
        ];
        for (u, (p, id)) in cases {
            let url = reqwest::Url::parse(u).unwrap();
            let (platform, got) = classify(&url);
            assert_eq!((platform.as_str(), got.as_str()), (p, id), "case {u}");
        }
        // 非歌单页 / 未知平台返回空。
        let url = reqwest::Url::parse("https://music.163.com/song?id=1").unwrap();
        assert_eq!(classify(&url).0, "netease"); // 仍是网易域（单曲链接由接口层报错）
        let url = reqwest::Url::parse("https://example.com/x").unwrap();
        assert_eq!(classify(&url).0, "");
        // QQ id 非数字拒绝。
        let url = reqwest::Url::parse("https://y.qq.com/n/ryqq/playlist/abc").unwrap();
        assert_eq!(classify(&url).0, "");
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
