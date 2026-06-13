import * as esbuild from 'esbuild';
import { existsSync, readdirSync, writeFileSync, mkdirSync, unlinkSync, realpathSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { basename, join, resolve } from 'node:path';

// Compile every Argon component to a JS module, then bundle them into
// dist/components.js — the shared, cacheable script every page loads.
const componentsDir = 'src/components';
const iconsDir = join(componentsDir, 'icons');
const argonOut = join(realpathSync(tmpdir()), 'friday-argon-js');
mkdirSync('dist', { recursive: true });
mkdirSync(argonOut, { recursive: true });
for (const stale of readdirSync(argonOut)) {
  unlinkSync(join(argonOut, stale));
}

const tsxIn = (dir) =>
  readdirSync(dir)
    .filter((f) => f.endsWith('.tsx'))
    .sort()
    .map((f) => join(dir, f));
const componentFiles = [...tsxIn(componentsDir), ...tsxIn(iconsDir)];

const result = spawnSync(
  './node_modules/.bin/argon',
  ['compile', ...componentFiles, '--js', '--out-dir', argonOut],
  { stdio: 'inherit' },
);
if (result.status !== 0) throw new Error('argon --js failed');

const entry = join(argonOut, 'components-entry.js');
writeFileSync(
  entry,
  readdirSync(argonOut)
    .filter((f) => f.endsWith('.js') && f !== 'components-entry.js')
    .sort()
    .map((f) => `import './${f}';`)
    .join('\n'),
);

// Compiled modules keep their source-relative imports (./icons/x.js,
// ../api/auth.js) but land flat in argonOut; map them back.
const argonImportsPlugin = {
  name: 'argon-imports',
  setup(build) {
    build.onResolve({ filter: /^\.\.?\// }, async (args) => {
      if (args.resolveDir !== argonOut || args.kind === 'entry-point') return;
      if (args.path.startsWith('./')) {
        const flat = join(argonOut, basename(args.path));
        if (existsSync(flat)) return { path: flat };
      }
      return build.resolve(args.path, {
        kind: args.kind,
        resolveDir: resolve(componentsDir),
      });
    });
  },
};

await Promise.all([
  esbuild.build({
    entryPoints: [entry],
    bundle: true,
    format: 'esm',
    minify: true,
    outfile: 'dist/components.js',
    plugins: [argonImportsPlugin],
  }),
  esbuild.build({
    entryPoints: ['src/style/index.css'],
    bundle: true,
    outfile: 'dist/common.css',
  }),
  esbuild.build({
    entryPoints: ['src/api/index.ts'],
    bundle: true,
    format: 'esm',
    minify: true,
    outfile: 'dist/api.js',
  }),
  esbuild.build({
    entryPoints: ['src/pages/threads-page.ts', 'src/pages/files-page.ts', 'src/pages/settings-page.ts'],
    bundle: true,
    format: 'esm',
    minify: true,
    outdir: 'dist/pages',
  }),
]);
