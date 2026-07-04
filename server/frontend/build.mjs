import * as esbuild from 'esbuild';
import { existsSync, readdirSync, writeFileSync, mkdirSync, unlinkSync, realpathSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { tmpdir } from 'node:os';
import { basename, join, resolve } from 'node:path';

// Compile every Argon component to a JS module, then bundle them into
// dist/components.js — the shared, cacheable script every page loads.
const componentsDir = 'src/components';
const iconsDir = join(componentsDir, 'icons');
const argonOut = join(realpathSync(tmpdir()), 'stride-argon-js');
const vendorDir = 'dist/vendor';
mkdirSync('dist', { recursive: true });
mkdirSync(vendorDir, { recursive: true });
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

// Without an explicit target esbuild emits esnext (private class fields, etc.),
// which older mobile Safari/Chrome fail to parse — the whole bundle then never
// runs and every hydrated control (login button included) is dead. Downlevel to
// a baseline that covers phones a few years old.
const target = ['es2020'];
const vendorBuilds = [
  ['d3', 'd3'],
  ['@observablehq/plot', 'plot', 'Plot'],
  ['decimal.js', 'decimal', 'Decimal'],
  ['dagre', 'dagre', 'dagre'],
];

function buildVendor(entryPoint, filename, globalName) {
  return [
    esbuild.build({
      entryPoints: [entryPoint],
      bundle: true,
      format: 'esm',
      minify: true,
      target,
      outfile: join(vendorDir, `${filename}.js`),
    }),
    esbuild.build({
      entryPoints: [entryPoint],
      bundle: true,
      format: 'iife',
      globalName,
      minify: true,
      target,
      outfile: join(vendorDir, `${filename}.global.js`),
    }),
  ];
}

await Promise.all([
  esbuild.build({
    entryPoints: [entry],
    bundle: true,
    format: 'esm',
    minify: true,
    target,
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
    target,
    outfile: 'dist/api.js',
  }),
  esbuild.build({
    entryPoints: ['src/widget-frame.ts'],
    bundle: true,
    format: 'iife',
    minify: true,
    target,
    outfile: 'dist/widget-frame.js',
  }),
  ...vendorBuilds.flatMap(([entryPoint, filename, globalName = filename]) =>
    buildVendor(entryPoint, filename, globalName),
  ),
  esbuild.build({
    entryPoints: ['src/pages/threads-page.ts', 'src/pages/files-page.ts', 'src/pages/automations-page.ts', 'src/pages/settings-page.ts', 'src/pages/archived-page.ts'],
    bundle: true,
    format: 'esm',
    minify: true,
    target,
    outdir: 'dist/pages',
  }),
]);
