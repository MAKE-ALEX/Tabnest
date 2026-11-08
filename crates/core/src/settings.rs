//! Validation for the user preferences that used to live only in
//! `localStorage`. The server is now the source of truth so several devices
//! share one look; these helpers keep both runtimes accepting the same values.

pub const THEMES: [&str; 3] = ["light", "dark", "auto"];
pub const PALETTES: [&str; 6] = ["clay", "moss", "ocean", "plum", "sakura", "amber"];
pub const COLUMN_CHOICES: [&str; 8] = ["auto", "2", "3", "4", "5", "6", "7", "8"];

/// Groups per board (visual columns) and groups+links per layout payload.
const MAX_VISUAL_COLUMNS: usize = 64;
const MAX_ENTRIES: usize = 2000;

pub fn valid_theme(value: &str) -> bool {
    THEMES.contains(&value)
}

pub fn valid_palette(value: &str) -> bool {
    PALETTES.contains(&value)
}

pub fn valid_columns(value: &str) -> bool {
    COLUMN_CHOICES.contains(&value)
}

/// Drop unknown, duplicated or negative ids and cap the size so a malformed
/// client cannot store an unbounded blob.
pub fn sanitize_layout(raw: Vec<Vec<i64>>) -> Vec<Vec<i64>> {
    let mut seen = std::collections::HashSet::new();
    let mut total = 0usize;
    let mut result = Vec::with_capacity(raw.len().min(MAX_VISUAL_COLUMNS));

    for group in raw.into_iter().take(MAX_VISUAL_COLUMNS) {
        let mut clean = Vec::new();
        for id in group {
            if id <= 0 || !seen.insert(id) {
                continue;
            }
            if total >= MAX_ENTRIES {
                break;
            }
            clean.push(id);
            total += 1;
        }
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
