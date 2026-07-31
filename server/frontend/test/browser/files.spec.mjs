import { expect, test } from '@playwright/test';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const manifest = JSON.parse(readFileSync(new URL('../../dist/manifest.json', import.meta.url), 'utf8'));
const componentsBundle = fileURLToPath(new URL(`../../dist/${manifest.assets['components.js']}`, import.meta.url));

test('global files creates and opens folders through the generated browser component', async ({ page }) => {
  const requests = [];
  let archiveCreated = false;
  await page.route('http://stride.test/files', (route) =>
    route.fulfill({
      status: 200,
      contentType: 'text/html',
      body: '<app-file-explorer></app-file-explorer>',
    }));
  await page.route('http://stride.test/api/files**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    requests.push([request.method(), url.pathname, url.searchParams.get('path')]);

    if (request.method() === 'POST') {
      expect(request.postDataJSON()).toEqual({ path: '/home/user/Archive' });
      archiveCreated = true;
      await route.fulfill({ status: 204 });
      return;
    }

    const path = url.searchParams.get('path');
    const entries = path === '/home/user'
      ? [
          {
            name: 'Notes',
            path: '/home/user/Notes',
            kind: 'directory',
            size: null,
            updated_at: 1760000000000,
            mime_type: null,
          },
          ...(archiveCreated
            ? [{
                name: 'Archive',
                path: '/home/user/Archive',
                kind: 'directory',
                size: null,
                updated_at: 1760000000000,
                mime_type: null,
              }]
            : []),
        ]
      : [];
    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({ path, entries }),
    });
  });

  await page.goto('http://stride.test/files');
  await page.addScriptTag({ path: componentsBundle, type: 'module' });
  await expect(page.getByRole('button', { name: 'Notes' })).toBeVisible();

  page.once('dialog', (dialog) => dialog.accept('Archive'));
  await page.getByRole('button', { name: 'New folder' }).click();
  await expect(page.getByRole('button', { name: 'Archive' })).toBeVisible();
  await expect(page.getByText('Failed to create folder.')).toHaveCount(0);

  await page.getByRole('button', { name: 'Notes' }).click();
  await expect(page.getByText('/home/user/Notes', { exact: true })).toBeVisible();
  expect(requests).toEqual([
    ['GET', '/api/files', '/home/user'],
    ['POST', '/api/files/directories', null],
    ['GET', '/api/files', '/home/user'],
    ['GET', '/api/files', '/home/user/Notes'],
  ]);
});
