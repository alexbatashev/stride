import { createServer } from 'node:http';
import { watch } from 'node:fs';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = path.dirname(fileURLToPath(import.meta.url));
const host = process.env.HOST ?? '127.0.0.1';
const port = Number(process.env.PORT ?? 3001);
const reloadClients = new Set();
let buildPromise = null;
let buildVersion = 0;
let lastBuildStartedAt = 0;
let reloadTimer = null;

const contentTypes = {
  '.css': 'text/css; charset=utf-8',
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
};

const reloadScript = `
<script type="module">
const events = new EventSource('/__reload');
events.addEventListener('reload', () => location.reload());
</script>`;

function resolvePath(base, requestPath) {
  const filePath = path.resolve(base, `.${decodeURIComponent(requestPath)}`);
  return filePath.startsWith(`${base}${path.sep}`) ? filePath : null;
}

async function build() {
  buildVersion += 1;
  await import(`./build.mjs?version=${buildVersion}`);
}

async function rebuild() {
  if (buildPromise) {
    return;
  }

  lastBuildStartedAt = Date.now();
  buildPromise = build();

  try {
    await buildPromise;
    for (const client of reloadClients) {
      client.write('event: reload\ndata: ok\n\n');
    }
  } catch (error) {
    console.error(error);
  } finally {
    buildPromise = null;
  }
}

function queueRebuild() {
  if (Date.now() - lastBuildStartedAt < 500) {
    return;
  }

  clearTimeout(reloadTimer);
  reloadTimer = setTimeout(rebuild, 200);
}

function watchDirectory(directory) {
  watch(directory, { recursive: true }, (eventType, filename) => {
    if (!filename) {
      return;
    }

    queueRebuild();
  });
}

function watchFile(filePath) {
  watch(filePath, queueRebuild);
}

async function serveFile(res, filePath) {
  try {
    let body = await readFile(filePath);
    if (path.extname(filePath) === '.html') {
      body = Buffer.from(`${body.toString('utf8')}${reloadScript}`);
    }

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

    if (url.pathname === '/__reload') {
      res.writeHead(200, {
        'cache-control': 'no-cache',
        connection: 'keep-alive',
        'content-type': 'text/event-stream',
      });
      res.write('\n');
      reloadClients.add(res);
      req.on('close', () => reloadClients.delete(res));
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

await rebuild();
watchDirectory(path.join(root, 'src'));
watchFile(path.join(root, 'showcase.html'));

server.listen(port, host, () => {
  console.log(`Showcase: http://localhost:${port}/showcase.html`);
});
