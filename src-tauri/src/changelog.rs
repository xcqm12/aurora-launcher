//! 更新日志：把仓库根目录的 RELEASE_NOTES.md 在编译期内嵌进二进制，
//! 解析成结构化版本列表供启动器「更新内容」页展示。
//!
//! 用 include_str! 而不是运行时读文件，安装后的程序不依赖仓库目录；
//! 每次发版改 RELEASE_NOTES.md 后重新构建即可同步。

use serde::Serialize;

const NOTES: &str = include_str!("../../RELEASE_NOTES.md");
const MAX_VERSIONS: usize = 8;

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct ChangelogEntry {
    /// 版本号，例如 0.6.1。
    version: String,
    /// 是否为当前运行版本。
    current: bool,
    /// 是否为历史版本。
    history: bool,
    /// 条目；以 "· " 开头的为上级条目的子项。
    items: Vec<String>,
}

#[derive(Debug, Serialize, PartialEq)]
pub(crate) struct Changelog {
    version: String,
    entries: Vec<ChangelogEntry>,
}

fn heading_version(line: &str) -> Option<(String, bool)> {
    let rest = line
        .strip_prefix("# Aurora Launcher v")
        .or_else(|| line.strip_prefix("## "))?
        .trim();
    if rest.is_empty() {
        return None;
    }
    let history = rest.contains("历史");
    let version = rest
        .split(['（', '('])
        .next()
        .unwrap_or(rest)
        .trim()
        .to_string();
    if version.is_empty() || !version.chars().next()?.is_ascii_digit() {
        return None;
    }
    Some((version, history))
}

/// 解析 RELEASE_NOTES.md：一级标题切分版本，`- ` 为条目，缩进项合并为 "· " 子项。
fn parse_notes(text: &str, current: &str) -> Vec<ChangelogEntry> {
    let mut entries: Vec<ChangelogEntry> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_end();
        if let Some((version, history)) = heading_version(line.trim_start()) {
            entries.push(ChangelogEntry {
                version,
                current: false,
                history,
                items: Vec::new(),
            });
            continue;
        }
        let Some(entry) = entries.last_mut() else {
            continue;
        };
        let trimmed = line.trim_start();
        if let Some(item) = trimmed.strip_prefix("- ") {
            // 顶层条目：缩进 ≥2 个空格视为同一条目的续行/子项。
            let indent = line.len() - trimmed.len();
            let text = item.trim().to_string();
            if text.is_empty() {
                continue;
            }
            if indent >= 2 {
                if let Some(last) = entry.items.last_mut() {
                    last.push('\n');
                    last.push_str("· ");
                    last.push_str(&text);
                } else {
                    entry.items.push(format!("· {text}"));
                }
            } else {
                entry.items.push(text);
            }
        } else if !trimmed.is_empty() {
            // 续行并入上一条目，避免长段落被丢掉。
            if let Some(last) = entry.items.last_mut() {
                last.push(' ');
                last.push_str(trimmed);
            }
        }
    }
    for entry in &mut entries {
        entry.current = entry.version == current;
    }
    entries.truncate(MAX_VERSIONS);
    entries
}

#[tauri::command]
pub(crate) fn app_changelog() -> Changelog {
    let version = env!("CARGO_PKG_VERSION").to_string();
    let entries = parse_notes(NOTES, &version);
    Changelog { version, entries }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "# Aurora Launcher v0.6.1\n\n- 实例内容管理：分页布局。\n  - 模组：启停 .jar。\n  - 存档：解析 level.dat。\n- 安全边界：路径校验。\n\n# Aurora Launcher v0.6.0（历史）\n\n- AI 伙伴（实验）。\n";

    #[test]
    fn parses_versions_and_nested_items() {
        let entries = parse_notes(SAMPLE, "0.6.1");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].version, "0.6.1");
        assert!(entries[0].current);
        assert!(!entries[0].history);
        assert_eq!(entries[0].items.len(), 2);
        assert!(entries[0].items[0].starts_with("实例内容管理"));
        assert!(entries[0].items[0].contains("· 模组"), "子项应并入上级条目");
        assert!(entries[0].items[0].contains("· 存档"));
        assert_eq!(entries[1].version, "0.6.0");
        assert!(entries[1].history);
        assert!(!entries[1].current);
    }

    #[test]
    fn real_notes_include_current_version() {
        let version = env!("CARGO_PKG_VERSION");
        let entries = parse_notes(NOTES, version);
        assert!(!entries.is_empty(), "RELEASE_NOTES.md 应至少有一个版本");
        assert!(
            entries.iter().any(|entry| entry.current),
            "RELEASE_NOTES.md 应包含当前版本 {version} 的条目"
        );
        assert!(
            entries[0].current,
            "当前版本应排在最前，实际是 {}",
            entries[0].version
        );
        assert!(entries[0].items.iter().all(|item| !item.is_empty()));
    }

    #[test]
    fn ignores_non_version_headings() {
        let entries = parse_notes("# 一些说明\n\n- 不属于版本的内容\n", "0.0.0");
        assert!(entries.is_empty());
    }
}

