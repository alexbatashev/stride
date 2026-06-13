// Smoke tests for the compiled component bundle: registration, rendering,
// reactivity, and the custom events the page hydrators rely on.
// Run with: pnpm test (builds dist/components.js first).
import { test, before } from 'node:test';
import assert from 'node:assert/strict';
import { GlobalRegistrator } from '@happy-dom/global-registrator';

GlobalRegistrator.register();

before(async () => {
  await import('../dist/components.js');
});

function mount(tag, props = {}) {
  const el = document.createElement(tag);
  Object.assign(el, props);
  document.body.appendChild(el);
  return el;
}

function lastEvent(el, name) {
  const seen = { detail: undefined, count: 0 };
  el.addEventListener(name, (e) => {
    seen.detail = e.detail;
    seen.count += 1;
  });
  return seen;
}

test('all custom elements register', () => {
  for (const tag of [
    'app-button', 'app-text-input', 'auth-form', 'app-sidebar', 'app-sidebar-toggle',
    'app-message', 'app-spoiler', 'auto-markdown', 'app-prompt-input',
    'app-approval-bar', 'app-quiz-bar', 'app-data-table', 'app-file-browser',
    'app-file-manager', 'icon-arrow-up', 'icon-x',
  ]) {
    assert.ok(customElements.get(tag), `${tag} is not registered`);
  }
});

test('app-sidebar renders projects and threads as links', () => {
  const el = mount('app-sidebar', {
    projects: [{ id: 'p1', title: 'Project One', threads: [{ id: 't1', title: 'Thread One' }] }],
    threads: [{ id: 't2', title: 'Loose' }],
    activeThread: 't1',
  });
  const html = el.shadowRoot.innerHTML;
  assert.match(html, /Project One/);
  assert.match(html, /Thread One/);
  assert.match(html, /Loose/);

  const active = el.shadowRoot.querySelector('a[data-thread-id="t1"]');
  assert.equal(active.getAttribute('aria-current'), 'page');

  // Threads are plain links; navigation is the browser's job, not a custom event.
  const loose = el.shadowRoot.querySelector('a[data-thread-id="t2"]');
  assert.equal(loose.getAttribute('href'), '/threads/t2');

  // Reactive update: a new thread shows up without remounting.
  el.threads = [{ id: 't2', title: 'Loose' }, { id: 't3', title: 'Fresh' }];
  assert.match(el.shadowRoot.innerHTML, /Fresh/);
});

test('app-sidebar footer dispatches logout and new-project', () => {
  const el = mount('app-sidebar');
  const logout = lastEvent(el, 'logout');
  const newProject = lastEvent(el, 'new-project');
  for (const button of el.shadowRoot.querySelectorAll('.footer [data-action]')) {
    button.click();
  }
  assert.equal(logout.count, 1);
  assert.equal(newProject.count, 1);
});

test('app-message renders markdown for agent text', () => {
  const el = mount('app-message', { kind: 'agent', text: 'plain **bold** text' });
  const markdown = el.shadowRoot.querySelector('auto-markdown');
  assert.ok(markdown);
  assert.match(markdown.shadowRoot.innerHTML, /<strong>bold<\/strong>/);
});

test('app-message escaped text round-trips through markdown', () => {
  const el = mount('app-message', { kind: 'agent', text: 'a &lt;tag&gt; &amp; more' });
  const markdown = el.shadowRoot.querySelector('auto-markdown');
  const paragraph = markdown.shadowRoot.querySelector('p');
  assert.equal(paragraph.textContent, 'a <tag> & more');
});

test('app-message tool output folds into a spoiler', () => {
  const el = mount('app-message', { kind: 'tool_output', toolName: 'Shell', text: 'output' });
  const spoiler = el.shadowRoot.querySelector('app-spoiler');
  assert.ok(spoiler);
  assert.match(spoiler.shadowRoot.innerHTML, /Shell/);
  assert.doesNotMatch(spoiler.shadowRoot.innerHTML, /output/);
  spoiler.shadowRoot.querySelector('button').click();
  assert.match(spoiler.shadowRoot.innerHTML, /output/);
});

test('app-prompt-input submits on Enter and clears', () => {
  const el = mount('app-prompt-input');
  const submitted = lastEvent(el, 'prompt-submit');
  const textarea = el.shadowRoot.querySelector('textarea');
  textarea.value = '  hello friday  ';
  textarea.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
  assert.equal(submitted.detail.value, 'hello friday');
  assert.equal(textarea.value, '');
});

test('app-prompt-input swaps send for stop while running', () => {
  const el = mount('app-prompt-input');
  assert.ok(el.shadowRoot.querySelector('button[type="submit"]'));
  el.running = true;
  const stop = el.shadowRoot.querySelector('button.stop');
  assert.ok(stop);
  const stopped = lastEvent(el, 'prompt-stop');
  stop.click();
  assert.equal(stopped.count, 1);
  assert.ok(el.shadowRoot.querySelector('textarea').disabled);
});

test('app-approval-bar answers yes and no', () => {
  const el = mount('app-approval-bar', { message: 'Run rm -rf /tmp/x?' });
  assert.match(el.shadowRoot.innerHTML, /Run rm -rf/);
  const answered = lastEvent(el, 'approval-response');
  el.shadowRoot.querySelector('button.yes').click();
  assert.equal(answered.detail.approved, true);
  el.shadowRoot.querySelector('button.no').click();
  assert.equal(answered.detail.approved, false);
});

test('app-quiz-bar submits picked option and custom answer', () => {
  const el = mount('app-quiz-bar', { question: 'Pick one', options: ['Alpha', 'Beta'] });
  assert.match(el.shadowRoot.innerHTML, /Pick one/);
  const answered = lastEvent(el, 'quiz-response');

  const radio = el.shadowRoot.querySelector('input[type="radio"][value="Beta"]');
  radio.checked = true;
  el.shadowRoot.querySelector('.footer button').click();
  assert.equal(answered.detail.answer, 'Beta');

  el.shadowRoot.querySelector('input[type="text"]').value = 'Custom';
  el.shadowRoot.querySelector('.footer button').click();
  assert.equal(answered.detail.answer, 'Custom');
});

test('app-data-table renders rows and reports selection', () => {
  const rows = [
    { name: 'a.txt', path: 'dir/a.txt', kind: 'file', sizeLabel: '1 KB', updatedLabel: 'Jan 1, 2026' },
    { name: 'sub', path: 'dir/sub', kind: 'directory', sizeLabel: '', updatedLabel: '' },
  ];
  const el = mount('app-data-table', { rows });
  assert.match(el.shadowRoot.innerHTML, /a\.txt/);
  assert.match(el.shadowRoot.innerHTML, /1 KB/);

  const selection = lastEvent(el, 'selection-change');
  const box = el.shadowRoot.querySelector('input[data-row-id="dir/a.txt"]');
  box.checked = true;
  box.dispatchEvent(new Event('change', { bubbles: true }));
  assert.deepEqual(selection.detail.selectedIds, ['dir/a.txt']);

  const action = lastEvent(el, 'row-action');
  el.shadowRoot.querySelector('button[data-row-id="dir/sub"]').click();
  assert.deepEqual(action.detail, { action: 'open', rowId: 'dir/sub' });
});

test('auth-form switches mode via a plain link', () => {
  const el = mount('auth-form', { mode: 'login' });
  assert.match(el.shadowRoot.innerHTML, /Log in/);
  // The mode toggle is a plain navigation link, robust on every page.
  const link = el.shadowRoot.querySelector('.switch a');
  assert.equal(link.getAttribute('href'), '/auth/register');
});
