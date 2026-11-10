//! 用户偏好设置的合法性校验。
//!
//! 这些偏好过去只存在浏览器 `localStorage` 里，现在服务端是唯一数据源，
//! 让多台设备共享同一套外观；下面的辅助函数保证两个运行时接受同一组取值。

pub const THEMES: [&str; 3] = ["light", "dark", "auto"];
pub const PALETTES: [&str; 6] = ["clay", "moss", "ocean", "plum", "sakura", "amber"];
pub const COLUMN_CHOICES: [&str; 8] = ["auto", "2", "3", "4", "5", "6", "7", "8"];

/// 每个面板允许的最大视觉列数，以及单次排版数据允许的分组 + 链接总数。
/// 设置上限是为了防止畸形客户端写入无界的数据块。
const MAX_VISUAL_COLUMNS: usize = 64;
const MAX_ENTRIES: usize = 2000;

/// 校验明暗模式取值是否在允许列表内。
pub fn valid_theme(value: &str) -> bool {
    THEMES.contains(&value)
}

/// 校验配色方案取值是否在允许列表内。
pub fn valid_palette(value: &str) -> bool {
    PALETTES.contains(&value)
}

/// 校验视觉列数取值（`auto` 或 `2`–`8`）。
pub fn valid_columns(value: &str) -> bool {
    COLUMN_CHOICES.contains(&value)
}

/// 清洗前端提交的瀑布流排版：丢弃未知、重复、非正数的分组 id，
/// 并限制列数与总数，避免畸形请求把无界数据写进数据库。
pub fn sanitize_layout(raw: Vec<Vec<i64>>) -> Vec<Vec<i64>> {
    // 全局去重：同一个分组只能出现在一列里。
    let mut seen = std::collections::HashSet::new();
    let mut total = 0usize;
    let mut result = Vec::with_capacity(raw.len().min(MAX_VISUAL_COLUMNS));

    for group in raw.into_iter().take(MAX_VISUAL_COLUMNS) {
        let mut clean = Vec::new();
        for id in group {
            // 跳过非法 id 和已经出现过的 id。
            if id <= 0 || !seen.insert(id) {
                continue;
            }
            if total >= MAX_ENTRIES {
                break;
            }
            clean.push(id);
            total += 1;
        }
        // 即使这一列被清空也保留占位，保证列结构不塌缩。
        result.push(clean);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_known_values() {
        assert!(valid_theme("auto"));
        assert!(!valid_theme("sepia"));
        assert!(valid_palette("clay"));
        assert!(!valid_palette("red"));
        assert!(valid_columns("auto"));
        assert!(valid_columns("8"));
        assert!(!valid_columns("1"));
        assert!(!valid_columns("9"));
    }

    #[test]
    fn sanitizes_layout() {
        let raw = vec![vec![3, 3, -1, 4], vec![5, 6], vec![3, 7]];
        assert_eq!(
            sanitize_layout(raw),
            vec![vec![3, 4], vec![5, 6], vec![7]]
        );
    }

    #[test]
    fn caps_layout_size() {
        let raw: Vec<Vec<i64>> = (0..100).map(|i| vec![i + 1, i + 1]).collect();
        let out = sanitize_layout(raw);
        assert!(out.len() <= MAX_VISUAL_COLUMNS);
        let total: usize = out.iter().map(Vec::len).sum();
        assert!(total <= MAX_ENTRIES);
    }
}
