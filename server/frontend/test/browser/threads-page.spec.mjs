import { expect, test } from '@playwright/test';
import { fileURLToPath } from 'node:url';

const componentsBundle = fileURLToPath(new URL('../../dist/components.js', import.meta.url));

const threadFirstPaintHtml = String.raw`<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <script type="application/json" data-argon-stores>{"sidebar":{"activeThread":"thread-1"}}</script>
  </head>
  <body id="threads-page" data-thread-id="thread-1" data-selected-model="fast" data-running="false">
    <main>
      <section class="content">
        <div class="wrapper" data-messages>
          <app-message data-message-id="message-1" data-seq="1" data-role="tool" data-kind="tool_output" data-format="markdown" data-text="token 0" data-thinking="" data-tool-name="Shell">
            <template shadowrootmode="open">
              <style></style><!--argon-text:0--><app-spoiler data-title="Shell" data-content="token 0"><template shadowrootmode="open"><style></style><button type="button" aria-expanded="false" data-argon="1078" data-argon-bind="0"><span class="title"><!--argon-text:0-->Shell<!--/argon-text:0--></span><span class="chevron" aria-hidden="true"><!--argon-text:1--><icon-chevron-right><template shadowrootmode="open"><style></style><svg viewBox="0 0 24 24"><path d="m9 18 6-6-6-6"></path></svg></template></icon-chevron-right><!--/argon-text:1--></span></button><!--argon-text:2--><!--/argon-text:2--></template></app-spoiler><!--/argon-text:0-->
            </template>
          </app-message>
        </div>
      </section>
    </main>
  </body>
</html>`;

async function importComponents(page) {
  await page.addScriptTag({ path: componentsBundle, type: 'module' });
  await page.waitForFunction(() => customElements.get('app-message') && customElements.get('app-spoiler'));
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

test('threads page first-paint message hydration has no DOM snapshot diff', async ({ page }) => {
  await page.setContent(threadFirstPaintHtml);

  const before = await shadowSnapshot(page, '[data-messages]');
  await importComponents(page);
  await page.waitForFunction(() => document.querySelector('app-message')?.hasAttribute('hydrated'));
  const after = await shadowSnapshot(page, '[data-messages]');

  expect(after).toBe(before);
});

test('streamed thinking spoiler stays open through 100 browser updates', async ({ page }) => {
  await page.setContent(`<!doctype html>
    <html lang="en">
      <head>
        <script type="application/json" data-argon-stores>{"sidebar":{"activeThread":"thread-1"}}</script>
      </head>
      <body>
        <app-message data-message-id="message-1" data-seq="1" data-role="agent" data-kind="agent" data-format="markdown" data-text="body 0" data-thinking="token 0" data-tool-name=""></app-message>
      </body>
    </html>`);
  await importComponents(page);

  const result = await page.evaluate(async () => {
    const message = document.querySelector('app-message');
    const spoiler = message.shadowRoot.querySelector('app-spoiler');
    spoiler.shadowRoot.querySelector('button').click();

    for (let i = 1; i <= 100; i += 1) {
      message.text = `body ${i}`;
      message.thinking = `token ${i}`;
      await new Promise((resolve) => requestAnimationFrame(resolve));

      const current = message.shadowRoot.querySelector('app-spoiler');
      if (current !== spoiler) {
        return { ok: false, reason: `spoiler remounted at token ${i}` };
      }
      if (current.shadowRoot.querySelector('button').getAttribute('aria-expanded') !== 'true') {
        return { ok: false, reason: `spoiler closed at token ${i}` };
      }
    }

    return {
      ok: true,
      thinking: spoiler.shadowRoot.textContent,
      body: message.shadowRoot.querySelector('auto-markdown').shadowRoot.textContent,
    };
  });

  expect(result).toEqual({
    ok: true,
    thinking: expect.stringContaining('token 100'),
    body: expect.stringContaining('body 100'),
  });
});
