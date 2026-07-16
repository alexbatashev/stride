import { expect, test } from '@playwright/test';
import { fileURLToPath } from 'node:url';

const componentsBundle = fileURLToPath(new URL('../../dist/components.js', import.meta.url));

test('composer attachment cards are visible, compact, and removable', async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await page.setContent(`
    <script type="application/json" data-argon-stores>{"sidebar":{"activeThread":"thread-1"}}</script>
    <style>
      :root {
        color-scheme: dark;
        --accent: oklch(0.922 0 0);
        --accent-foreground: oklch(0.205 0 0);
        --border: oklch(1 0 0 / 10%);
        --card: oklch(0.205 0 0);
        --card-foreground: oklch(0.985 0 0);
        --foreground: oklch(0.985 0 0);
        --muted: oklch(0.269 0 0);
        --muted-foreground: oklch(0.708 0 0);
        --prompt-bg: #212121;
        --prompt-border: #333333;
      }
      body { background: #ffffff; font-family: sans-serif; margin: 0; }
    </style>
    <main style="box-sizing:border-box;margin:560px auto 0;max-width:870px;width:100%">
      <app-prompt-input></app-prompt-input>
    </main>
  `);
  await page.addScriptTag({ path: componentsBundle, type: 'module' });
  await page.waitForFunction(() => customElements.get('app-prompt-input'));
  await page.evaluate(() => {
    document.querySelector('app-prompt-input').attachments = [
      { key: 'one', id: 'staged-one', name: 'research.pdf', size: 1536, state: 'done' },
      { key: 'two', id: '', name: 'design-system.zip', size: 8192, state: 'uploading' },
    ];
  });

  const prompt = page.locator('app-prompt-input');
  const group = prompt.locator('[aria-label="Attached files"]');
  const cards = group.locator('app-attachment');
  await expect(group).toBeVisible();
  await expect(cards).toHaveCount(2);
  await expect(cards.nth(0)).toContainText('research.pdf');
  await expect(cards.nth(0)).toContainText('PDF · 2 KB');
  await expect(cards.nth(1)).toContainText('Uploading · 8 KB');

  const geometry = await group.evaluate((element) => {
    const prompt = element.getRootNode().host.getBoundingClientRect();
    const groupRect = element.getBoundingClientRect();
    const form = element.getRootNode().querySelector('form').getBoundingClientRect();
    const card = element.querySelector('app-attachment').getBoundingClientRect();
    return {
      groupWithinPrompt: groupRect.left >= prompt.left && groupRect.right <= prompt.right,
      groupAboveComposer: groupRect.bottom < form.top,
      cardHeight: card.height,
      cardWidth: card.width,
      formHeight: form.height,
    };
  });
  expect(geometry.groupWithinPrompt).toBe(true);
  expect(geometry.groupAboveComposer).toBe(true);
  expect(geometry.cardHeight).toBeLessThanOrEqual(60);
  expect(geometry.cardWidth).toBeGreaterThanOrEqual(160);
  expect(geometry.cardWidth).toBeLessThanOrEqual(240);

  await expect(prompt.locator('app-button[aria-label="Remove research.pdf"]')).toBeVisible();

  const textarea = prompt.locator('textarea');
  await textarea.fill(Array.from({ length: 30 }, (_, index) => `Line ${index + 1}`).join('\n'));
  const expanded = await prompt.evaluate((element) => {
    const textarea = element.shadowRoot.querySelector('textarea').getBoundingClientRect();
    const form = element.shadowRoot.querySelector('form').getBoundingClientRect();
    return { formHeight: form.height, textareaHeight: textarea.height };
  });
  expect(expanded.formHeight).toBeGreaterThan(geometry.formHeight);
  expect(expanded.textareaHeight).toBeGreaterThanOrEqual(200);
  expect(expanded.textareaHeight).toBeLessThanOrEqual(220);
});
