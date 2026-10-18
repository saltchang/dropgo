//! Share paths (`notes/2026`) and file paths inside a share.

use rand::RngExt;

pub const MAX_SEGMENTS: usize = 8;
const MAX_SEGMENT_LEN: usize = 64;

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum PathError {
    #[error("path.too_many_segments")]
    TooManySegments,
    #[error("path.bad_segment")]
    BadSegment,
    #[error("path.reserved")]
    Reserved,
}

/// Normalizes owner input into a stored share path. `None` means "generate one".
pub fn parse_share_path(input: &str) -> Result<Option<String>, PathError> {
    let p = input.trim().trim_matches('/').to_lowercase();
    if p.is_empty() {
        return Ok(None);
    }
    let segments: Vec<&str> = p.split('/').collect();
    if segments.len() > MAX_SEGMENTS {
        return Err(PathError::TooManySegments);
    }
    for s in &segments {
        if s.is_empty()
            || s.len() > MAX_SEGMENT_LEN
            || !s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        {
            return Err(PathError::BadSegment);
        }
        if s.starts_with('_') {
            return Err(PathError::Reserved);
        }
    }
    Ok(Some(p))
}

pub fn random_share_path() -> String {
    const CHARS: &[u8] = b"abcdefghijkmnpqrstuvwxyz23456789";
    let mut rng = rand::rng();
    (0..8).map(|_| CHARS[rng.random_range(0..CHARS.len())] as char).collect()
}

/// Checks a relative file path from an upload or a zip entry.
pub fn valid_rel_path(p: &str) -> bool {
    !p.is_empty()
        && !p.starts_with('/')
        && !p.contains(['\\', '\0'])
        && p.split('/').all(|s| !s.is_empty() && s != "." && s != "..")
}

/// Every prefix of a lowercased request path that could be a share path.
pub fn lookup_candidates(req_path: &str) -> Vec<String> {
    let lower = req_path.trim_start_matches('/').to_lowercase();
    let segs: Vec<&str> = lower.split('/').filter(|s| !s.is_empty()).collect();
    (1..=segs.len().min(MAX_SEGMENTS)).map(|n| segs[..n].join("/")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_paths() {
        assert_eq!(parse_share_path("Notes/2026/Q3").unwrap().unwrap(), "notes/2026/q3");
        assert_eq!(parse_share_path(" /a/b/ ").unwrap().unwrap(), "a/b");
        assert_eq!(parse_share_path("a_b-c").unwrap().unwrap(), "a_b-c");
        assert_eq!(parse_share_path("").unwrap(), None);
        assert_eq!(parse_share_path("_dropgo/x"), Err(PathError::Reserved));
        assert_eq!(parse_share_path("docs/_x"), Err(PathError::Reserved));
        assert_eq!(parse_share_path("a//b"), Err(PathError::BadSegment));
        assert_eq!(parse_share_path("a b"), Err(PathError::BadSegment));
        assert_eq!(parse_share_path("caf\u{e9}"), Err(PathError::BadSegment));
        assert_eq!(parse_share_path(&"a".repeat(65)), Err(PathError::BadSegment));
        assert!(parse_share_path(&"a".repeat(64)).is_ok());
        assert_eq!(parse_share_path("1/2/3/4/5/6/7/8/9"), Err(PathError::TooManySegments));
        assert!(parse_share_path("1/2/3/4/5/6/7/8").is_ok());
        let r = random_share_path();
        assert_eq!(parse_share_path(&r).unwrap().unwrap(), r);
    }

    #[test]
    fn rel_paths() {
        assert!(valid_rel_path("img/a.png"));
        assert!(valid_rel_path("a..b.txt"));
        for bad in ["", "/etc/passwd", "../../etc/passwd", "a/../b", "a\\b", "a//b", "a/", "a\0b", "./a"] {
            assert!(!valid_rel_path(bad), "{bad:?}");
        }
    }

    #[test]
    fn candidates() {
        assert_eq!(lookup_candidates("/A/b/x.png"), ["a", "a/b", "a/b/x.png"]);
        assert_eq!(lookup_candidates("/"), Vec::<String>::new());
    }
}
