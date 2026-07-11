import * as esbuild from 'esbuild';
import { readdirSync, writeFileSync, mkdirSync, rmSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join, relative } from 'node:path';

// Compile every Argon component to a JS module, then bundle them into
// dist/components.js — the shared, cacheable script every page loads.
const componentsDir = 'src/components';
const iconsDir = join(componentsDir, 'icons');
const pagesDir = 'src/pages';
const sharedDir = 'src/shared';
const storesDir = 'src/stores';
const argonOut = 'dist/argon';
const vendorDir = 'dist/vendor';
mkdirSync('dist', { recursive: true });
mkdirSync(vendorDir, { recursive: true });
rmSync(argonOut, { recursive: true, force: true });
rmSync('dist/pages', { recursive: true, force: true });
mkdirSync(argonOut, { recursive: true });

const tsxIn = (dir) =>
  readdirSync(dir)
    .filter((f) => f.endsWith('.tsx'))
    .sort()
    .map((f) => join(dir, f));
const tsIn = (dir) =>
  readdirSync(dir)
    .filter((f) => f.endsWith('.ts'))
    .sort()
    .map((f) => join(dir, f));
const pageComponentFiles = tsxIn(pagesDir);
const componentFiles = [...tsxIn(componentsDir), ...tsxIn(iconsDir), ...pageComponentFiles];
const componentSupportFiles = [
  ...tsIn(componentsDir),
  ...tsIn(iconsDir),
  'src/pages/sidebar.ts',
  'src/pages/thread-actions.ts',
];
const storeFiles = readdirSync(storesDir)
  .filter((f) => f.endsWith('.ts'))
  .sort()
  .map((f) => join(storesDir, f));
const apiFiles = readdirSync('src/api')
  .filter((f) => f.endsWith('.ts'))
  .sort()
  .map((f) => join('src/api', f));

const result = spawnSync(
  './node_modules/.bin/argon',
  ['compile', ...storeFiles, ...componentFiles, '--js', '--out-dir', argonOut, '--root', 'src'],
  { stdio: 'inherit' },
);
if (result.status !== 0) throw new Error('argon --js failed');

const sharedResult = spawnSync(
  './node_modules/.bin/argon',
  ['compile', ...tsIn(sharedDir), '--shared', '--out-dir', argonOut, '--root', 'src'],
  { stdio: 'inherit' },
);
if (sharedResult.status !== 0) throw new Error('argon --shared failed');

const entry = join(argonOut, 'components-entry.js');
writeFileSync(
  entry,
  [...storeFiles, ...componentFiles]
    .map((file) => relative('src', file).replace(/\.tsx?$/, '.js'))
    .sort()
    .map((file) => `import './${file}';`)
    .join('\n'),
);

await esbuild.build({
  entryPoints: apiFiles,
  bundle: false,
  format: 'esm',
  outdir: join(argonOut, 'api'),
});

if (componentSupportFiles.length > 0) {
  await esbuild.build({
    entryPoints: componentSupportFiles,
    bundle: false,
    format: 'esm',
    outbase: 'src',
    outdir: argonOut,
  });
}

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
      outfile: join(vendorDir, `${filename}.js`),
    }),
    esbuild.build({
      entryPoints: [entryPoint],
      bundle: true,
      format: 'iife',
      globalName,
      minify: true,
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
    outfile: 'dist/components.js',
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
    entryPoints: ['src/widget-frame.ts'],
    bundle: true,
    format: 'iife',
    minify: true,
    outfile: 'dist/widget-frame.js',
  }),
  ...vendorBuilds.flatMap(([entryPoint, filename, globalName = filename]) =>
    buildVendor(entryPoint, filename, globalName),
  ),
]);
