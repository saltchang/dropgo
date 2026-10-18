//! Ignore rules for uploads. `static/filter.mjs` applies the same rules in the browser,
//! and `tests/fixtures/ignore/` checks that both agree.

use std::path::Path;

use ignore::{
    WalkBuilder,
    gitignore::{Gitignore, GitignoreBuilder},
};

const SKIPPED_DIRS: [&str; 2] = ["node_modules", "__MACOSX"];

/// Docker anchors every pattern at the top level. Translate each one to an anchored gitignore line.
pub fn anchor_dockerignore_line(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (neg, pat) = line.strip_prefix('!').map_or(("", line), |p| ("!", p));
    Some(if pat.starts_with('/') { format!("{neg}{pat}") } else { format!("{neg}/{pat}") })
}

/// One matcher per `.dockerignore` pattern, in order, with whether it is a negation.
/// Docker checks each pattern against the path and its parents; the last match wins.
struct DockerIgnore(Vec<(Gitignore, bool)>);

impl DockerIgnore {
    fn load(root: &Path) -> Self {
        let src = std::fs::read_to_string(root.join(".dockerignore")).unwrap_or_default();
        let rules = src
            .lines()
            .filter_map(anchor_dockerignore_line)
            .filter_map(|line| {
                let (neg, pat) = line.strip_prefix('!').map_or((false, line.as_str()), |p| (true, p));
                let mut b = GitignoreBuilder::new(root);
                b.add_line(None, pat).ok()?;
                Some((b.build().ok()?, neg))
            })
            .collect();
        DockerIgnore(rules)
    }

    fn ignores(&self, rel: &Path) -> bool {
        self.0
            .iter()
            .filter(|(g, _)| g.matched_path_or_any_parents(rel, false).is_ignore())
            .last()
            .is_some_and(|(_, neg)| !neg)
    }
}

/// Relative paths of the files under `root` that survive the ignore rules, sorted.
pub fn kept_files(root: &Path) -> std::io::Result<Vec<String>> {
    let docker = DockerIgnore::load(root);
    let walker = WalkBuilder::new(root)
        .hidden(true)
        .git_ignore(true)
        .require_git(false)
        .ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .follow_links(false)
        .filter_entry(|e| !SKIPPED_DIRS.iter().any(|d| e.file_name() == *d))
        .build();
    let mut out = Vec::new();
    for entry in walker {
        let entry = entry.map_err(std::io::Error::other)?;
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let rel = entry.path().strip_prefix(root).expect("under root");
        if docker.ignores(rel) {
            continue;
        }
        out.push(rel.to_string_lossy().replace('\\', "/"));
    }
    out.sort();
    Ok(out)
}

/// If every file sits under one top-level directory, returns that directory name.
pub fn single_top_dir(files: &[String]) -> Option<String> {
    let first = files.first()?.split_once('/')?.0;
    files.iter().all(|f| f.split_once('/').is_some_and(|(d, _)| d == first)).then(|| first.to_string())
}

/// `index.html`, else `index.md`, else the single top-level `.html` or `.md` file.
pub fn entry_file(files: &[String]) -> Option<String> {
    for name in ["index.html", "index.md"] {
        if files.iter().any(|f| f == name) {
            return Some(name.to_string());
        }
    }
    let mut top = files.iter().filter(|f| !f.contains('/') && (f.ends_with(".html") || f.ends_with(".md")));
    match (top.next(), top.next()) {
        (Some(only), None) => Some(only.clone()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Fixture {
        files: std::collections::BTreeMap<String, String>,
        kept: Vec<String>,
    }

    #[test]
    fn shared_fixtures() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ignore");
        let mut n = 0;
        for f in std::fs::read_dir(dir).unwrap() {
            let f = f.unwrap().path();
            let fx: Fixture = serde_json::from_str(&std::fs::read_to_string(&f).unwrap()).unwrap();
            let tmp = tempfile::tempdir().unwrap();
            for (p, content) in &fx.files {
                let full = tmp.path().join(p);
                std::fs::create_dir_all(full.parent().unwrap()).unwrap();
                std::fs::write(full, content).unwrap();
            }
            let mut want = fx.kept.clone();
            want.sort();
            assert_eq!(kept_files(tmp.path()).unwrap(), want, "{}", f.display());
            n += 1;
        }
        assert!(n > 5);
    }

    #[test]
    fn entry() {
        let v = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(entry_file(&v(&["a.md", "index.md", "index.html"])).unwrap(), "index.html");
        assert_eq!(entry_file(&v(&["notes.md", "img/a.png"])).unwrap(), "notes.md");
        assert_eq!(entry_file(&v(&["report.html", "README.md"])), None);
        assert_eq!(entry_file(&v(&["sub/index.html"])), None);
        assert_eq!(single_top_dir(&v(&["site/index.html", "site/a/b.png"])).unwrap(), "site");
        assert_eq!(single_top_dir(&v(&["site/index.html", "x.png"])), None);
        assert_eq!(single_top_dir(&v(&["index.html"])), None);
    }
}
