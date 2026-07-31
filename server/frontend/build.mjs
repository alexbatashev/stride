import * as esbuild from 'esbuild';
import { copyFileSync, mkdirSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { join } from 'node:path';

const vendorDir = 'dist/vendor';
mkdirSync('dist', { recursive: true });
mkdirSync(vendorDir, { recursive: true });

const result = spawnSync(
  './node_modules/.bin/argon',
  ['build'],
  { stdio: 'inherit' },
);
if (result.status !== 0) throw new Error('argon build failed');

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
    entryPoints: ['src/style/index.css'],
    bundle: true,
    outfile: 'dist/common.css',
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

copyFileSync('showcase.html', 'dist/index.html');
