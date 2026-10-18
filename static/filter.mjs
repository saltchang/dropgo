// Ignore rules for folder uploads, matching src/ignore_rules.rs.
// `ignore` is the npm package's factory; `tests/fixtures/ignore/` checks both sides agree.

const SKIPPED_DIRS = ['node_modules', '__MACOSX'];

export function anchorDockerignoreLine(line) {
  line = line.trim();
  if (!line || line.startsWith('#')) return null;
  const neg = line.startsWith('!') ? '!' : '';
  const pat = neg ? line.slice(1) : line;
  return pat.startsWith('/') ? neg + pat : `${neg}/${pat}`;
}

function parents(path) {
  const parts = path.split('/');
  const out = [];
  for (let i = parts.length - 1; i > 0; i--) out.push(parts.slice(0, i).join('/'));
  return out;
}

// files: [{ path, text }] where `text` is set for .gitignore and .dockerignore files.
// Returns { kept: [path], skipped: [path] }, both sorted.
export function filterFiles(files, ignore) {
  const gitignores = new Map();
  let docker = [];
  for (const f of files) {
    const name = f.path.split('/').pop();
    if (name === '.gitignore') gitignores.set(f.path.slice(0, -name.length).replace(/\/$/, ''), ignore().add(f.text));
    if (f.path === '.dockerignore') {
      docker = f.text.split(/\r?\n/).map(anchorDockerignoreLine).filter(Boolean).map((line) => {
        const neg = line.startsWith('!');
        return { neg, ig: ignore().add(neg ? line.slice(1) : line) };
      });
    }
  }

  // Like git: the deepest .gitignore with an opinion wins.
  const gitState = (path, isDir) => {
    for (const dir of parents(path).concat([''])) {
      const ig = gitignores.get(dir);
      if (!ig) continue;
      const rel = (dir ? path.slice(dir.length + 1) : path) + (isDir ? '/' : '');
      const r = ig.test(rel);
      if (r.ignored) return true;
      if (r.unignored) return false;
    }
    return false;
  };
  // Like Docker: each pattern is checked against the path and its parents, and the last match wins.
  const dockerIgnored = (path) => {
    const last = docker.filter((r) => r.ig.ignores(path)).pop();
    return last ? !last.neg : false;
  };

  const kept = [];
  const skipped = [];
  for (const f of files) {
    const segs = f.path.split('/');
    const skip =
      segs.some((s) => s.startsWith('.') || SKIPPED_DIRS.includes(s)) ||
      parents(f.path).some((d) => gitState(d, true)) ||
      gitState(f.path, false) ||
      dockerIgnored(f.path);
    (skip ? skipped : kept).push(f.path);
  }
  return { kept: kept.sort(), skipped: skipped.sort() };
}

// Mirrors single_top_dir and entry_file in src/ignore_rules.rs.
export function stripTopDir(paths) {
  const first = paths[0]?.split('/')[0];
  const all = paths.length > 0 && paths.every((p) => p.includes('/') && p.split('/')[0] === first);
  return all ? paths.map((p) => p.slice(first.length + 1)) : paths;
}

export function entryFile(paths) {
  for (const name of ['index.html', 'index.md']) if (paths.includes(name)) return name;
  const top = paths.filter((p) => !p.includes('/') && (p.endsWith('.html') || p.endsWith('.md')));
  return top.length === 1 ? top[0] : null;
}
