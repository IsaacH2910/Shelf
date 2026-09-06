use regex::Regex;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ParsedChapter {
    pub title: String,
    pub chapter_number: Option<f64>,
    pub volume_number: Option<i32>,
    pub sort_key: f64,
}

const UNNUMBERED_SORT_KEY: f64 = 9_000_000.0;

pub fn parse_chapter_from_filename(file_path: &str) -> ParsedChapter {
    let path = Path::new(file_path);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Unknown")
        .to_string();

    let mut chapter_number = None;
    let volume_number;
    let mut sort_key = UNNUMBERED_SORT_KEY;

    // Explicit chapter markers first: Ch/Chapter/Ep/Episode/第X話
    if chapter_number.is_none() {
        chapter_number = capture_number(
            &stem,
            r"(?i)(?:ch(?:apter)?\.?\s*|ep(?:isode)?\.?\s*|第\s*)(\d{1,4}(?:\.\d+)?)\s*(?:話|话|回)?",
        );
    }

    // Season + episode pattern: S1E03 / S1 E03
    if chapter_number.is_none() {
        chapter_number = capture_number(&stem, r"(?i)\bs\d{1,2}\s*[-_. ]*e(\d{1,4})\b");
    }

    // Leading numeric prefix: 01 - title / 1. title
    if chapter_number.is_none() {
        chapter_number = capture_number(
            &stem,
            r"(?i)^\s*(\d{1,4}(?:\.\d+)?)\s*(?:[-_.、．。\s]|$)",
        );
    }

    // Volume marker kept separate so chapter-like ordering remains primary.
    volume_number = capture_int(&stem, r"(?i)(?:vol(?:ume)?\.?\s*|v\.?\s*)(\d{1,4})");

    // Final fallback: bounded generic numbers only; rejects huge IDs/timestamps.
    if chapter_number.is_none() && volume_number.is_none() {
        chapter_number = capture_number(&stem, r"\b(\d{1,4}(?:\.\d+)?)\b");
    }

    let variant_offset = variant_rank_offset(&stem);
    if let Some(n) = chapter_number {
        sort_key = n + variant_offset;
    } else if let Some(v) = volume_number {
        sort_key = v as f64 * 1000.0 + variant_offset;
    }

    ParsedChapter {
        title: stem.clone(),
        chapter_number,
        volume_number,
        sort_key,
    }
}

fn variant_rank_offset(stem: &str) -> f64 {
    let lower = stem.to_lowercase();

    // Keep main chapter/episode first, while still retaining alternate versions.
    if contains_any(&lower, &["无修正", "無修正", "无码", "無碼", "去码", "去碼"]) {
        return 0.02;
    }
    if contains_any(&lower, &["修正", "修復", "修复", "改", "revised", "fix"]) {
        return 0.08;
    }
    if contains_any(&lower, &["補圖", "补图", "extra", "bonus", "番外", "特別", "特别", "後記", "后记", "外傳", "外传"]) {
        return 0.12;
    }
    0.0
}

fn contains_any(text: &str, tokens: &[&str]) -> bool {
    tokens.iter().any(|t| text.contains(t))
}

fn capture_number(input: &str, pattern: &str) -> Option<f64> {
    let re = Regex::new(pattern).ok()?;
    let caps = re.captures(input)?;
    let num = caps.get(1)?.as_str().parse::<f64>().ok()?;
    if num > 0.0 { Some(num) } else { None }
}

fn capture_int(input: &str, pattern: &str) -> Option<i32> {
    let re = Regex::new(pattern).ok()?;
    let caps = re.captures(input)?;
    let num = caps.get(1)?.as_str().parse::<i32>().ok()?;
    if num > 0 { Some(num) } else { None }
}

#[cfg(test)]
mod tests {
    use super::parse_chapter_from_filename;
    use super::UNNUMBERED_SORT_KEY;

    #[test]
    fn parses_ch_dot() {
        let p = parse_chapter_from_filename("/lib/One Piece/Ch. 12.pdf");
        assert_eq!(p.chapter_number, Some(12.0));
        assert_eq!(p.sort_key, 12.0);
    }

    #[test]
    fn parses_chapter_word() {
        let p = parse_chapter_from_filename("Chapter 12.pdf");
        assert_eq!(p.chapter_number, Some(12.0));
    }

    #[test]
    fn parses_volume() {
        let p = parse_chapter_from_filename("Vol. 03.pdf");
        assert_eq!(p.volume_number, Some(3));
        assert_eq!(p.sort_key, 3000.0);
    }

    #[test]
    fn parses_v01() {
        let p = parse_chapter_from_filename("v01.pdf");
        assert_eq!(p.volume_number, Some(1));
    }

    #[test]
    fn parses_japanese_chapter() {
        let p = parse_chapter_from_filename("第12話.pdf");
        assert_eq!(p.chapter_number, Some(12.0));
    }

    #[test]
    fn parses_bare_number() {
        let p = parse_chapter_from_filename("12.pdf");
        assert_eq!(p.chapter_number, Some(12.0));
    }

    #[test]
    fn parses_leading_episode_number_when_id_suffix_exists() {
        let p = parse_chapter_from_filename("1. 初始之風 無限之路 [201264].mp4");
        assert_eq!(p.chapter_number, Some(1.0));
        assert_eq!(p.sort_key, 1.0);
    }

    #[test]
    fn ignores_large_timestamp_like_number() {
        let p = parse_chapter_from_filename("video_HQ_225305.mp4");
        assert_eq!(p.chapter_number, None);
        assert_eq!(p.sort_key, UNNUMBERED_SORT_KEY);
    }

    #[test]
    fn keeps_explicit_chapter_marker_over_season_number() {
        let p = parse_chapter_from_filename("【日語】進擊的巨人S1 第02話.mp4");
        assert_eq!(p.chapter_number, Some(2.0));
    }

    #[test]
    fn keeps_primary_variant_before_extras() {
        let base = parse_chapter_from_filename("第46话.pdf");
        let uncensored = parse_chapter_from_filename("第46话 (无码).pdf");
        let revised = parse_chapter_from_filename("第46话 改.pdf");
        let bonus = parse_chapter_from_filename("第46话 补图.pdf");

        assert_eq!(base.chapter_number, Some(46.0));
        assert_eq!(uncensored.chapter_number, Some(46.0));
        assert_eq!(revised.chapter_number, Some(46.0));
        assert_eq!(bonus.chapter_number, Some(46.0));

        assert!(base.sort_key < uncensored.sort_key);
        assert!(uncensored.sort_key < revised.sort_key);
        assert!(revised.sort_key < bonus.sort_key);
    }

    #[test]
    fn applies_variant_offset_to_volume_entries() {
        let base = parse_chapter_from_filename("Vol. 03.pdf");
        let bonus = parse_chapter_from_filename("Vol. 03 特別篇.pdf");

        assert_eq!(base.volume_number, Some(3));
        assert_eq!(bonus.volume_number, Some(3));
        assert!(base.sort_key < bonus.sort_key);
    }
}
