// Browser QA against the stack in compose.yml: sign-in, uploads, share pages, markdown, trash and 360 px.
// Writes 2560x1440 screenshots to shots/ and exits 1 if a check fails. Run: npm run qa
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { crc32 } from 'node:zlib';
import { chromium } from 'playwright';

const A = 'https://drop.localhost:8443';
const dir = new URL('.', import.meta.url).pathname;
mkdirSync(dir + 'shots', { recursive: true });
const shot = (p, n) => p.screenshot({ path: `${dir}shots/${n}.png` });
// Paths are unique per run, so the script can run again on the same stack.
const R = Date.now().toString(36);
const log = (...a) => console.log('QA', ...a);
const failed = [];
const check = (name, ok, detail = '') => {
  console.log(ok ? 'ok  ' : 'FAIL', name, detail);
  if (!ok) failed.push(name);
};

// Fixtures are written at run time: git would skip the ignored and junk files they must contain.
const fx = mkdtempSync(join(tmpdir(), 'dropgo-qa-'));
const site = {
  'index.html': `<!doctype html><html><head><meta charset="utf-8"><title>My site</title><link rel="stylesheet" href="style.css"></head>
<body><h1>Hello from a dropped folder</h1><img src="img/dot.svg" width="64" alt="dot"><p id="c"></p>
<script>try{document.cookie;document.getElementById('c').textContent='cookie readable'}catch(e){document.getElementById('c').textContent='cookie blocked: '+e.name}</script></body></html>
`,
  'style.css': 'body{font-family:sans-serif;background:#fef6e4;color:#172c66;padding:2rem}\n',
  'img/dot.svg': '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="5" fill="#f582ae"/></svg>\n',
  '.gitignore': 'secret.txt\n',
  'secret.txt': 's\n',
  '.DS_Store': 'd\n',
  'node_modules/lib/x.js': 'x\n',
};
const notes = '# Release notes\n\nRendered on upload with **comrak**.\n\n```rust\nfn main() { println!("hi"); }\n```\n\n```mermaid\ngraph LR; Upload-->Claim-->Serve\n```\n\nEuler: $e^{i\\pi} + 1 = 0$\n\n| a | b |\n|---|---|\n| 1 | 2 |\n';
for (const [rel, body] of Object.entries(site)) {
  mkdirSync(dirname(join(fx, 'mysite', rel)), { recursive: true });
  writeFileSync(join(fx, 'mysite', rel), body);
}
writeFileSync(join(fx, 'notes.md'), notes);
const zipEntries = Object.entries(site).map(([rel, body]) => ['mysite/' + rel, body]);
zipEntries.push(['__MACOSX/mysite/._index.html', 'm']);
writeFileSync(join(fx, 'mysite.zip'), storedZip(zipEntries));

/// A zip without compression, enough for a fixture.
function storedZip(entries) {
  const parts = [], central = [];
  let offset = 0;
  for (const [name, text] of entries) {
    const n = Buffer.from(name), data = Buffer.from(text), crc = crc32(data);
    const local = Buffer.alloc(30);
    local.writeUInt32LE(0x04034b50, 0); local.writeUInt16LE(20, 4);
    local.writeUInt32LE(crc, 14); local.writeUInt32LE(data.length, 18); local.writeUInt32LE(data.length, 22);
    local.writeUInt16LE(n.length, 26);
    const c = Buffer.alloc(46);
    c.writeUInt32LE(0x02014b50, 0); c.writeUInt16LE(20, 4); c.writeUInt16LE(20, 6);
    c.writeUInt32LE(crc, 16); c.writeUInt32LE(data.length, 20); c.writeUInt32LE(data.length, 24);
    c.writeUInt16LE(n.length, 28); c.writeUInt32LE(offset, 42);
    parts.push(local, n, data); central.push(c, n);
    offset += 30 + n.length + data.length;
  }
  const cd = Buffer.concat(central), end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50, 0); end.writeUInt16LE(entries.length, 8); end.writeUInt16LE(entries.length, 10);
  end.writeUInt32LE(cd.length, 12); end.writeUInt32LE(offset, 16);
  return Buffer.concat([...parts, cd, end]);
}

const br = await chromium.launch();
const ctx = await br.newContext({ ignoreHTTPSErrors: true, viewport: { width: 2560, height: 1440 }, locale: 'zh-TW' });
const p = await ctx.newPage();
const errors = [];
p.on('console', (m) => m.type() === 'error' && errors.push(m.text()));

async function login(page, user = 'alice') {
  await page.waitForURL(/localhost:18080/);
  await page.fill('input[name=username]', `sub-${user}`);
  await page.fill('textarea[name=claims]', JSON.stringify({ preferred_username: user }));
  await page.locator('form [type=submit]').first().click();
}

// 1. Sign in.
await p.goto(A + '/');
await login(p);
await p.waitForURL(A + '/');
log('signed in, title:', await p.title());
await shot(p, '01-upload-empty');
await p.click('#picker summary');
check('one picker button opens folder and file choices', await p.locator('#folder-label').isVisible() && await p.locator('label:has(#pick-file)').isVisible());
await shot(p, '01b-picker-open');
await p.keyboard.press('Escape');
await p.click('h1');
check('password is disabled for owner only', await p.locator('#password').isDisabled());

// 2. Folder upload with client-side ignore preview.
await p.setInputFiles('#pick-folder', fx + '/mysite');
await p.waitForSelector('#skipped:not([hidden])');
log('summary:', await p.textContent('#summary'));
await p.click('#skipped summary');
const skipped = (await p.locator('#skipped li').allTextContents()).join(', ');
check('folder upload skips ignored files', skipped === '.DS_Store, .gitignore, node_modules/lib/x.js, secret.txt', skipped);
await p.fill('#path', `demo/site-${R}`);
await p.selectOption('#visibility', 'public');
await p.waitForFunction(() => !document.getElementById('submit').disabled);
await shot(p, '02-upload-preview');
await p.click('#submit');
await p.waitForSelector('#done:not([hidden])');
const siteUrl = await p.inputValue('#share-url');
log('share url:', siteUrl);
await shot(p, '03-upload-done');

// 3. Visit the share in another page.
const v = await ctx.newPage();
await v.goto(siteUrl);
check('share page cannot read cookies', (await v.textContent('#c')) === 'cookie blocked: SecurityError', v.url());
check('share page image loads', await v.evaluate(() => document.images[0].naturalWidth > 0));
await shot(v, '04-share-site');

// 4. Markdown upload, password protected, then re-upload to the same path for the overwrite prompt.
await p.goto(A + '/');
await p.setInputFiles('#pick-file', fx + '/notes.md');
await p.fill('#path', `notes-${R}`);
await p.selectOption('#visibility', 'public');
await p.fill('#password', 'hunter2');
await p.click('[data-reveal=password]');
check('eye button shows the password', (await p.getAttribute('#password', 'type')) === 'text');
// Near its top edge: the press must not move the button out from under the pointer.
await p.click('[data-reveal=password]', { position: { x: 18, y: 3 } });
check('eye button hides it again', (await p.getAttribute('#password', 'type')) === 'password');
await p.click('.chip[data-days="7"]');
check('expiry chip fills the date', (await p.inputValue('#expires-local')).length === 16);
await shot(p, '02b-password-expiry');
await p.waitForFunction(() => !document.getElementById('submit').disabled);
await p.click('#submit');
await p.waitForSelector('#done:not([hidden])');
const notesUrl = await p.inputValue('#share-url');

await p.goto(A + '/');
await p.setInputFiles('#pick-file', fx + '/notes.md');
await p.fill('#path', `notes-${R}`);
await p.waitForSelector('#overwrite:not([hidden])');
check('overwrite prompt fills in settings', (await p.inputValue('#visibility')) === 'public' && (await p.inputValue('#expires-local')).length === 16 && (await p.getAttribute('#password', 'placeholder')) !== '', await p.textContent('#overwrite-msg'));
await shot(p, '05-overwrite-prompt');

// 5. Password form as a visitor (fresh context), then the rendered markdown.
const anon = await br.newContext({ ignoreHTTPSErrors: true, viewport: { width: 2560, height: 1440 } });
const w = await anon.newPage();
await w.goto(notesUrl);
await shot(w, '06-password-form');
await w.fill('input[type=password]', 'wrong');
await w.locator('form [type=submit]').click();
check('wrong password shows an error', await w.locator('.error').isVisible());
await w.fill('input[type=password]', 'hunter2');
await w.locator('form [type=submit]').click();
await w.waitForSelector('svg[id^=mermaid], .mermaid svg', { timeout: 15000 });
await w.waitForSelector('.katex');
await w.evaluate(() => document.fonts.ready);
check('mermaid renders', (await w.locator('.mermaid svg').count()) === 1);
check('katex fonts load', (await w.evaluate(() => [...document.fonts].filter((f) => f.family.includes('KaTeX') && f.status === 'loaded').length)) > 0);
check('document.cookie throws on markdown page', await w.evaluate(() => { try { document.cookie; return false; } catch { return true; } }));
await shot(w, '07-markdown-dark');
check('footer has downloads and theme toggle', (await w.locator('footer a').count()) === 4);
await w.locator('footer a.dropgo-theme').click();
await w.waitForLoadState();
check('theme toggles to light', (await w.evaluate(() => document.documentElement.dataset.theme)) === 'light');
await shot(w, '08-markdown-light');

// 6. List, edit, delete, trash.
await p.goto(A + '/shares');
await shot(p, '09-list');
await p.fill('#search', 'markdown');
const visibleRows = () => p.locator('li.share:visible').count();
check('search matches the file type', (await visibleRows()) >= 1 && (await p.locator('li.share:visible .badge', { hasText: 'HTML' }).count()) === 0);
check('search highlights the hit', (await p.locator('li.share:visible mark').first().textContent()).toLowerCase() === 'markdown');
await shot(p, '09b-list-search');
await p.fill('#search', '');
await p.fill('#from', '2000-01-01');
await p.fill('#to', '2000-01-02');
check('date range hides other days', (await visibleRows()) === 0 && await p.locator('#no-match').isVisible());
await p.fill('#from', '');
await p.fill('#to', '');
await p.locator('a[href$="/edit"]').first().click();
await shot(p, '10-edit');
await p.goto(A + '/shares');
p.on('dialog', (d) => d.accept());
await p.locator('form[action$="/delete"] button').first().click();
await p.goto(A + '/shares?tab=trash');
await shot(p, '11-trash');

// 7. 360 px on a phone: no page scrolls sideways; zip upload works.
const editUrl = A + await p.goto(A + '/shares').then(() => p.locator('a[href$="/edit"]').first().getAttribute('href'));
const phone = await br.newContext({ ignoreHTTPSErrors: true, viewport: { width: 360, height: 780 }, isMobile: true, hasTouch: true, deviceScaleFactor: 4,
  userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1',
  storageState: await ctx.storageState() });
const m = await phone.newPage();
for (const url of [A + '/', A + '/shares', A + '/shares?tab=trash', editUrl]) {
  await m.goto(url);
  const sw = await m.evaluate(() => document.documentElement.scrollWidth);
  check(`no sideways scroll at 360 px: ${url.replace(A, '')}`, sw <= 360, `scrollWidth ${sw}`);
}
await m.goto(A + '/');
check('folder picker hidden on phone', await m.locator('#folder-label').isHidden());
await m.setInputFiles('#pick-file', fx + '/mysite.zip');
await m.fill('#path', `phone-${R}`);
await m.selectOption('#visibility', 'public');
await m.waitForFunction(() => !document.getElementById('submit').disabled);
await m.screenshot({ path: `${dir}shots/12-mobile-upload.png`, fullPage: true });
await m.click('#submit');
await m.waitForSelector('#done:not([hidden])');
const phoneUrl = await m.inputValue('#share-url');
const pv = await (await ctx.request.get(phoneUrl)).text();
check('zip upload serves index.html', pv.includes('Hello from a dropped folder'), phoneUrl);
check('zip junk and ignored files are 404', (await ctx.request.get(phoneUrl + '__MACOSX/mysite/._index.html')).status() === 404 && (await ctx.request.get(phoneUrl + 'secret.txt')).status() === 404);
await m.screenshot({ path: `${dir}shots/13-mobile-done.png`, fullPage: true });
await m.goto(A + '/shares');
await m.screenshot({ path: `${dir}shots/14-mobile-list.png`, fullPage: true });

// The path check answers 409 while the overwrite prompt is shown.
const unexpected = errors.filter((e) => !e.includes('status of 409'));
check('no unexpected console errors', unexpected.length === 0, unexpected.join(' | '));
await br.close();
console.log(failed.length ? `${failed.length} failed` : 'all checks passed');
process.exit(failed.length ? 1 : 0);
