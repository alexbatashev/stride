import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

await import('./build.mjs');

const root = path.dirname(fileURLToPath(import.meta.url));
const host = process.env.HOST ?? '127.0.0.1';
const port = Number(process.env.PORT ?? 3001);

const contentTypes = {
  '.css': 'text/css; charset=utf-8',
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
};

function resolvePath(base, requestPath) {
  const filePath = path.resolve(base, `.${decodeURIComponent(requestPath)}`);
  return filePath.startsWith(`${base}${path.sep}`) ? filePath : null;
}

async function serveFile(res, filePath) {
  try {
    const body = await readFile(filePath);
    res.writeHead(200, {
      'content-type': contentTypes[path.extname(filePath)] ?? 'application/octet-stream',
    });
    res.end(body);
  } catch (error) {
    if (error.code === 'ENOENT') {
      res.writeHead(404);
      res.end('Not found');
      return;
    }
    throw error;
  }
}

const server = createServer(async (req, res) => {
  try {
    const url = new URL(req.url, `http://${req.headers.host}`);

    if (url.pathname === '/' || url.pathname === '/showcase.html') {
      await serveFile(res, path.join(root, 'showcase.html'));
      return;
    }

    if (url.pathname.startsWith('/static/')) {
      const filePath = resolvePath(path.join(root, 'dist'), url.pathname.slice('/static'.length));
      if (filePath) {
        await serveFile(res, filePath);
        return;
      }
    }

    res.writeHead(404);
    res.end('Not found');
  } catch (error) {
    console.error(error);
    res.writeHead(500);
    res.end('Internal server error');
  }
});

server.listen(port, host, () => {
  console.log(`Showcase: http://localhost:${port}/showcase.html`);
});
