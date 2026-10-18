// Copies the browser libraries into static/vendor. Run `npm run vendor` after changing versions,
// and keep the CDN versions in src/render.rs in step.
import { build } from 'esbuild';
import { cpSync, mkdirSync, readdirSync, rmSync } from 'node:fs';

const out = '../static/vendor';
rmSync(out, { recursive: true, force: true });
mkdirSync(`${out}/katex/fonts`, { recursive: true });

await build({ entryPoints: ['node_modules/ignore/index.js'], bundle: true, format: 'esm', minify: true, outfile: `${out}/ignore.mjs` });
cpSync('node_modules/mermaid/dist/mermaid.min.js', `${out}/mermaid.min.js`);
cpSync('node_modules/katex/dist/katex.min.js', `${out}/katex/katex.min.js`);
cpSync('node_modules/katex/dist/katex.min.css', `${out}/katex/katex.min.css`);
// Browsers that run mermaid all take woff2, the first source in katex.min.css.
for (const f of readdirSync('node_modules/katex/dist/fonts').filter((f) => f.endsWith('.woff2'))) {
  cpSync(`node_modules/katex/dist/fonts/${f}`, `${out}/katex/fonts/${f}`);
}
