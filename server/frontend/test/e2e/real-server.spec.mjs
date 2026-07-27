import { expect, test } from '@playwright/test';

test.describe.configure({ mode: 'serial' });

const username = `e2e_${Date.now()}`;
const password = 'correct horse battery staple';
const browserErrors = [];
const encounteredControls = new Set();
const verifiedControls = new Set();
let disposableThreadId = '';
let authCookie;
let suiteHadFailure = false;

function installControlCoverage() {
  const state = {
    encountered: new Set(JSON.parse(sessionStorage.getItem('stride-e2e-encountered') ?? '[]')),
    verified: new Set(JSON.parse(sessionStorage.getItem('stride-e2e-verified') ?? '[]')),
  };
  window.__strideControlCoverage = state;

  const interactiveSelector = [
    'button',
    'a[href]',
    'input:not([type="hidden"])',
    'select',
    'textarea',
    '[role="button"]',
    '[role="menuitem"]',
    '[role="tab"]',
    '[role="switch"]',
  ].join(',');

  function ownerPath(element) {
    const owners = [];
    let node = element;
    while (node && owners.length < 1) {
      const root = node.getRootNode();
      if (!(root instanceof ShadowRoot)) break;
      owners.push(root.host.localName);
      node = root.host;
    }
    return owners.join('>');
  }

  function normalize(value) {
    return String(value ?? '')
      .replace(/e2e_\d+/g, 'e2e_user')
      .replace(/[0-9a-f]{8}-[0-9a-f-]{27,}/gi, ':id')
      .replace(/\/threads\/[^/?#]+/g, '/threads/:id')
      .replace(/^Copied$/, 'Copy message')
      .replace(/^Working(?: for \d+s|…)$/, 'Working')
      .replace(/\s+/g, ' ')
      .trim()
      .slice(0, 100);
  }

  function signature(element) {
    const kind = element instanceof HTMLInputElement
      ? `input:${element.type}`
      : element.localName;
    const identityNodes = [element];
    let node = element;
    while (identityNodes.length < 4) {
      const root = node.getRootNode();
      if (!(root instanceof ShadowRoot)) break;
      identityNodes.push(root.host);
      node = root.host;
    }
    const identity = identityNodes
      .map((candidate) =>
        candidate.getAttribute('aria-label') ??
        candidate.getAttribute('name') ??
        candidate.getAttribute('data-section') ??
        candidate.getAttribute('data-tool') ??
        candidate.getAttribute('data-action') ??
        candidate.getAttribute('href') ??
        candidate.getAttribute('title') ??
        candidate.textContent ??
        candidate.className)
      .find((value) => normalize(value) !== '');
    return `${ownerPath(element)}|${kind}|${normalize(identity)}`;
  }

  function isUsable(element) {
    if (element.matches(':disabled,[hidden],[aria-hidden="true"]')) return false;
    if (element instanceof HTMLInputElement && element.readOnly) return false;
    if (
      element.localName.includes('-') &&
      element.shadowRoot?.querySelector(interactiveSelector)
    ) return false;
    if (typeof element.checkVisibility === 'function' && !element.checkVisibility()) return false;
    const style = getComputedStyle(element);
    if (
      style.display === 'none' ||
      style.visibility === 'hidden' ||
      style.pointerEvents === 'none' ||
      Number(style.opacity) === 0
    ) return false;
    const rect = element.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  }

  function visit(root) {
    for (const element of root.querySelectorAll('*')) {
      if (element.matches(interactiveSelector) && isUsable(element)) {
        state.encountered.add(signature(element));
      }
      if (element.shadowRoot) visit(element.shadowRoot);
    }
  }

  function save() {
    for (const signature of JSON.parse(sessionStorage.getItem('stride-e2e-encountered') ?? '[]')) {
      state.encountered.add(signature);
    }
    for (const signature of JSON.parse(sessionStorage.getItem('stride-e2e-verified') ?? '[]')) {
      state.verified.add(signature);
    }
    sessionStorage.setItem('stride-e2e-encountered', JSON.stringify([...state.encountered]));
    sessionStorage.setItem('stride-e2e-verified', JSON.stringify([...state.verified]));
  }

  function observeInteraction() {
    visit(document);
    save();
    window.setTimeout(() => visit(document), 0);
  }

  for (const eventName of ['click', 'input', 'change']) {
    document.addEventListener(eventName, observeInteraction, true);
  }
  state.signature = signature;
  state.leafControl = (element) =>
    element.localName.includes('-')
      ? element.shadowRoot?.querySelector(interactiveSelector) ?? element
      : element;
  state.markVerified = (controlSignature) => {
    state.verified.add(controlSignature);
    save();
  };
  window.setInterval(() => {
    visit(document);
    save();
  }, 50);
}

function observeBrowserErrors(page) {
  page.on('console', (message) => {
    if (message.type() === 'error') browserErrors.push(`console: ${message.text()}`);
  });
  page.on('pageerror', (error) => browserErrors.push(`page: ${error.message}`));
}

async function fillTextInput(page, name, value) {
  const input = page.locator(`app-text-input[data-name="${name}"] input`);
  await verifyFill(input, value, () => expect(input).toHaveValue(value));
}

async function assertHydratedPage(page, path, root) {
  const response = await page.goto(path);
  expect(response?.status()).toBe(200);
  await expect(page.locator(root)).toBeVisible();
  await expect(page.locator('app-sidebar')).toBeVisible();
}

async function verifyAction(locator, action, postcondition) {
  const page = locator.page();
  const signature = await locator.evaluate((element) => {
    const control = window.__strideControlCoverage.leafControl(element);
    return window.__strideControlCoverage.signature(control);
  });
  await action();
  await postcondition();
  await page.evaluate((verifiedSignature) => {
    window.__strideControlCoverage.markVerified(verifiedSignature);
  }, signature);
}

async function verifyClick(locator, postcondition, options) {
  await verifyAction(locator, () => locator.click(options), postcondition);
}

async function verifyFill(locator, value, postcondition) {
  await verifyAction(locator, () => locator.fill(value), postcondition);
}

test.beforeEach(async ({ page, context }, testInfo) => {
  await page.addInitScript(installControlCoverage);
  observeBrowserErrors(page);
  if (!testInfo.title.startsWith('registration and login')) {
    if (authCookie) {
      await context.addCookies([authCookie]);
      return;
    }
    let response = await page.request.post('/api/login', {
      data: { username, password },
    });
    if (response.status() === 401) {
      response = await page.request.post('/api/register', {
        data: { username, password },
      });
    }
    expect(response.status()).toBe(200);
    authCookie = (await context.cookies()).find((cookie) => cookie.name === 'token');
    expect(authCookie).toBeDefined();
  }
});

test.afterEach(async ({ page, context }, testInfo) => {
  if (testInfo.status !== testInfo.expectedStatus) suiteHadFailure = true;
  const tokenCookie = (await context.cookies()).find((cookie) => cookie.name === 'token');
  if (tokenCookie) authCookie = tokenCookie;
  const coverage = await page.evaluate(() => ({
    encountered: [...(window.__strideControlCoverage?.encountered ?? [])],
    verified: [...(window.__strideControlCoverage?.verified ?? [])],
  })).catch(() => ({ encountered: [], verified: [] }));
  for (const signature of coverage.encountered) encounteredControls.add(signature);
  for (const signature of coverage.verified) verifiedControls.add(signature);
});

test.afterAll(() => {
  expect(browserErrors, browserErrors.join('\n')).toEqual([]);
  const gaps = [...encounteredControls].filter((signature) => !verifiedControls.has(signature)).sort();
  if (!suiteHadFailure && !process.env.STRIDE_E2E_ALLOW_PARTIAL) {
    expect(
      gaps,
      `Controls without verified postconditions:\n${gaps.join('\n')}\n\nVerified controls:\n${[...verifiedControls].sort().join('\n')}`,
    ).toEqual([]);
  }
});

test('registration and login pages work through the real auth API', async ({ page }) => {
  await page.goto('/auth/login');
  await expect(page.locator('auth-form')).toBeVisible();
  await verifyClick(
    page.getByRole('link', { name: 'Register' }),
    () => expect(page).toHaveURL(/\/auth\/register$/),
  );
  await verifyClick(
    page.getByRole('link', { name: 'Log in' }),
    () => expect(page).toHaveURL(/\/auth\/login$/),
  );
  await verifyClick(
    page.getByRole('link', { name: 'Register' }),
    () => expect(page).toHaveURL(/\/auth\/register$/),
  );

  await fillTextInput(page, 'username', username);
  await fillTextInput(page, 'password', password);
  await verifyClick(page.getByRole('button', { name: 'Register' }), async () => {
    await expect(page).toHaveURL(/\/threads$/);
    await expect(page.locator('threads-page-view')).toBeVisible();
  });

  await verifyClick(
    page.locator('app-sidebar button.trigger'),
    () => expect(page.getByRole('menuitem', { name: 'Log out' })).toBeVisible(),
  );
  await verifyClick(
    page.getByRole('menuitem', { name: 'Log out' }),
    () => expect(page).toHaveURL(/\/auth\/login$/),
  );

  await fillTextInput(page, 'username', username);
  await fillTextInput(page, 'password', password);
  await verifyClick(
    page.getByRole('button', { name: 'Log in' }),
    () => expect(page).toHaveURL(/\/threads$/),
  );
});

test('task page exercises every locally available composer and panel control', async ({ page, context }) => {
  await page.goto('/threads');
  await expect(page.locator('app-prompt-input')).toBeVisible();

  await verifyClick(
    page.getByRole('button', { name: 'Choose model' }),
    () => expect(page.getByRole('listbox', { name: 'Models' })).toBeVisible(),
  );
  await verifyClick(page.getByRole('option', { name: /Nemotron 3 Ultra/ }), async () => {
    await expect(page.getByRole('listbox', { name: 'Models' })).toBeHidden();
    await expect(page.locator('app-prompt-input')).toHaveAttribute('data-selected-model', 'default');
  });

  await page.locator('app-prompt-input input[type="file"]').evaluate((input) => {
    input.addEventListener('click', () => {
      window.__strideFilePickerInvoked = true;
    });
  });
  await verifyClick(page.locator('app-button[aria-label="Add attachment"] button'), () =>
    expect.poll(() => page.evaluate(() => window.__strideFilePickerInvoked)).toBe(true));
  await page.locator('app-prompt-input input[type="file"]').setInputFiles({
    name: 'task-attachment.txt',
    mimeType: 'text/plain',
    buffer: Buffer.from('task attachment'),
  });
  const removeAttachment = page.locator(
    'app-button[aria-label="Remove task-attachment.txt"] button',
  );
  await expect(removeAttachment).toBeVisible();
  await verifyClick(
    removeAttachment,
    () => expect(removeAttachment).toHaveCount(0),
  );

  const prompt = page.getByRole('textbox', { name: 'Ask S.T.R.I.D.E. anything' });
  await verifyFill(
    prompt,
    'draft without sending',
    () => expect(page.locator('app-button[aria-label="Send message"] button')).toBeVisible(),
  );
  await verifyClick(page.locator('app-button[aria-label="Send message"] button'), async () => {
    await expect(page).toHaveURL(/\/threads\/[^/]+$/);
    await expect(page.locator('app-button[aria-label="Stop response"] button')).toBeVisible();
    disposableThreadId = new URL(page.url()).pathname.split('/').pop() ?? '';
    expect(disposableThreadId).not.toBe('');
  });
  const activeWork = page.locator('app-work-group button').first();
  await expect(activeWork).toBeVisible();
  const expanded = await activeWork.getAttribute('aria-expanded');
  await verifyClick(activeWork, () =>
    expect(activeWork).toHaveAttribute('aria-expanded', expanded === 'true' ? 'false' : 'true'));
  await verifyClick(page.locator('app-button[aria-label="Stop response"] button'), () =>
    expect(page.locator('app-button[aria-label="Stop response"] button')).toHaveCount(0));
  const localCopy = page.locator('app-button[aria-label="Copy message"] button').last();
  if (await localCopy.isVisible().catch(() => false)) {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
    await verifyClick(localCopy, async () => {
      await expect(page.locator('app-button[aria-label="Copied"] button')).toBeVisible();
      expect(await page.evaluate(() => navigator.clipboard.readText())).not.toBe('');
    });
  }

  await context.clearPermissions();
  await page.evaluate(() => {
    Object.defineProperty(navigator, 'mediaDevices', {
      configurable: true,
      value: { getUserMedia: async () => { throw new Error('denied'); } },
    });
  });
  await verifyClick(
    page.locator('app-button[aria-label="Record voice message"] button'),
    () => expect(page.locator('[data-error]')).toContainText('Microphone access was denied.'),
  );

  const sidePanel = page.locator('app-side-panel');
  await verifyClick(
    page.locator('app-button[data-action="side-panel-open"] button'),
    () => expect(sidePanel).toHaveAttribute('open', ''),
  );
  await verifyClick(
    page.getByRole('tab', { name: 'Subagents' }),
    () => expect(sidePanel).toHaveAttribute('data-active-tab', 'subagents'),
  );
  await verifyClick(
    page.getByRole('tab', { name: 'Files' }),
    () => expect(sidePanel).toHaveAttribute('data-active-tab', 'files'),
  );
  page.on('dialog', async (dialog) => {
    if (dialog.type() === 'prompt') await dialog.accept('task-folder');
    else await dialog.accept();
  });
  const threadFiles = sidePanel.locator('app-file-explorer');
  await verifyClick(threadFiles.getByRole('button', { name: 'Folder' }), () =>
    expect(threadFiles.getByRole('button', { name: 'task-folder', exact: true })).toBeVisible());
  await verifyClick(threadFiles.getByRole('button', { name: 'task-folder', exact: true }), () =>
    expect(threadFiles.locator('.path')).toContainText('task-folder'));
  await verifyClick(threadFiles.getByRole('button', { name: 'Up one level' }), () =>
    expect(threadFiles.locator('.path')).not.toContainText('task-folder'));
  const threadFileInput = threadFiles.locator('input[type="file"]');
  await threadFileInput.evaluate((input) => {
    input.addEventListener('click', () => {
      window.__strideThreadFilePickerInvoked = true;
    });
  });
  await verifyClick(threadFiles.getByRole('button', { name: 'Upload' }), () =>
    expect.poll(() => page.evaluate(() => window.__strideThreadFilePickerInvoked)).toBe(true));
  await threadFileInput.setInputFiles({
    name: 'thread-file.txt',
    mimeType: 'text/plain',
    buffer: Buffer.from('thread file'),
  });
  await expect(threadFiles.getByRole('button', { name: 'thread-file.txt', exact: true })).toBeVisible();
  await verifyClick(threadFiles.getByRole('button', { name: 'thread-file.txt', exact: true }), () =>
    expect(page.getByRole('dialog', { name: 'thread-file.txt' })).toBeVisible());
  await verifyClick(
    page.getByRole('dialog', { name: 'thread-file.txt' }).getByRole('button', { name: 'Close' }),
    () => expect(page.getByRole('dialog', { name: 'thread-file.txt' })).toBeHidden(),
  );
  await verifyClick(threadFiles.getByRole('button', { name: 'Actions for thread-file.txt' }), () =>
    expect(page.getByRole('menuitem', { name: 'Download' })).toBeVisible());
  await verifyAction(
    page.getByRole('menuitem', { name: 'Download' }),
    async () => {
      const [download] = await Promise.all([
        page.waitForEvent('download'),
        page.getByRole('menuitem', { name: 'Download' }).click(),
      ]);
      expect(download.suggestedFilename()).toBe('thread-file.txt');
    },
    () => expect(page.getByRole('menuitem', { name: 'Download' })).toHaveCount(0),
  );
  const threadSelectAll = threadFiles.getByRole('checkbox', { name: 'Select all rows' });
  await verifyAction(threadSelectAll, () => threadSelectAll.check(), () =>
    expect(threadFiles.getByRole('button', { name: 'Remove' })).toBeEnabled());
  await verifyClick(threadFiles.getByRole('button', { name: 'Remove' }), () =>
    expect(threadFiles.getByRole('button', { name: 'thread-file.txt', exact: true })).toHaveCount(0));
  await verifyClick(
    page.locator('app-button[data-action="side-panel-close"] button'),
    () => expect(sidePanel).not.toHaveAttribute('open', ''),
  );

});

test('task page controls create a task and stream a real model answer', async ({ page }) => {
  test.skip(
    !process.env.STRIDE_OPENROUTER_API_KEY,
    'STRIDE_OPENROUTER_API_KEY is required for the real model round-trip',
  );
  await page.goto('/threads');
  await expect(page.locator('app-prompt-input')).toBeVisible();

  const prompt = page.locator('app-prompt-input textarea');
  await verifyFill(
    prompt,
    'Reply with exactly E2E_OK and nothing else.',
    () => expect(page.locator('app-button[aria-label="Send message"] button')).toBeVisible(),
  );
  await verifyClick(page.locator('app-button[aria-label="Send message"] button'), async () => {
    await expect(page).toHaveURL(/\/threads\/[^/]+$/);
    await expect(page.locator('app-message').filter({ hasText: 'E2E_OK' })).toBeVisible({
      timeout: 80_000,
    });
    await page.reload();
    await expect(page.locator('app-message').filter({ hasText: 'E2E_OK' })).toBeVisible();
  });
});

test('files page exercises create, upload, navigate, select, rename, and remove', async ({ page }) => {
  await assertHydratedPage(page, '/files', 'app-file-browser');
  page.on('dialog', async (dialog) => {
    if (dialog.type() === 'prompt') {
      const value = dialog.message().startsWith('Folder') ? 'e2e-folder' : 'renamed.txt';
      await dialog.accept(value);
    } else {
      await dialog.accept();
    }
  });

  await verifyClick(page.getByRole('button', { name: 'New folder' }), async () => {
    await expect(page.getByText('e2e-folder', { exact: true })).toBeVisible();
    await page.reload();
    await expect(page.getByText('e2e-folder', { exact: true })).toBeVisible();
  });

  const fileInput = page.locator('app-file-explorer input[type="file"]');
  await fileInput.evaluate((input) => {
    input.addEventListener('click', () => {
      window.__strideGlobalFilePickerInvoked = true;
    });
  });
  await verifyClick(page.getByRole('button', { name: 'Upload' }), () =>
    expect.poll(() => page.evaluate(() => window.__strideGlobalFilePickerInvoked)).toBe(true));
  await fileInput.setInputFiles({
    name: 'e2e.txt',
    mimeType: 'text/plain',
    buffer: Buffer.from('stride e2e'),
  });
  await expect(page.getByText('e2e.txt', { exact: true })).toBeVisible();
  await fileInput.setInputFiles({
    name: 'e2e.txt',
    mimeType: 'text/plain',
    buffer: Buffer.from('stride e2e updated'),
  });
  await fileInput.setInputFiles({
    name: 'pixel.png',
    mimeType: 'image/png',
    buffer: Buffer.from(
      'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII=',
      'base64',
    ),
  });
  await expect(page.getByText('pixel.png', { exact: true })).toBeVisible();

  const selectAll = page.getByRole('checkbox', { name: 'Select all rows' });
  await verifyAction(selectAll, () => selectAll.check(), () =>
    expect(page.getByRole('button', { name: 'Remove' })).toBeEnabled());
  await selectAll.uncheck();
  await expect(page.getByRole('button', { name: 'Remove' })).toBeDisabled();

  await verifyClick(page.getByRole('button', { name: 'e2e-folder' }), () =>
    expect(page.locator('app-file-explorer .path')).toContainText('e2e-folder'));
  await verifyClick(page.getByRole('button', { name: 'Up one level' }), () =>
    expect(page.locator('app-file-explorer .path')).toContainText('/home/user'));

  await verifyClick(page.getByRole('button', { name: 'e2e.txt', exact: true }), () =>
    expect(page.getByRole('dialog', { name: 'e2e.txt' })).toBeVisible());
  const versionActions = page.getByRole('button', { name: 'Version actions' }).first();
  await verifyClick(versionActions, () =>
    expect(page.getByRole('menuitem', { name: 'Download' })).toBeVisible());
  await page.evaluate(() => {
    const originalFetch = window.fetch.bind(window);
    window.__strideVersionDownloadUrl = '';
    window.fetch = (input, init) => {
      const url = typeof input === 'string' ? input : input.url;
      if (url.includes('/api/files/') && url.includes('?version=')) {
        window.__strideVersionDownloadUrl = url;
      }
      return originalFetch(input, init);
    };
  });
  await verifyAction(
    page.getByRole('menuitem', { name: 'Download' }),
    () => page.getByRole('menuitem', { name: 'Download' }).click(),
    async () => {
      await expect.poll(() => page.evaluate(() => window.__strideVersionDownloadUrl)).toContain('?version=');
      await expect(page.getByRole('menuitem', { name: 'Download' })).toHaveCount(0);
    },
  );
  const versionDownloadUrl = await page.evaluate(() => window.__strideVersionDownloadUrl);
  const versionResponse = await page.request.get(versionDownloadUrl);
  expect(versionResponse.status()).toBe(200);
  expect(await versionResponse.text()).toBe('stride e2e updated');
  await verifyClick(page.getByRole('button', { name: 'Restore' }).last(), async () => {
    await expect(page.getByRole('dialog', { name: 'e2e.txt' })).toBeVisible();
    const response = await page.request.get('/api/files/home/user/e2e.txt');
    expect(response.status()).toBe(200);
    expect(await response.text()).toBe('stride e2e');
  });
  await verifyClick(page.getByRole('button', { name: 'Close' }), () =>
    expect(page.getByRole('dialog', { name: 'e2e.txt' })).toBeHidden());
  await verifyClick(page.getByRole('button', { name: 'pixel.png', exact: true }), () =>
    expect(page.getByRole('dialog', { name: 'pixel.png' })).toBeVisible());
  await verifyClick(page.getByRole('button', { name: 'Version actions' }).first(), () =>
    expect(page.getByRole('menuitem', { name: 'Preview' })).toBeVisible());
  await verifyClick(page.getByRole('menuitem', { name: 'Preview' }), () =>
    expect(page.getByRole('dialog', { name: /pixel\.png · version/ })).toBeVisible());
  await verifyClick(
    page.getByRole('dialog', { name: /pixel\.png · version/ }).getByRole('button', { name: 'Close' }),
    () => expect(page.getByRole('dialog', { name: /pixel\.png · version/ })).toBeHidden(),
  );
  await verifyClick(page.getByRole('dialog', { name: 'pixel.png' }).getByRole('button', { name: 'Close' }), () =>
    expect(page.getByRole('dialog', { name: 'pixel.png' })).toBeHidden());
  await verifyClick(page.getByRole('button', { name: 'Actions for pixel.png' }), () =>
    expect(page.getByRole('menuitem', { name: 'Download' })).toBeVisible());
  await verifyAction(
    page.getByRole('menuitem', { name: 'Download' }),
    async () => {
      const [download] = await Promise.all([
        page.waitForEvent('download'),
        page.getByRole('menuitem', { name: 'Download' }).click(),
      ]);
      expect(download.suggestedFilename()).toBe('pixel.png');
    },
    () => expect(page.getByRole('menuitem', { name: 'Download' })).toHaveCount(0),
  );

  await verifyClick(page.getByRole('button', { name: 'Actions for e2e.txt' }), () =>
    expect(page.getByRole('menuitem', { name: 'Download' })).toBeVisible());
  await verifyAction(
    page.getByRole('menuitem', { name: 'Download' }),
    async () => {
      const [download] = await Promise.all([
        page.waitForEvent('download'),
        page.getByRole('menuitem', { name: 'Download' }).click(),
      ]);
      expect(download.suggestedFilename()).toBe('e2e.txt');
    },
    () => expect(page.getByRole('menuitem', { name: 'Download' })).toHaveCount(0),
  );

  const originalRow = page.locator('app-data-table input[data-row-id$="/e2e.txt"]');
  await verifyAction(originalRow, () => originalRow.check(), () =>
    expect(page.getByRole('button', { name: 'Rename' })).toBeEnabled());
  await verifyClick(page.getByRole('button', { name: 'Rename' }), async () => {
    await expect(page.getByText('renamed.txt', { exact: true })).toBeVisible();
    await page.reload();
    await expect(page.getByText('renamed.txt', { exact: true })).toBeVisible();
    await expect(page.getByText('e2e.txt', { exact: true })).toHaveCount(0);
  });

  await verifyClick(page.getByRole('button', { name: 'renamed.txt', exact: true }), () =>
    expect(page.getByRole('dialog', { name: 'renamed.txt' })).toBeVisible());
  await verifyClick(page.getByRole('button', { name: 'Close' }), () =>
    expect(page.getByRole('dialog', { name: 'renamed.txt' })).toBeHidden());
  await verifyClick(page.getByRole('button', { name: 'Actions for renamed.txt' }), () =>
    expect(page.getByRole('menuitem', { name: 'Download' })).toBeVisible());
  await verifyAction(
    page.getByRole('menuitem', { name: 'Download' }),
    async () => {
      const [download] = await Promise.all([
        page.waitForEvent('download'),
        page.getByRole('menuitem', { name: 'Download' }).click(),
      ]);
      expect(download.suggestedFilename()).toBe('renamed.txt');
    },
    () => expect(page.getByRole('menuitem', { name: 'Download' })).toHaveCount(0),
  );

  const renamedRow = page.locator('app-data-table input[data-row-id$="/renamed.txt"]');
  await verifyAction(renamedRow, () => renamedRow.check(), () =>
    expect(page.getByRole('button', { name: 'Remove' })).toBeEnabled());
  await verifyClick(page.getByRole('button', { name: 'Remove' }), async () => {
    await expect(page.getByRole('button', { name: 'renamed.txt', exact: true })).toHaveCount(0);
    await page.reload();
    await expect(page.getByRole('button', { name: 'renamed.txt', exact: true })).toHaveCount(0);
  });
  await verifyAction(selectAll, () => selectAll.check(), () =>
    expect(page.getByRole('button', { name: 'Remove' })).toBeEnabled());
  await verifyClick(page.getByRole('button', { name: 'Remove' }), async () => {
    await expect(page.getByRole('button', { name: 'pixel.png', exact: true })).toHaveCount(0);
    await expect(page.getByRole('button', { name: 'e2e-folder', exact: true })).toHaveCount(0);
  });
});

test('automations page exercises every local row and modal action', async ({ page }) => {
  await assertHydratedPage(page, '/automations', 'app-automations');
  const newAutomation = page.getByRole('button', { name: 'New automation' });
  const modalHeading = page.getByRole('heading', { name: 'New automation' });

  await verifyClick(newAutomation, () => expect(modalHeading).toBeVisible());
  await verifyClick(page.getByRole('button', { name: 'Close' }), () =>
    expect(modalHeading).toBeHidden());

  await verifyClick(newAutomation, () => expect(modalHeading).toBeVisible());
  await verifyClick(page.getByRole('button', { name: 'Cancel' }), async () => {
    await expect(modalHeading).toBeHidden();
    await expect(page.getByRole('button', { name: 'Select E2E webhook' })).toHaveCount(0);
  });

  await verifyClick(newAutomation, () => expect(modalHeading).toBeVisible());
  const nameInput = page.locator('app-automations input[name="name"]');
  await verifyFill(nameInput, 'E2E webhook', () => expect(nameInput).toHaveValue('E2E webhook'));

  const trigger = page.locator('app-automations select[name="trigger"]');
  const schedule = page.locator('app-automations input[name="schedule"]');
  await verifyFill(schedule, '*/30 * * * *', () => expect(schedule).toHaveValue('*/30 * * * *'));
  await verifyAction(trigger, () => trigger.selectOption('vfs_change'), () =>
    expect(page.locator('app-automations input[name="watch_path"]')).toBeVisible());
  const watchPath = page.locator('app-automations input[name="watch_path"]');
  await verifyFill(watchPath, 'reports/', () => expect(watchPath).toHaveValue('reports/'));
  await trigger.selectOption('email');
  const emailAccount = page.locator('app-automations select[name="email_account"]');
  await verifyAction(emailAccount, () => emailAccount.selectOption(''), async () => {
    await expect(emailAccount).toHaveValue('');
    await expect(emailAccount.locator('option')).toHaveCount(1);
  });
  await trigger.selectOption('gmail');
  await expect(page.getByText('Connect Google in Settings first.', { exact: false })).toBeVisible();
  await trigger.selectOption('manual');
  await expect(schedule).toHaveAttribute('type', 'hidden');
  await trigger.selectOption('webhook');
  await expect(trigger).toHaveValue('webhook');

  const kind = page.locator('app-automations select[name="kind"]');
  await verifyAction(kind, () => kind.selectOption('python'), () => expect(kind).toHaveValue('python'));
  const notify = page.locator('app-automations select[name="notify"]');
  await verifyAction(notify, () => notify.selectOption('telegram'), () => expect(notify).toHaveValue('telegram'));
  await notify.selectOption('none');

  const enabled = page.locator('app-automations input[name="enabled"]');
  await verifyAction(enabled, () => enabled.uncheck(), () => expect(enabled).not.toBeChecked());
  await enabled.check();

  const payload = page.locator('app-automations textarea[name="payload"]');
  await verifyFill(
    payload,
    'print("E2E_AUTOMATION_OK")',
    () => expect(payload).toHaveValue('print("E2E_AUTOMATION_OK")'),
  );
  await verifyClick(page.getByRole('button', { name: 'Create automation' }), async () => {
    await expect(page.getByRole('heading', { name: 'Webhook created' })).toBeVisible();
    await expect(page.locator('app-automations input.secret')).toHaveCount(2);
  });
  await verifyClick(page.getByRole('button', { name: 'Close' }), async () => {
    await expect(page.getByRole('heading', { name: 'Webhook created' })).toBeHidden();
    await expect(page.getByRole('button', { name: 'Select E2E webhook' })).toBeVisible();
  });

  await verifyClick(page.getByRole('button', { name: 'Select E2E webhook' }), () =>
    expect(page.getByRole('heading', { name: 'E2E webhook' })).toBeVisible());
  await verifyClick(page.getByRole('button', { name: 'Pause E2E webhook' }), () =>
    expect(page.getByRole('button', { name: 'Enable E2E webhook' })).toBeVisible());
  await verifyClick(page.getByRole('button', { name: 'Enable E2E webhook' }), () =>
    expect(page.getByRole('button', { name: 'Pause E2E webhook' })).toBeVisible());
  await verifyClick(page.getByRole('button', { name: 'Run E2E webhook' }), () =>
    expect(page.getByText('python execution is disabled', { exact: false })).toBeVisible({
      timeout: 30_000,
    }));

  page.once('dialog', (dialog) => dialog.accept());
  await verifyClick(page.getByRole('button', { name: 'Delete E2E webhook' }), async () => {
    await expect(page.getByRole('button', { name: 'Select E2E webhook' })).toHaveCount(0);
    await page.reload();
    await expect(page.getByRole('button', { name: 'Select E2E webhook' })).toHaveCount(0);
  });
});

test('settings controls persist local changes and report external boundary failures', async ({ page }) => {
  await assertHydratedPage(page, '/settings', 'app-settings');
  const openSection = async (tab) => {
    const section = tab === 'MCP servers'
      ? 'mcp'
      : tab === 'Writable folders'
        ? 'files'
        : tab.toLowerCase();
    await verifyClick(
      page.locator('app-settings').getByRole('button', { name: tab, exact: true }),
      () => expect(page.locator('app-settings .layout')).toHaveAttribute('data-active', section),
    );
    await page.waitForTimeout(50);
  };

  const personal = page.locator('app-settings-personal');
  const fullName = personal.locator('input[name="full-name"]');
  const personality = personal.locator('textarea[name="personality"]');
  await verifyFill(fullName, 'E2E User', () => expect(fullName).toHaveValue('E2E User'));
  await verifyFill(personality, 'Prefer exact semantic UI checks.', () =>
    expect(personality).toHaveValue('Prefer exact semantic UI checks.'));
  await verifyClick(personal.getByRole('button', { name: 'Save changes' }), () =>
    expect(personal.getByText('Saved.', { exact: true })).toBeVisible());
  await assertHydratedPage(page, '/settings', 'app-settings');
  await expect(fullName).toHaveValue('E2E User');
  await expect(personality).toHaveValue('Prefer exact semantic UI checks.');

  await openSection('Connections');
  const github = page.locator('app-settings-github');
  const githubToken = github.locator('input[name="token"]');
  await verifyFill(githubToken, '', () => expect(githubToken).toHaveValue(''));
  await verifyClick(github.getByRole('button', { name: 'Connect with token' }), () =>
    expect(github.getByText('Enter a personal access token.', { exact: true })).toBeVisible());

  await openSection('Email');
  const email = page.locator('app-settings-email');
  for (const [name, value] of [
    ['name', 'E2E mail'],
    ['email', 'e2e@example.test'],
    ['host', '127.0.0.1'],
    ['port', '1'],
    ['username', 'e2e@example.test'],
    ['password', 'not-a-real-password'],
  ]) {
    const input = email.locator(`input[name="${name}"]`);
    await verifyFill(input, value, () => expect(input).toHaveValue(value));
  }
  const errorCountBeforeImap = browserErrors.length;
  await verifyClick(email.getByRole('button', { name: 'Add account' }), () =>
    expect(email.locator('.error')).toContainText('IMAP connection failed'));
  expect(browserErrors.slice(errorCountBeforeImap)).toEqual([
    'console: Failed to load resource: the server responded with a status of 400 (Bad Request)',
  ]);
  browserErrors.splice(errorCountBeforeImap);

  await openSection('MCP servers');
  const mcp = page.locator('app-settings-mcp');
  for (const [name, value] of [
    ['name', 'E2E_Mcp'],
    ['url', 'https://example.test/mcp'],
    ['bearer_token', 'e2e-bearer'],
  ]) {
    const input = mcp.locator(`input[name="${name}"]`);
    await verifyFill(input, value, () => expect(input).toHaveValue(value));
  }
  const headers = mcp.locator('textarea[name="headers_json"]');
  await verifyFill(headers, '{"X-E2E":"yes"}', () => expect(headers).toHaveValue('{"X-E2E":"yes"}'));
  await verifyClick(mcp.getByRole('button', { name: 'Add server' }), async () => {
    await expect(mcp.getByText('E2E_Mcp', { exact: true })).toBeVisible();
    await assertHydratedPage(page, '/settings', 'app-settings');
    await openSection('MCP servers');
    await expect(mcp.getByText('E2E_Mcp', { exact: true })).toBeVisible();
  });
  page.once('dialog', (dialog) => dialog.accept());
  await verifyClick(mcp.getByRole('button', { name: 'Remove' }), () =>
    expect(mcp.getByText('E2E_Mcp', { exact: true })).toHaveCount(0));

  await openSection('Writable folders');
  const writable = page.locator('app-settings-files');
  for (const path of ['/home/user/E2E', '/home/user/E2E/Notes']) {
    const response = await page.request.post('/api/files/directories', { data: { path } });
    expect(response.status()).toBe(204);
  }
  const folderPath = writable.locator('input[name="path"]');
  await verifyFill(folderPath, 'E2E/Notes', () => expect(folderPath).toHaveValue('E2E/Notes'));
  await verifyClick(writable.getByRole('button', { name: 'Add folder' }), async () => {
    await expect(writable.getByText('/home/user/E2E/Notes', { exact: true })).toBeVisible();
    await assertHydratedPage(page, '/settings', 'app-settings');
    await openSection('Writable folders');
    await expect(writable.getByText('/home/user/E2E/Notes', { exact: true })).toBeVisible();
  });
  page.once('dialog', (dialog) => dialog.accept());
  await verifyClick(writable.getByRole('button', { name: 'Remove' }), () =>
    expect(writable.getByText('/home/user/E2E/Notes', { exact: true })).toHaveCount(0));
  for (const path of ['/home/user/E2E/Notes', '/home/user/E2E']) {
    const response = await page.request.delete(`/api/files/${path.slice(1)}`);
    expect(response.status()).toBe(204);
  }

  await openSection('Memories');
  const memory = page.locator('app-settings-memory');
  const memoryQuery = memory.locator('input[aria-label="Search memories"]');
  await verifyFill(memoryQuery, 'no-e2e-memory', () =>
    expect(memory.getByText('No memories match this search.', { exact: true })).toBeVisible());
  await verifyClick(memory.getByRole('button', { name: 'Refresh' }), () =>
    expect(memory.getByText('Showing saved memory structure.', { exact: true })).toBeVisible());

  await openSection('Skills');
  const skills = page.locator('app-settings-skills');
  for (const [name, value] of [
    ['name', 'e2e-skill'],
    ['description', 'A disposable end-to-end skill.'],
  ]) {
    const input = skills.locator(`input[name="${name}"]`);
    await verifyFill(input, value, () => expect(input).toHaveValue(value));
  }
  const skillContent = skills.locator('textarea[name="content"]');
  await verifyFill(skillContent, '# E2E\nVerify the real UI.', () =>
    expect(skillContent).toHaveValue('# E2E\nVerify the real UI.'));
  await verifyClick(skills.getByRole('button', { name: 'Add skill' }), async () => {
    await expect(skills.getByText('e2e-skill', { exact: true })).toBeVisible();
    await assertHydratedPage(page, '/settings', 'app-settings');
    await openSection('Skills');
    await expect(skills.getByText('e2e-skill', { exact: true })).toBeVisible();
  });
  await verifyClick(skills.getByRole('button', { name: 'Edit' }), () =>
    expect(skills.getByText('Edit skill', { exact: true })).toBeVisible());
  await verifyClick(skills.getByRole('button', { name: 'Cancel' }), () =>
    expect(skills.getByText('Edit skill', { exact: true })).toHaveCount(0));
  await verifyClick(skills.getByRole('button', { name: 'Edit' }), () =>
    expect(skills.getByText('Edit skill', { exact: true })).toBeVisible());
  const editDescription = skills.locator('input[name="description"]');
  await verifyFill(editDescription, 'Updated end-to-end skill.', () =>
    expect(editDescription).toHaveValue('Updated end-to-end skill.'));
  await verifyClick(skills.getByRole('button', { name: 'Save changes' }), async () => {
    await expect(skills.getByText('Updated end-to-end skill.', { exact: false })).toBeVisible();
    await assertHydratedPage(page, '/settings', 'app-settings');
    await openSection('Skills');
    await expect(skills.getByText('Updated end-to-end skill.', { exact: false })).toBeVisible();
  });
  page.once('dialog', (dialog) => dialog.accept());
  await verifyClick(skills.getByRole('button', { name: 'Remove' }), () =>
    expect(skills.getByText('e2e-skill', { exact: true })).toHaveCount(0));

  await openSection('Models');
  const models = page.locator('app-settings-models');
  for (const [name, value] of [
    ['name', 'e2e_provider'],
    ['url', 'https://example.test/v1'],
    ['token', 'e2e-token'],
  ]) {
    const input = models.locator(`form:has(app-button:has-text("Add provider")) input[name="${name}"]`);
    await verifyFill(input, value, () => expect(input).toHaveValue(value));
  }
  const providerKind = models.locator('select[name="kind"]');
  await verifyAction(providerKind, () => providerKind.selectOption('openrouter'), () =>
    expect(providerKind).toHaveValue('openrouter'));
  await verifyClick(models.getByRole('button', { name: 'Add provider' }), () =>
    expect(models.locator('.model-item .name').filter({ hasText: /^e2e_provider$/ })).toBeVisible());

  const modelForm = models.locator('form:has(app-button:has-text("Add model"))');
  for (const [name, value] of [
    ['name', 'e2e_model'],
    ['display_name', 'E2E Model'],
    ['slug', 'e2e/model'],
  ]) {
    const input = modelForm.locator(`input[name="${name}"]`);
    await verifyFill(input, value, () => expect(input).toHaveValue(value));
  }
  const description = modelForm.locator('textarea[name="description"]');
  await verifyFill(description, 'E2E model description', () =>
    expect(description).toHaveValue('E2E model description'));
  const provider = modelForm.locator('select[name="provider_id"]');
  await verifyAction(provider, () => provider.selectOption({ label: 'e2e_provider' }), () =>
    expect(provider).not.toHaveValue(''));
  const reasoning = modelForm.locator('select[name="reasoning_effort"]');
  await verifyAction(reasoning, () => reasoning.selectOption('high'), () =>
    expect(reasoning).toHaveValue('high'));
  const vision = modelForm.locator('app-checkbox[name="vision"] button');
  await verifyClick(vision, () => expect(vision).toHaveAttribute('aria-checked', 'true'));
  await verifyClick(models.getByRole('button', { name: 'Add model' }), () =>
    expect(models.locator('.model-item .name').filter({ hasText: /^E2E Model$/ })).toBeVisible());

  const allowedModel = models.locator('app-checkbox[data-model="e2e_model"] button');
  await verifyClick(allowedModel, async () => {
    await models.getByRole('button', { name: 'Save agent settings' }).click();
    await expect(models.getByText('Saved.', { exact: true })).toBeVisible();
    const response = await page.request.get('/api/settings/agent');
    expect(response.status()).toBe(200);
    expect((await response.json()).subagent_allowed_models).not.toContain('e2e_model');
  });
  const guidelines = models.locator('textarea[name="subagent-guidelines"]');
  await verifyFill(guidelines, 'Use E2E Model for browser verification.', () =>
    expect(guidelines).toHaveValue('Use E2E Model for browser verification.'));
  await verifyClick(models.getByRole('button', { name: 'Save agent settings' }), async () => {
    await expect(models.getByText('Saved.', { exact: true })).toBeVisible();
    await assertHydratedPage(page, '/settings', 'app-settings');
    await openSection('Models');
    await expect(guidelines).toHaveValue('Use E2E Model for browser verification.');
  });
  page.once('dialog', (dialog) => dialog.accept());
  await verifyClick(models.locator('.model-item').filter({ hasText: 'E2E Model' }).getByRole('button', { name: 'Remove' }), () =>
    expect(models.locator('.model-item .name').filter({ hasText: /^E2E Model$/ })).toHaveCount(0));
  page.once('dialog', (dialog) => dialog.accept());
  await verifyClick(models.locator('.model-item').filter({ hasText: 'e2e_provider' }).getByRole('button', { name: 'Remove' }), () =>
    expect(models.getByText('e2e_provider', { exact: true })).toHaveCount(0));

  await openSection('Threads');
  const threads = page.locator('app-settings-threads');
  const switches = threads.locator('app-switch button');
  for (let index = 0; index < 2; index += 1) {
    const toggle = switches.nth(index);
    const before = await toggle.getAttribute('aria-checked');
    await verifyClick(toggle, () =>
      expect(toggle).toHaveAttribute('aria-checked', before === 'true' ? 'false' : 'true'));
    await verifyClick(toggle, () => expect(toggle).toHaveAttribute('aria-checked', before));
  }
  const archiveDays = threads.locator('input[name="archive-days"]');
  const removeDays = threads.locator('input[name="remove-days"]');
  await verifyFill(archiveDays, '21', () => expect(archiveDays).toHaveValue('21'));
  await verifyFill(removeDays, '120', () => expect(removeDays).toHaveValue('120'));
  await verifyClick(threads.getByRole('button', { name: 'Save changes' }), async () => {
    await expect(threads.getByText('Saved.', { exact: true })).toBeVisible();
    await assertHydratedPage(page, '/settings', 'app-settings');
    await openSection('Threads');
    await expect(archiveDays).toHaveValue('21');
    await expect(removeDays).toHaveValue('120');
  });

  await openSection('Personal');
  await expect(fullName).toHaveValue('E2E User');

  await assertHydratedPage(page, '/archived', 'app-archived-threads');
  await assertHydratedPage(page, '/threads', 'threads-page-view');
});

test('sidebar links and account controls are wired on the real shell', async ({ page, context }) => {
  await page.goto('/threads');
  for (const [label, target] of [
    ['Files', /\/files$/],
    ['Automations', /\/automations$/],
    ['New task', /\/threads$/],
  ]) {
    await page.goto('/threads');
    await verifyClick(
      page.getByRole('link', { name: label, exact: true }),
      () => expect(page).toHaveURL(target),
    );
  }

  await page.goto('/files');
  await verifyClick(page.getByRole('button', { name: 'skills', exact: true }), () =>
    expect(page.locator('app-file-explorer .path')).toContainText('/home/user/skills'));
  await verifyClick(page.getByRole('button', { name: 'Up one level' }), () =>
    expect(page.locator('app-file-explorer .path')).toContainText('/home/user'));
  for (const [label, target] of [
    ['New task', /\/threads$/],
    ['Automations', /\/automations$/],
    ['Files', /\/files$/],
  ]) {
    await page.goto('/files');
    await verifyClick(
      page.getByRole('link', { name: label, exact: true }),
      () => expect(page).toHaveURL(target),
    );
  }

  await page.goto('/threads');
  const threadSidebarPanel = page.locator('app-sidebar-panel');
  const brandToggle = page.locator('app-sidebar app-sidebar-toggle button');
  await verifyClick(page.getByRole('button', { name: 'Toggle sidebar' }), () =>
    expect(threadSidebarPanel).toHaveAttribute('state', 'collapsed'));
  await verifyClick(brandToggle, () =>
    expect(threadSidebarPanel).toHaveAttribute('state', 'open'));

  const account = page.locator('app-sidebar button.trigger');
  await verifyClick(account, () =>
    expect(page.getByRole('menuitem', { name: 'Settings' })).toBeVisible());
  await verifyClick(page.getByRole('menuitem', { name: 'Settings' }), () =>
    expect(page.getByRole('dialog', { name: 'Settings' })).toBeVisible());
  await verifyClick(page.getByRole('button', { name: 'Close' }), () =>
    expect(page.getByRole('dialog', { name: 'Settings' })).toBeHidden());
  await verifyClick(account, () =>
    expect(page.getByRole('menuitem', { name: 'Archived' })).toBeVisible());
  await verifyClick(page.getByRole('menuitem', { name: 'Archived' }), () =>
    expect(page).toHaveURL(/\/archived$/));
  await page.goto('/threads');

  page.once('dialog', (dialog) => dialog.accept('E2E Project'));
  await verifyClick(page.getByRole('button', { name: 'New project' }), () =>
    expect(page.getByText('E2E Project', { exact: true })).toBeVisible());
  const projectGroup = page.locator('sidebar-thread-group').filter({ hasText: 'E2E Project' });
  const projectLabel = projectGroup.getByRole('button', { name: 'E2E Project', exact: true });
  const projectSection = projectGroup.locator('app-sidebar-group');
  await verifyClick(projectLabel, () => expect(projectSection).toHaveClass(/closed/));
  await verifyClick(projectLabel, () => expect(projectSection).not.toHaveClass(/closed/));
  await projectLabel.hover();
  await verifyClick(projectGroup.getByTitle('New thread'), () =>
    expect(page).toHaveURL(/\/threads\?project=/));

  await projectLabel.hover();
  page.once('dialog', (dialog) => dialog.accept('E2E Project Renamed'));
  await verifyClick(projectGroup.getByTitle('Rename'), async () => {
    await expect(page.getByText('E2E Project Renamed', { exact: true })).toBeVisible();
    await page.reload();
    await expect(page.getByText('E2E Project Renamed', { exact: true })).toBeVisible();
  });
  const renamedProjectGroup = page.locator('sidebar-thread-group').filter({ hasText: 'E2E Project Renamed' });
  const renamedProjectLabel = renamedProjectGroup.getByRole('button', { name: 'E2E Project Renamed', exact: true });
  await verifyClick(renamedProjectLabel, () =>
    expect(renamedProjectGroup.locator('app-sidebar-group')).toHaveClass(/closed/));
  await verifyClick(renamedProjectLabel, () =>
    expect(renamedProjectGroup.locator('app-sidebar-group')).not.toHaveClass(/closed/));
  await renamedProjectLabel.hover();
  page.once('dialog', (dialog) => dialog.accept());
  await verifyClick(renamedProjectGroup.getByTitle('Delete'), async () => {
    await expect(page.getByText('E2E Project Renamed', { exact: true })).toHaveCount(0);
    await page.reload();
    await expect(page.getByText('E2E Project Renamed', { exact: true })).toHaveCount(0);
  });

  const threadId = disposableThreadId;
  expect(threadId).not.toBe('');
  await page.goto(`/threads/${threadId}`);
  const threadLink = page.locator(`a[href="/threads/${threadId}"]`);
  await expect(threadLink).toBeVisible();
  const defaultThreadGroup = page.locator('sidebar-thread-group').filter({ hasText: /^Threads/ });
  const defaultThreadLabel = defaultThreadGroup.getByRole('button', { name: 'Threads', exact: true });
  await verifyClick(defaultThreadLabel, () =>
    expect(defaultThreadGroup.locator('app-sidebar-group')).toHaveClass(/closed/));
  await verifyClick(defaultThreadLabel, () =>
    expect(defaultThreadGroup.locator('app-sidebar-group')).not.toHaveClass(/closed/));
  const copy = page.locator('app-button[aria-label="Copy message"] button').last();
  if (await copy.isVisible().catch(() => false)) {
    await context.grantPermissions(['clipboard-read', 'clipboard-write']);
    await verifyClick(copy, async () => {
      await expect(page.locator('app-button[aria-label="Copied"] button')).toBeVisible();
      expect(await page.evaluate(() => navigator.clipboard.readText())).not.toBe('');
    });
  }

  const newThreadRow = page.locator('app-sidebar-menu-item').filter({
    has: threadLink,
  });
  await newThreadRow.hover();
  const threadActions = newThreadRow.getByTitle('Thread actions');
  await verifyClick(threadActions, () =>
    expect(page.getByRole('menuitem', { name: 'Rename' })).toBeVisible());
  await verifyClick(page.getByRole('menuitem', { name: 'Rename' }), () =>
    expect(page.getByRole('dialog', { name: 'Rename thread' })).toBeVisible());
  const titleInput = page.getByRole('textbox', { name: 'Title' });
  await verifyFill(titleInput, 'E2E renamed thread', () =>
    expect(titleInput).toHaveValue('E2E renamed thread'));
  await verifyClick(page.getByRole('button', { name: 'Save' }), async () => {
    await expect(page.getByRole('link', { name: 'E2E renamed thread' })).toBeVisible();
    await page.reload();
    await expect(page.getByRole('link', { name: 'E2E renamed thread' })).toBeVisible();
  });
  let renamedThreadRow = page.locator('app-sidebar-menu-item').filter({
    has: page.getByRole('link', { name: 'E2E renamed thread', exact: true }),
  });
  await renamedThreadRow.hover();
  await verifyClick(renamedThreadRow.getByTitle('Thread actions'), () =>
    expect(page.getByRole('menuitem', { name: 'Rename' })).toBeVisible());
  await verifyClick(page.getByRole('menuitem', { name: 'Rename' }), () =>
    expect(page.getByRole('dialog', { name: 'Rename thread' })).toBeVisible());
  await verifyClick(page.getByRole('button', { name: 'Cancel' }), () =>
    expect(page.getByRole('dialog', { name: 'Rename thread' })).toBeHidden());

  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto(`/threads/${threadId}`);
  const mobilePanel = page.locator('app-sidebar-panel');
  await verifyClick(page.locator('threads-page-view header app-sidebar-toggle button'), () =>
    expect(mobilePanel).toHaveAttribute('state', 'open'));
  await verifyClick(
    page.locator('app-sidebar button.scrim'),
    () => expect(mobilePanel).toHaveAttribute('state', 'hidden'),
    { position: { x: 380, y: 400 } },
  );
  await verifyClick(page.locator('threads-page-view header app-sidebar-toggle button'), () =>
    expect(mobilePanel).toHaveAttribute('state', 'open'));
  await verifyClick(page.locator('app-sidebar app-sidebar-toggle button'), () =>
    expect(mobilePanel).toHaveAttribute('state', 'hidden'));
  const mobileThreadActions = page.locator('app-button[data-action="thread-menu"] button');
  await verifyClick(mobileThreadActions, () =>
    expect(page.getByRole('menuitem', { name: 'Files' })).toBeVisible());
  await verifyClick(page.getByRole('menuitem', { name: 'Files' }), () =>
    expect(page.getByRole('dialog', { name: 'Files' })).toBeVisible());
  await verifyClick(page.getByRole('dialog', { name: 'Files' }).getByRole('button', { name: 'Close' }), () =>
    expect(page.getByRole('dialog', { name: 'Files' })).toBeHidden());
  await verifyClick(mobileThreadActions, () =>
    expect(page.getByRole('menuitem', { name: 'Subagents' })).toBeVisible());
  await verifyClick(page.getByRole('menuitem', { name: 'Subagents' }), () =>
    expect(page.getByRole('dialog', { name: 'Subagents' })).toBeVisible());
  await verifyClick(page.getByRole('dialog', { name: 'Subagents' }).getByRole('button', { name: 'Close' }), () =>
    expect(page.getByRole('dialog', { name: 'Subagents' })).toBeHidden());
  await page.setViewportSize({ width: 1280, height: 720 });
  await page.goto(`/threads/${threadId}`);
  await verifyClick(page.getByRole('link', { name: 'E2E renamed thread', exact: true }), () =>
    expect(page).toHaveURL(new RegExp(`/threads/${threadId}$`)));

  renamedThreadRow = page.locator('app-sidebar-menu-item').filter({
    has: page.getByRole('link', { name: 'E2E renamed thread', exact: true }),
  });
  await renamedThreadRow.hover();
  await verifyClick(renamedThreadRow.getByTitle('Thread actions'), () =>
    expect(page.getByRole('menuitem', { name: 'Archive' })).toBeVisible());
  await verifyClick(page.getByRole('menuitem', { name: 'Archive' }), async () => {
    await expect(page).toHaveURL(/\/threads$/);
    await expect(page.getByRole('link', { name: 'E2E renamed thread' })).toHaveCount(0);
  });

  await page.goto('/archived');
  const archivedRow = page.locator('app-archived-threads .row').filter({ hasText: 'E2E renamed thread' });
  await expect(archivedRow).toBeVisible();
  await verifyClick(page.getByRole('button', { name: 'Thread actions' }), () =>
    expect(page.getByRole('menuitem', { name: 'Unarchive' })).toBeVisible());
  await verifyClick(page.getByRole('menuitem', { name: 'Unarchive' }), async () => {
    await page.waitForLoadState('domcontentloaded');
    await expect(archivedRow).toHaveCount(0);
    await page.goto('/threads');
    await expect(page.getByRole('link', { name: 'E2E renamed thread' })).toBeVisible();
  });

  const restoredThreadRow = page.locator('app-sidebar-menu-item').filter({
    has: page.getByRole('link', { name: 'E2E renamed thread', exact: true }),
  });
  await restoredThreadRow.hover();
  await verifyClick(restoredThreadRow.getByTitle('Thread actions'), () =>
    expect(page.getByRole('menuitem', { name: 'Archive' })).toBeVisible());
  await verifyClick(page.getByRole('menuitem', { name: 'Archive' }), () =>
    expect(page).toHaveURL(/\/threads$/));
  await page.goto('/archived');
  await verifyClick(page.getByRole('button', { name: 'Thread actions' }), () =>
    expect(page.getByRole('menuitem', { name: 'Delete' })).toBeVisible());
  await verifyClick(page.getByRole('menuitem', { name: 'Delete' }), () =>
    expect(page.getByRole('alertdialog')).toBeVisible());
  await verifyClick(page.getByRole('button', { name: 'Cancel' }), async () => {
    await expect(page.getByRole('alertdialog')).toBeHidden();
    await expect(page.locator('app-archived-threads .row').filter({ hasText: 'E2E renamed thread' })).toBeVisible();
  });
  await verifyClick(page.getByRole('button', { name: 'Thread actions' }), () =>
    expect(page.getByRole('menuitem', { name: 'Delete' })).toBeVisible());
  await verifyClick(page.getByRole('menuitem', { name: 'Delete' }), () =>
    expect(page.getByRole('alertdialog')).toBeVisible());
  await verifyClick(page.getByRole('button', { name: 'Delete' }), async () => {
    await page.waitForLoadState('domcontentloaded');
    const deletedRow = page.locator('app-archived-threads .row').filter({ hasText: 'E2E renamed thread' });
    await expect(deletedRow).toHaveCount(0);
    await page.reload();
    await expect(deletedRow).toHaveCount(0);
  });

  const projectsCleanup = await page.request.delete('/api/files/home/user/Projects');
  expect([204, 404]).toContain(projectsCleanup.status());
  await page.goto('/files');
  const shellPanel = page.locator('app-sidebar-panel');
  await verifyClick(page.getByRole('button', { name: 'Toggle sidebar' }), () =>
    expect(shellPanel).toHaveAttribute('state', 'collapsed'));
  await verifyClick(page.locator('app-sidebar app-sidebar-toggle button'), () =>
    expect(shellPanel).toHaveAttribute('state', 'open'));
  await verifyClick(page.locator('app-sidebar button.trigger'), () =>
    expect(page.getByRole('menuitem', { name: 'Archived' })).toBeVisible());
  await verifyClick(page.getByRole('menuitem', { name: 'Archived' }), () =>
    expect(page).toHaveURL(/\/archived$/));
});
