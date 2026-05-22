import * as esbuild from 'esbuild';
import { existsSync, unlinkSync } from 'node:fs';

const litEntry = [
  "export { CSSResult, LitElement, ReactiveElement, _$LE, _$LH, adoptStyles, css, defaultConverter, getCompatibleStyle, html, isServer, mathml, noChange, notEqual, nothing, render, supportsAdoptingStyleSheets, svg, unsafeCSS } from 'lit';",
  "export { literal, unsafeStatic, withStatic } from 'lit/static-html.js';",
  "export * from 'lit/async-directive.js';",
  "export * from 'lit/decorators.js';",
  "export * from 'lit/directive-helpers.js';",
  "export * from 'lit/directive.js';",
  "export * from 'lit/directives/async-append.js';",
  "export * from 'lit/directives/async-replace.js';",
  "export * from 'lit/directives/cache.js';",
  "export * from 'lit/directives/choose.js';",
  "export * from 'lit/directives/class-map.js';",
  "export * from 'lit/directives/guard.js';",
  "export * from 'lit/directives/if-defined.js';",
  "export * from 'lit/directives/join.js';",
  "export * from 'lit/directives/keyed.js';",
  "export * from 'lit/directives/live.js';",
  "export * from 'lit/directives/map.js';",
  "export * from 'lit/directives/range.js';",
  "export * from 'lit/directives/ref.js';",
  "export * from 'lit/directives/repeat.js';",
  "export * from 'lit/directives/style-map.js';",
  "export * from 'lit/directives/template-content.js';",
  "export * from 'lit/directives/unsafe-html.js';",
  "export * from 'lit/directives/unsafe-mathml.js';",
  "export * from 'lit/directives/unsafe-svg.js';",
  "export * from 'lit/directives/until.js';",
  "export * from 'lit/directives/when.js';",
  "import 'lit/polyfill-support.js';",
].join('\n');
for (const staleFile of ['dist/lit-decorators.js', 'dist/lit-entry.js']) {
  if (existsSync(staleFile)) {
    unlinkSync(staleFile);
  }
}

// When compiling pages, component imports are side-effect-only registrations
// that components.js already handles. Stub them out to avoid duplicate
// customElements.define calls.
const componentStubPlugin = {
  name: 'component-stub',
  setup(build) {
    build.onResolve({ filter: /^\.\.\/components\// }, args => ({
      path: args.path, namespace: 'stub',
    }));
    build.onLoad({ filter: /.*/, namespace: 'stub' }, () => ({
      contents: '', loader: 'js',
    }));
  },
};

const litExternalPlugin = {
  name: 'lit-external',
  setup(build) {
    build.onResolve({ filter: /^lit\/.+/ }, () => ({
      path: 'lit',
      external: true,
    }));
  },
};

await Promise.all([
  esbuild.build({
    entryPoints: ['src/style/index.css'],
    bundle: true,
    outfile: 'dist/common.css',
  }),
  esbuild.build({
    stdin: {
      contents: litEntry,
      resolveDir: '.',
      sourcefile: 'lit-entry.js',
    },
    bundle: true,
    format: 'esm',
    minify: true,
    outfile: 'dist/lit.js',
  }),
  esbuild.build({
    entryPoints: ['src/components/index.ts'],
    bundle: true,
    format: 'esm',
    external: ['lit'],
    minify: true,
    outfile: 'dist/components.js',
    plugins: [litExternalPlugin],
  }),
  esbuild.build({
    entryPoints: ['src/api/index.ts'],
    bundle: true,
    format: 'esm',
    minify: true,
    outfile: 'dist/api.js',
  }),
  esbuild.build({
    entryPoints: ['src/pages/auth-page.ts', 'src/pages/sample-page.ts', 'src/pages/threads-page.ts'],
    bundle: true,
    format: 'esm',
    external: ['lit'],
    minify: true,
    outdir: 'dist/pages',
    plugins: [componentStubPlugin, litExternalPlugin],
  }),
]);
