import { expect, test } from '@playwright/test';
import { fileURLToPath } from 'node:url';

const componentsBundle = fileURLToPath(new URL('../../dist/components.js', import.meta.url));

async function importComponents(page) {
  await page.addScriptTag({ path: componentsBundle, type: 'module' });
  await page.waitForFunction(() => customElements.get('app-message') && customElements.get('app-work-group') && customElements.get('app-tool-activity'));
}

async function renderedMessageHtml(page) {
  await page.setContent('<script type="application/json" data-argon-stores>{"sidebar":{"activeThread":"thread-1"}}</script>');
  await importComponents(page);
  return page.evaluate(() => {
    const message = document.createElement('app-message');
    Object.assign(message, { messageId: 'message-1', seq: 1, role: 'agent', kind: 'agent', format: 'markdown', text: 'Ready', thinking: 'Checking', pending: false });
    for (const [name, value] of Object.entries({ 'data-message-id': 'message-1', 'data-seq': '1', 'data-role': 'agent', 'data-kind': 'agent', 'data-format': 'markdown', 'data-text': 'Ready', 'data-thinking': 'Checking', 'data-pending': 'false' })) message.setAttribute(name, value);
    document.body.appendChild(message);
    const escape = (value) => String(value).replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    const serialize = (node) => {
      if (node.nodeType === Node.TEXT_NODE) return escape(node.data);
      if (node.nodeType === Node.COMMENT_NODE) return `<!--${node.nodeValue}-->`;
      if (node.nodeType !== Node.ELEMENT_NODE) return '';
      const attrs = [...node.attributes].filter((attr) => attr.name !== 'hydrated').map((attr) => ` ${attr.name}="${escape(attr.value)}"`).join('');
      const shadow = node.shadowRoot ? `<template shadowrootmode="open">${[...node.shadowRoot.childNodes].map(serialize).join('')}</template>` : '';
      return `<${node.localName}${attrs}>${shadow}${[...node.childNodes].map(serialize).join('')}</${node.localName}>`;
    };
    return serialize(message);
  });
}

async function renderedChatHtml(page) {
  await page.setContent('<script type="application/json" data-argon-stores>{"sidebar":{"activeThread":"thread-1"}}</script>');
  await importComponents(page);
  return page.evaluate(() => {
    const chat = document.createElement('app-chat-view');
    const turns = [{
      id: 'turn-1',
      hasUser: false,
      user: { id: '', seq: 0, createdAt: 0, role: 'user', kind: 'message', format: 'markdown', text: '', thinking: '', toolName: '', toolDetail: '', status: 'finished', isError: false, pending: false },
      hasWork: true,
      workLabel: 'Worked for 12s',
      running: false,
      startedAt: 0,
      segments: [{ id: 'segment-1', commentary: 'Checking the files', tools: [{ id: 'tool-1', seq: 1, createdAt: 1, role: 'tool', kind: 'tool_activity', format: 'markdown', text: 'README.md', thinking: '', toolName: 'Ran command', toolDetail: 'ls', status: 'finished', isError: false, pending: false }] }],
      hasAnswer: true,
      answer: { id: 'answer-1', seq: 2, createdAt: 2, role: 'agent', kind: 'agent', format: 'markdown', text: 'Done', thinking: '', toolName: '', toolDetail: '', status: 'finished', isError: false, pending: false },
    }];
    chat.turns = turns;
    chat.setAttribute('data-turns', JSON.stringify(turns));
    document.body.appendChild(chat);
    const escape = (value) => String(value).replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    const serialize = (node) => {
      if (node.nodeType === Node.TEXT_NODE) return escape(node.data);
      if (node.nodeType === Node.COMMENT_NODE) return `<!--${node.nodeValue}-->`;
      if (node.nodeType !== Node.ELEMENT_NODE) return '';
      const attrs = [...node.attributes].filter((attr) => attr.name !== 'hydrated').map((attr) => ` ${attr.name}="${escape(attr.value)}"`).join('');
      const shadow = node.shadowRoot ? `<template shadowrootmode="open">${[...node.shadowRoot.childNodes].map(serialize).join('')}</template>` : '';
      return `<${node.localName}${attrs}>${shadow}${[...node.childNodes].map(serialize).join('')}</${node.localName}>`;
    };
    return serialize(chat);
  });
}

async function shadowSnapshot(page, selector) {
  return page.locator(selector).evaluate((root) => {
    const escapeAttr = (value) =>
      String(value)
        .replace(/&/g, '&amp;')
        .replace(/"/g, '&quot;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;');
    const escapeText = (value) =>
      String(value)
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;');

    function serialize(node) {
      if (node.nodeType === Node.TEXT_NODE) {
        return escapeText(node.data);
      }
      if (node.nodeType === Node.COMMENT_NODE) {
        return `<!--${node.nodeValue}-->`;
      }
      if (node.nodeType !== Node.ELEMENT_NODE) {
        return '';
      }

      const attrs = [...node.attributes]
        .filter((attr) => attr.name !== 'hydrated')
        .sort((a, b) => a.name.localeCompare(b.name))
        .map((attr) => ` ${attr.name}="${escapeAttr(attr.value)}"`)
        .join('');
      const shadow = node.shadowRoot
        ? `<template shadowrootmode="open">${[...node.shadowRoot.childNodes].map(serialize).join('')}</template>`
        : '';
      const light = [...node.childNodes].map(serialize).join('');
      return `<${node.localName}${attrs}>${shadow}${light}</${node.localName}>`;
    }

    return [...root.childNodes].map(serialize).join('');
  });
}

async function openSettings(page) {
  await page.waitForFunction(() => {
    const item = document.querySelector('app-sidebar')?.shadowRoot?.querySelector('sidebar-settings-item');
    return item?.shadowRoot?.querySelector('app-sidebar-menu-button')?.shadowRoot?.querySelector('button');
  });
  await page.evaluate(() => {
    const sidebar = document.querySelector('app-sidebar');
    const item = sidebar.shadowRoot.querySelector('sidebar-settings-item');
    item.shadowRoot.querySelector('app-sidebar-menu-button').shadowRoot.querySelector('button').click();
  });
  await page.waitForFunction(() => {
    const host = document.querySelector('app-settings-dialog');
    const dialog = host?.shadowRoot?.querySelector('app-dialog');
    return dialog?.open && dialog.querySelector('app-settings');
  });
}

test('settings dialog follows the shadcn desktop structure and leaves background work mounted', async ({ page }) => {
  const runtimeErrors = [];
  page.on('pageerror', (error) => runtimeErrors.push(error.message));
  page.on('console', (message) => { if (message.type() === 'error') runtimeErrors.push(message.text()); });
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.setContent(`<!doctype html><html><head><script type="application/json" data-argon-stores>{"sidebar":{"activeThread":"thread-1"}}</script></head><body><div id="background">0</div><app-sidebar></app-sidebar><app-settings-dialog></app-settings-dialog></body></html>`);
  await importComponents(page);
  await page.evaluate(() => {
    window.__backgroundTicks = 0;
    window.__backgroundTimer = window.setInterval(() => {
      window.__backgroundTicks += 1;
      document.querySelector('#background').textContent = String(window.__backgroundTicks);
    }, 5);
  });

  await openSettings(page);
  const geometry = await page.evaluate(() => {
    const host = document.querySelector('app-settings-dialog');
    const primitive = host.shadowRoot.querySelector('app-dialog');
    const dialog = primitive.shadowRoot.querySelector('.dialog').getBoundingClientRect();
    const settings = primitive.querySelector('app-settings');
    const tabs = settings.shadowRoot.querySelector('.tabs').getBoundingClientRect();
    const panels = settings.shadowRoot.querySelector('.panels').getBoundingClientRect();
    return {
      dialogWidth: dialog.width,
      tabsRight: tabs.right,
      panelsLeft: panels.left,
      ticks: window.__backgroundTicks,
      cards: settings.shadowRoot.querySelectorAll('app-card').length,
    };
  });

  expect(geometry.dialogWidth).toBeGreaterThan(800);
  expect(geometry.tabsRight).toBeLessThanOrEqual(geometry.panelsLeft + 1);
  expect(geometry.cards).toBe(0);
  await page.waitForTimeout(30);
  expect(await page.evaluate(() => window.__backgroundTicks)).toBeGreaterThan(geometry.ticks);
  expect(runtimeErrors.filter((message) => !message.startsWith("WebSocket connection to 'ws://api/events' failed"))).toEqual([]);
  await page.evaluate(() => window.clearInterval(window.__backgroundTimer));
});

test('settings dialog is full-screen with top tabs on mobile', async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.setContent(`<!doctype html><html><head><script type="application/json" data-argon-stores>{"sidebar":{"activeThread":"thread-1"}}</script></head><body><app-sidebar></app-sidebar><app-settings-dialog></app-settings-dialog></body></html>`);
  await importComponents(page);
  await openSettings(page);

  const geometry = await page.evaluate(() => {
    const host = document.querySelector('app-settings-dialog');
    const primitive = host.shadowRoot.querySelector('app-dialog');
    const dialog = primitive.shadowRoot.querySelector('.dialog').getBoundingClientRect();
    const settings = primitive.querySelector('app-settings');
    const tabs = settings.shadowRoot.querySelector('.tabs').getBoundingClientRect();
    const panels = settings.shadowRoot.querySelector('.panels').getBoundingClientRect();
    return { dialog, tabsBottom: tabs.bottom, panelsTop: panels.top };
  });

  expect(Math.abs(geometry.dialog.width - 390)).toBeLessThanOrEqual(1);
  expect(Math.abs(geometry.dialog.height - 844)).toBeLessThanOrEqual(1);
  expect(geometry.tabsBottom).toBeLessThanOrEqual(geometry.panelsTop + 1);
});

test('skill creation submits through app-button with a valid browser pattern', async ({ page }) => {
  const runtimeErrors = [];
  let created;
  page.on('pageerror', (error) => runtimeErrors.push(error.message));
  page.on('console', (message) => { if (message.type() === 'error') runtimeErrors.push(message.text()); });
  await page.route('http://stride.test/**', async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    if (url.pathname === '/api/settings/skills' && request.method() === 'POST') {
      created = request.postDataJSON();
      await route.fulfill({ json: { id: 'skill-1', ...created } });
      return;
    }
    if (url.pathname === '/api/settings/skills') {
      await route.fulfill({ json: created ? [{ id: 'skill-1', ...created }] : [] });
      return;
    }
    await route.fulfill({ contentType: 'text/html', body: '<app-settings-skills></app-settings-skills>' });
  });
  await page.goto('http://stride.test/');
  await importComponents(page);

  const name = page.getByRole('textbox', { name: 'Name', exact: true });
  await expect(name).toBeVisible();
  expect(await name.evaluate((input) => input.checkValidity())).toBe(false);
  await name.fill('python-debugging');
  await page.getByRole('textbox', { name: 'Title', exact: true }).fill('Python Debugging');
  await page.getByRole('textbox', { name: 'Description', exact: true }).fill('Trace Python failures.');
  await page.getByRole('textbox', { name: 'Content', exact: true }).fill('Inspect the traceback.');
  expect(await name.evaluate((input) => input.checkValidity())).toBe(true);
  await page.getByRole('button', { name: 'Add skill', exact: true }).click();

  await expect.poll(() => created).toEqual({
    name: 'python-debugging',
    title: 'Python Debugging',
    description: 'Trace Python failures.',
    content: 'Inspect the traceback.',
  });
  await expect(page.getByText('Python Debugging', { exact: true })).toBeVisible();
  expect(runtimeErrors).toEqual([]);
});

test('threads page first-paint message hydration has no DOM snapshot diff', async ({ browser, page }) => {
  const context = await browser.newContext();
  const source = await context.newPage();
  const messageHtml = await renderedMessageHtml(source);
  await context.close();
  await page.setContent(`<script type="application/json" data-argon-stores>{"sidebar":{"activeThread":"thread-1"}}</script><div data-messages>${messageHtml}</div>`);

  const before = await shadowSnapshot(page, '[data-messages]');
  await importComponents(page);
  await page.waitForFunction(() => document.querySelector('app-message')?.hasAttribute('hydrated'));
  const after = await shadowSnapshot(page, '[data-messages]');

  expect(after).toBe(before);
});

test('nested first-paint work fold hydrates and opens', async ({ browser, page }) => {
  const context = await browser.newContext();
  const source = await context.newPage();
  const chatHtml = await renderedChatHtml(source);
  await context.close();
  await page.setContent(`<script type="application/json" data-argon-stores>{"sidebar":{"activeThread":"thread-1"}}</script>${chatHtml}`);

  await importComponents(page);
  await page.waitForFunction(() => document.querySelector('app-chat-view')?.shadowRoot?.querySelector('app-work-group')?.hasAttribute('hydrated'));
  const result = await page.evaluate(() => {
    const work = document.querySelector('app-chat-view').shadowRoot.querySelector('app-work-group');
    const button = work.shadowRoot.querySelector('.fold-toggle');
    button.click();
    return {
      expanded: button.getAttribute('aria-expanded'),
      commentary: work.shadowRoot.querySelector('auto-markdown')?.textContent,
      tool: work.shadowRoot.querySelector('app-tool-cluster')?.shadowRoot.querySelector('app-tool-activity')?.shadowRoot.textContent,
    };
  });

  expect(result).toEqual({
    expanded: 'true',
    commentary: expect.stringContaining('Checking the files'),
    tool: expect.stringContaining('Ran command'),
  });
});

test('model picker opens above its trigger within the viewport', async ({ page }) => {
  await page.setViewportSize({ width: 640, height: 480 });
  await page.setContent(`<!doctype html>
    <html lang="en">
      <head>
        <script type="application/json" data-argon-stores>{"sidebar":{"activeThread":"thread-1"}}</script>
        <style>body { align-items: flex-end; display: flex; justify-content: center; margin: 0; min-height: 100vh; }</style>
      </head>
      <body><app-model-picker></app-model-picker></body>
    </html>`);
  await importComponents(page);

  const geometry = await page.evaluate(async () => {
    const picker = document.querySelector('app-model-picker');
    picker.models = Array.from({ length: 20 }, (_, index) => ({
      value: `model-${index}`,
      label: `Model ${index}`,
      description: `Description ${index}`,
      vision: false,
    }));
    await new Promise((resolve) => requestAnimationFrame(resolve));
    picker.shadowRoot.querySelector('.trigger-button').click();
    const trigger = picker.shadowRoot.querySelector('.trigger-button').getBoundingClientRect();
    const popup = picker.shadowRoot.querySelector('.popup').getBoundingClientRect();
    return {
      popupBottom: popup.bottom,
      popupTop: popup.top,
      triggerTop: trigger.top,
      viewportHeight: window.innerHeight,
    };
  });

  expect(geometry.popupBottom).toBeLessThanOrEqual(geometry.triggerTop);
  expect(geometry.popupTop).toBeGreaterThanOrEqual(0);
  expect(geometry.popupBottom).toBeLessThanOrEqual(geometry.viewportHeight);

  await page.locator('body').click({ position: { x: 8, y: 8 } });
  const open = await page.locator('app-model-picker').evaluate((picker) => picker.hasAttribute('open'));
  expect(open).toBe(false);
});

test('streamed tool disclosure stays open and mounted through 100 updates', async ({ page }) => {
  await page.setContent('<script type="application/json" data-argon-stores>{"sidebar":{"activeThread":"thread-1"}}</script><app-tool-activity data-title="Ran command" data-detail="ls -la" data-content="token 0" data-status="running" data-is-error="false"></app-tool-activity>');
  await importComponents(page);
  const result = await page.evaluate(async () => {
    const tool = document.querySelector('app-tool-activity');
    tool.shadowRoot.querySelector('button').click();
    for (let i = 1; i <= 100; i += 1) {
      tool.content = `token ${i}`;
      tool.status = i === 100 ? 'finished' : 'running';
      await new Promise((resolve) => requestAnimationFrame(resolve));
      if (tool.shadowRoot.querySelector('button').getAttribute('aria-expanded') !== 'true') return false;
    }
    return tool.shadowRoot.textContent.includes('token 100');
  });
  expect(result).toBe(true);
});

test('one work fold owns reasoning and tools through 100 browser updates', async ({ page }) => {
  await page.setContent(`<!doctype html>
    <html lang="en">
      <head>
        <script type="application/json" data-argon-stores>{"sidebar":{"activeThread":"thread-1"}}</script>
      </head>
      <body>
        <app-work-group data-label="Worked for 12s"></app-work-group>
      </body>
    </html>`);
  await importComponents(page);

  const result = await page.evaluate(async () => {
    const work = document.querySelector('app-work-group');
    const makeSegments = (i) => [{
      id: 'segment-1',
      commentary: `checking ${i}`,
      tools: [{ id: 'tool-1', seq: 1, createdAt: 1, role: 'tool', kind: 'tool_activity', format: 'markdown', text: `token ${i}`, thinking: '', toolName: 'Ran command', toolDetail: 'rg flags', status: 'running', isError: false, pending: true }],
    }];
    work.segments = makeSegments(0);
    await new Promise((resolve) => requestAnimationFrame(resolve));
    const fold = work.shadowRoot.querySelector('.fold-toggle');
    if (work.shadowRoot.querySelector('app-tool-cluster')) return { ok: false, reason: 'work visible before fold opens' };
    fold.click();
    const cluster = work.shadowRoot.querySelector('app-tool-cluster');
    const tool = cluster.shadowRoot.querySelector('app-tool-activity');
    tool.shadowRoot.querySelector('button').click();

    for (let i = 1; i <= 100; i += 1) {
      work.segments = makeSegments(i);
      await new Promise((resolve) => requestAnimationFrame(resolve));

      const current = work.shadowRoot.querySelector('app-tool-cluster').shadowRoot.querySelector('app-tool-activity');
      if (current !== tool) {
        return { ok: false, reason: `tool remounted at token ${i}` };
      }
      if (fold.getAttribute('aria-expanded') !== 'true') {
        return { ok: false, reason: `work fold closed at token ${i}` };
      }
      if (tool.shadowRoot.querySelector('button').getAttribute('aria-expanded') !== 'true') {
        return { ok: false, reason: `tool disclosure closed at token ${i}` };
      }
    }

    return {
      ok: true,
      commentary: work.shadowRoot.querySelector('auto-markdown').textContent,
      tool: tool.shadowRoot.textContent,
    };
  });

  expect(result).toEqual({
    ok: true,
    commentary: expect.stringContaining('checking 100'),
    tool: expect.stringContaining('token 100'),
  });
});
