import * as esbuild from 'esbuild';

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

await Promise.all([
  esbuild.build({
    entryPoints: ['src/style/index.css'],
    bundle: true,
    outfile: 'dist/common.css',
  }),
  esbuild.build({
    entryPoints: ['node_modules/lit/index.js'],
    bundle: true,
    format: 'esm',
    outfile: 'dist/lit.js',
  }),
  esbuild.build({
    entryPoints: ['src/components/index.ts'],
    bundle: true,
    format: 'esm',
    external: ['lit'],
    minify: true,
    outfile: 'dist/components.js',
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
    plugins: [componentStubPlugin],
  }),
]);
