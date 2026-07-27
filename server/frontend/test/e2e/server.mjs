import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:http';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { spawn } from 'node:child_process';

const root = await mkdtemp(join(tmpdir(), 'stride-web-e2e-'));
const port = Number(process.env.STRIDE_E2E_PORT ?? 3111);
const binary = resolve(process.env.STRIDE_E2E_SERVER_BIN ?? '../../target/debug/server');
const staticDir = resolve('dist');
const configPath = join(root, 'server.toml');
const databasePath = join(root, 'stride.db');
const filesPath = join(root, 'files');
const useRealModel = Boolean(process.env.STRIDE_OPENROUTER_API_KEY);
let modelServer;
let providerKind = 'OpenRouter';
let providerUrl = 'https://openrouter.ai/api/v1';

if (!useRealModel) {
  modelServer = createServer((request, response) => {
    if (request.method === 'POST' && request.url === '/v1/chat/completions') {
      response.writeHead(200, {
        'cache-control': 'no-cache',
        connection: 'keep-alive',
        'content-type': 'text/event-stream',
      });
      response.write(': connected\n\n');
      return;
    }
    response.writeHead(404);
    response.end();
  });
  await new Promise((resolveListen, rejectListen) => {
    modelServer.once('error', rejectListen);
    modelServer.listen(0, '127.0.0.1', resolveListen);
  });
  const address = modelServer.address();
  if (!address || typeof address === 'string') throw new Error('failed to start mock model server');
  providerKind = 'OpenAI';
  providerUrl = `http://127.0.0.1:${address.port}/v1`;
}

const config = `
[providers.openrouter]
kind = ${JSON.stringify(providerKind)}
url = ${JSON.stringify(providerUrl)}

[models.default]
slug = "nvidia/nemotron-3-ultra-550b-a55b:free"
display_name = "Nemotron 3 Ultra"
provider = "openrouter"
thinking = true
reasoning_effort = "high"
vision = false

[server]
db_path = ${JSON.stringify(databasePath)}
listen_addr = "127.0.0.1:${port}"
allow_registration = true
public_url = "http://127.0.0.1:${port}"

[server.files]
keep_versions = 3

[server.files.local]
enabled = true
base = ${JSON.stringify(filesPath)}
`;

await writeFile(configPath, config);

const child = spawn(binary, ['-c', configPath, '--static-dir', staticDir], {
  env: {
    ...process.env,
    STRIDE_JWT_SECRET:
      process.env.STRIDE_JWT_SECRET ??
      'stride-e2e-jwt-secret-is-local-and-disposable',
  },
  stdio: 'inherit',
});

let stopping = false;
async function stop(signal) {
  if (stopping) return;
  stopping = true;
  if (child.exitCode === null) child.kill(signal);
  modelServer?.closeAllConnections();
  if (modelServer) {
    await new Promise((resolveClose) => modelServer.close(resolveClose));
  }
  await rm(root, { recursive: true, force: true });
}

process.on('SIGINT', () => void stop('SIGINT'));
process.on('SIGTERM', () => void stop('SIGTERM'));

child.on('exit', async (code, signal) => {
  void rm(root, { recursive: true, force: true });
  if (signal) process.kill(process.pid, signal);
  process.exit(code ?? 1);
});

child.on('error', async (error) => {
  console.error(`failed to start ${binary}: ${error.message}`);
  await stop('SIGTERM');
  process.exit(1);
});
