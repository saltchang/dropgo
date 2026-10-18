import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import ignore from '../static/vendor/ignore.mjs';
import { filterFiles, entryFile, stripTopDir } from '../static/filter.mjs';

const dir = new URL('../tests/fixtures/ignore/', import.meta.url);
for (const name of readdirSync(dir)) {
  test(`fixture ${name}`, () => {
    const fx = JSON.parse(readFileSync(new URL(name, dir), 'utf8'));
    const files = Object.entries(fx.files).map(([path, text]) => ({ path, text }));
    assert.deepEqual(filterFiles(files, ignore).kept, [...fx.kept].sort());
  });
}

test('entry and top dir', () => {
  assert.equal(entryFile(['a.md', 'index.md', 'index.html']), 'index.html');
  assert.equal(entryFile(['report.html', 'README.md']), null);
  assert.deepEqual(stripTopDir(['site/index.html', 'site/a/b.png']), ['index.html', 'a/b.png']);
  assert.deepEqual(stripTopDir(['site/index.html', 'x.png']), ['site/index.html', 'x.png']);
});
