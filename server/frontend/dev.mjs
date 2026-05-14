import * as esbuild from 'esbuild';

const ctx = await esbuild.context({
  entryPoints: ['src/showcase.ts'],
  bundle: true,
  format: 'esm',
  outdir: 'showcase-dist',
});

await ctx.watch();
const server = await ctx.serve({servedir: '.', port: 3001});
console.log(`Showcase: http://localhost:${server.port}/showcase.html`);
