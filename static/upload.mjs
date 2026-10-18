// Upload page: pick or drop files, preview the ignore rules, check the path, send with progress.
import ignore from './vendor/ignore.mjs';
import { filterFiles, stripTopDir, entryFile } from './filter.mjs';

const $ = (id) => document.getElementById(id);
const S = JSON.parse($('strings').textContent);
const fmt = (key, ...args) => args.reduce((s, a) => s.replace('{}', a), S[key]);
const drop = $('drop');
const form = $('upload-form');
const MAX_BYTES = Number(drop.dataset.maxBytes);
const MAX_FILES = 10000;

// `files` is [{ path, file }] for files and folders, or `zip` is one File.
let pick = null;
let pathState = { ok: true, existing: null };

function size(b) {
  if (b >= 1 << 20) return `${(b / (1 << 20)).toFixed(1)} MB`;
  if (b >= 1 << 10) return `${(b / (1 << 10)).toFixed(1)} KB`;
  return `${b} B`;
}

async function choose(entries) {
  $('error').textContent = '';
  const skippedBox = $('skipped');
  skippedBox.hidden = true;
  if (entries.length === 1 && /\.zip$/i.test(entries[0].path)) {
    const zip = entries[0].file;
    pick = zip.size > MAX_BYTES ? null : { zip };
    $('summary').textContent = zip.size > MAX_BYTES ? fmt('upload.js.too_big', size(MAX_BYTES)) : fmt('upload.js.zip', zip.name);
    return update();
  }
  // Only the ignore files need their text.
  const withText = await Promise.all(
    entries.map(async (e) => {
      const name = e.path.split('/').pop();
      const text = name === '.gitignore' || name === '.dockerignore' ? await e.file.text() : undefined;
      return { path: e.path, text, file: e.file };
    }),
  );
  const { kept, skipped } = filterFiles(withText, ignore);
  const byPath = new Map(withText.map((e) => [e.path, e.file]));
  const files = kept.map((p) => ({ path: p, file: byPath.get(p) }));
  const total = files.reduce((n, f) => n + f.file.size, 0);
  const entry = entryFile(stripTopDir(kept));
  const lines = [fmt('upload.js.files', kept.length, size(total))];
  let ok = true;
  if (total > MAX_BYTES) { lines.push(fmt('upload.js.too_big', size(MAX_BYTES))); ok = false; }
  if (kept.length > MAX_FILES) { lines.push(S['upload.js.too_many']); ok = false; }
  if (!entry) { lines.push(S['upload.js.no_entry']); ok = false; }
  else lines.push(fmt('upload.js.entry', entry));
  $('summary').textContent = lines.join(' · ');
  if (skipped.length) {
    skippedBox.querySelector('summary').textContent = fmt('upload.js.skipped', skipped.length);
    skippedBox.querySelector('ul').replaceChildren(...skipped.map((p) => Object.assign(document.createElement('li'), { textContent: p })));
    skippedBox.hidden = false;
  }
  pick = ok && kept.length ? { files } : null;
  update();
}

function update() {
  const overwriteOk = !pathState.existing || $('overwrite-ok').checked;
  $('submit').disabled = !pick || !pathState.ok || !overwriteOk;
}

// The form offers owner or public; a password, new or kept from the share being overwritten, makes it a password share.
function visibility() {
  if ($('visibility').value === 'owner') return 'owner';
  const keep = pathState.existing?.visibility === 'password' && $('overwrite-ok').checked;
  return $('password').value || keep ? 'password' : 'public';
}

// ---------- picking and dropping ----------

// A picked folder arrives as `site/...`; the folder itself is the share root.
$('pick-folder').addEventListener('change', (e) =>
  choose([...e.target.files].map((f) => ({ path: f.webkitRelativePath.split('/').slice(1).join('/'), file: f }))),
);
$('pick-file').addEventListener('change', (e) => choose([...e.target.files].map((f) => ({ path: f.name, file: f }))));
const picker = $('picker');
picker.addEventListener('change', () => { picker.open = false; });
document.addEventListener('click', (e) => { if (!picker.contains(e.target)) picker.open = false; });
if (!('webkitdirectory' in document.createElement('input')) || /Android|iPhone|iPad/.test(navigator.userAgent)) {
  $('folder-label').hidden = true;
  // With one choice left, the button opens the file picker directly.
  picker.querySelector('summary .icon:last-child').remove();
  picker.querySelector('summary').addEventListener('click', (e) => { e.preventDefault(); $('pick-file').click(); });
}

function walk(entry, prefix) {
  if (entry.isFile) return new Promise((ok, fail) => entry.file((f) => ok([{ path: prefix + entry.name, file: f }]), fail));
  const reader = entry.createReader();
  const all = [];
  // readEntries returns at most 100 entries per call.
  const more = () =>
    new Promise((ok, fail) => reader.readEntries(ok, fail)).then((batch) => {
      if (!batch.length) return all;
      all.push(...batch);
      return more();
    });
  return more().then((children) => Promise.all(children.map((c) => walk(c, `${prefix}${entry.name}/`))).then((r) => r.flat()));
}

drop.addEventListener('dragover', (e) => { e.preventDefault(); drop.classList.add('over'); });
drop.addEventListener('dragleave', () => drop.classList.remove('over'));
drop.addEventListener('drop', async (e) => {
  e.preventDefault();
  drop.classList.remove('over');
  const roots = [...e.dataTransfer.items].map((i) => i.webkitGetAsEntry()).filter(Boolean);
  // One dropped folder is the share root, like a picked one.
  if (roots.length === 1 && roots[0].isDirectory) {
    const files = await walk(roots[0], '');
    const cut = roots[0].name.length + 1;
    return choose(files.map((f) => ({ path: f.path.slice(cut), file: f.file })));
  }
  choose((await Promise.all(roots.map((r) => walk(r, '')))).flat());
});

// ---------- settings ----------

function setExpiry(iso) {
  const d = iso && new Date(iso);
  $('expires-local').value = d ? new Date(d.getTime() - d.getTimezoneOffset() * 60000).toISOString().slice(0, 16) : '';
}

let checkTimer;
let checkSeq = 0;
$('path').addEventListener('input', () => {
  clearTimeout(checkTimer);
  checkTimer = setTimeout(checkPath, 300);
});

async function checkPath() {
  const seq = ++checkSeq;
  const res = await fetch(`/api/check?path=${encodeURIComponent($('path').value)}`);
  const body = await res.json().catch(() => ({}));
  if (seq !== checkSeq) return;
  const msg = $('path-msg');
  msg.classList.toggle('bad', !res.ok && !body.existing);
  msg.textContent = res.ok ? '' : body.error || S['upload.js.bad_path'];
  pathState = { ok: res.ok || !!body.existing, existing: body.existing || null };
  showOverwrite();
  update();
}

function showOverwrite() {
  const ex = pathState.existing;
  $('overwrite').hidden = !ex;
  $('overwrite-ok').checked = false;
  $('password').placeholder = S[ex?.visibility === 'password' ? 'form.password_keep' : 'form.password_hint'];
  if (!ex) return;
  $('path-msg').textContent = '';
  $('overwrite-msg').textContent = S['upload.js.overwrite'];
  $('visibility').value = ex.visibility === 'owner' ? 'owner' : 'public';
  setExpiry(ex.expires_at);
  form.dispatchEvent(new Event('change', { bubbles: true }));
}

form.addEventListener('input', update);
form.addEventListener('change', update);

// ---------- sending ----------

form.addEventListener('submit', (e) => {
  e.preventDefault();
  if ($('submit').disabled) return;
  const fd = new FormData();
  // Settings go first so the server can refuse before the files arrive.
  fd.append('path', $('path').value);
  fd.append('visibility', visibility());
  fd.append('password', visibility() === 'owner' ? '' : $('password').value);
  const local = $('expires-local').value;
  fd.append('expiry', local ? 'custom' : 'never');
  fd.append('expires_at', local ? new Date(local).toISOString() : '');
  if (pathState.existing && $('overwrite-ok').checked) fd.append('overwrite', pathState.existing.id);
  if (pick.zip) fd.append('zip', pick.zip, pick.zip.name);
  else for (const f of pick.files) fd.append(`f:${f.path}`, f.file, f.file.name);

  const xhr = new XMLHttpRequest();
  const progress = $('progress');
  $('submit').disabled = true;
  $('error').textContent = '';
  progress.hidden = false;
  progress.removeAttribute('value');
  xhr.upload.onprogress = (ev) => { if (ev.lengthComputable) progress.value = ev.loaded / ev.total; };
  xhr.upload.onload = () => progress.removeAttribute('value');
  xhr.onerror = () => { progress.hidden = true; $('error').textContent = S['upload.js.network']; update(); };
  xhr.onload = () => {
    progress.hidden = true;
    let body = {};
    try { body = JSON.parse(xhr.responseText); } catch {}
    if (xhr.status === 200) {
      form.hidden = true;
      drop.hidden = true;
      $('done').hidden = false;
      $('share-url').value = body.url;
      $('share-link').href = body.url;
      return;
    }
    $('error').textContent = body.error || S['upload.js.failed'];
    if (body.existing) { pathState = { ok: true, existing: body.existing }; showOverwrite(); }
    update();
  };
  xhr.open('POST', '/api/upload');
  xhr.send(fd);
});
