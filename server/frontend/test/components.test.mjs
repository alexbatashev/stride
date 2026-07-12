// Smoke tests for the compiled component bundle: registration, rendering,
// reactivity, and the custom events the page hydrators rely on.
// Run with: pnpm test (builds dist/components.js first).
import { test, before, afterEach } from 'node:test';
import assert from 'node:assert/strict';
import { GlobalRegistrator } from '@happy-dom/global-registrator';

GlobalRegistrator.register();

before(async () => {
  const stores = document.createElement('script');
  stores.type = 'application/json';
  stores.dataset.argonStores = '';
  stores.textContent = JSON.stringify({ sidebar: { activeThread: 't1' } });
  document.head.appendChild(stores);
  await import('../dist/components.js');
});

afterEach(() => {
  document.body.replaceChildren();
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

function tick() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}

function nextFrame() {
  return new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve)));
}

function buttonWithText(root, text) {
  return Array.from(root.querySelectorAll('button')).find((button) => button.textContent.trim() === text);
}

function deepElements(root) {
  return Array.from(root.querySelectorAll('*')).flatMap((element) => [
    element,
    ...(element.shadowRoot ? deepElements(element.shadowRoot) : []),
  ]);
}

test('all custom elements register', () => {
  for (const tag of [
    'app-button', 'app-input', 'app-text-input', 'auth-form', 'app-sidebar', 'app-sidebar-toggle',
    'app-chat-view', 'app-message', 'app-message-actions', 'app-spoiler', 'app-tool-activity', 'app-tool-cluster', 'app-work-group', 'auto-markdown', 'app-prompt-input',
    'app-approval-bar', 'app-quiz-bar', 'app-data-table', 'app-file-browser',
    'app-file-explorer', 'app-side-panel', 'app-subagent-view', 'app-automations', 'app-settings', 'icon-arrow-up', 'icon-x',
    'app-badge', 'app-label', 'app-separator', 'app-skeleton', 'app-aspect-ratio',
    'app-card', 'app-avatar', 'app-avatar-group', 'app-avatar-group-count', 'app-alert', 'app-progress', 'app-checkbox',
    'app-switch', 'app-toggle', 'app-textarea', 'app-radio-group', 'app-slider',
    'app-breadcrumb', 'app-tabs', 'app-accordion', 'app-pagination', 'app-dialog',
    'app-alert-dialog', 'app-sheet', 'app-tooltip', 'app-popover', 'app-hover-card',
    'app-select', 'app-combobox', 'app-attachment', 'app-marker', 'app-message-scroller',
    'app-kbd', 'app-spinner', 'app-toggle-group', 'app-sonner',
    'app-sidebar-provider', 'app-sidebar-panel', 'app-sidebar-inset', 'app-sidebar-header',
    'app-sidebar-content', 'app-sidebar-footer', 'app-sidebar-group', 'app-sidebar-group-label',
    'app-sidebar-group-content', 'app-sidebar-menu', 'app-sidebar-menu-item',
    'app-sidebar-menu-button', 'app-sidebar-menu-action', 'app-sidebar-menu-badge',
    'app-sidebar-input', 'app-sidebar-separator', 'app-sidebar-menu-skeleton', 'app-sidebar-rail',
    'app-settings-memory', 'app-settings-models', 'icon-check', 'icon-terminal',
  ]) {
    assert.ok(customElements.get(tag), `${tag} is not registered`);
  }
});

test('app-combobox filters options and emits selection intent', async () => {
  const el = mount('app-combobox', { options: [{ value: 'alpha', label: 'Alpha' }, { value: 'beta', label: 'Beta' }] });
  const events = [];
  el.addEventListener('value-change', (event) => events.push(event.detail));
  const input = el.shadowRoot.querySelector('app-input').shadowRoot.querySelector('input');
  input.value = 'bet';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  await nextFrame();
  assert.doesNotMatch(el.shadowRoot.textContent, /Alpha/);
  el.shadowRoot.querySelector('.option').click();
  assert.deepEqual(events, [{ value: 'beta' }]);
});

test('app-toggle-group is controlled and supports single selection intent', () => {
  const el = mount('app-toggle-group', { kind: 'single', value: ['bold'], options: [{ value: 'bold', label: 'Bold' }, { value: 'italic', label: 'Italic' }] });
  let detail;
  el.addEventListener('value-change', (event) => { detail = event.detail; });
  const toggles = Array.from(el.shadowRoot.querySelectorAll('app-toggle'));
  toggles.find((toggle) => toggle.textContent.trim() === 'Italic').shadowRoot.querySelector('button').click();
  assert.deepEqual(detail, { value: ['italic'] });
  assert.equal(toggles.find((toggle) => toggle.textContent.trim() === 'Bold').shadowRoot.querySelector('button').getAttribute('aria-pressed'), 'true');
});

test('app-sonner reports toast actions', () => {
  const el = mount('app-sonner', { toasts: [{ id: 'saved', title: 'Saved', action: 'Undo' }] });
  let detail;
  el.addEventListener('toast-action', (event) => { detail = event.detail; });
  buttonWithText(el.shadowRoot, 'Undo').click();
  assert.deepEqual(detail, { id: 'saved' });
});

test('app-sidebar renders projects and threads as links', () => {
  const el = mount('app-sidebar', {
    projects: [{ id: 'p1', title: 'Project One', threads: [{ id: 't1', title: 'Thread One' }] }],
    threads: [{ id: 't2', title: 'Loose' }],
  });
  const html = el.shadowRoot.innerHTML;
  assert.match(html, /Project One/);
  assert.match(html, /Thread One/);
  assert.match(html, /Loose/);

  const composed = deepElements(el.shadowRoot);
  for (const tag of ['app-sidebar-panel', 'app-sidebar-header', 'app-sidebar-content', 'app-sidebar-group', 'app-sidebar-menu', 'app-sidebar-menu-item', 'app-sidebar-menu-button', 'app-sidebar-menu-action', 'app-sidebar-footer', 'app-sidebar-rail']) {
    assert.ok(composed.some((element) => element.localName === tag), `${tag} is composed into app-sidebar`);
  }

  const links = composed.filter((element) => element.localName === 'a');
  const active = links.find((link) => link.getAttribute('href') === '/threads/t1');
  assert.equal(active?.getAttribute('aria-current'), 'page');

  // Threads are plain links; navigation is the browser's job, not a custom event.
  const loose = links.find((link) => link.getAttribute('href') === '/threads/t2');
  assert.equal(loose?.getAttribute('href'), '/threads/t2');

  // Reactive update: a new thread shows up without remounting.
  el.threads = [{ id: 't2', title: 'Loose' }, { id: 't3', title: 'Fresh' }];
  assert.match(el.shadowRoot.innerHTML, /Fresh/);
});

test('app-sidebar footer dispatches logout and new-project', () => {
  const el = mount('app-sidebar');
  const logout = lastEvent(el, 'logout');
  const newProject = lastEvent(el, 'new-project');
  for (const action of el.shadowRoot.querySelectorAll('app-sidebar-footer app-button')) {
    action.shadowRoot.querySelector('button').click();
  }
  assert.equal(logout.count, 1);
  assert.equal(newProject.count, 1);
});

test('app-sidebar thread menu includes its composed action anchor', () => {
  const el = mount('app-sidebar', { threads: [{ id: 't1', title: 'Thread one' }] });
  const menu = lastEvent(el, 'thread-menu');
  const action = deepElements(el.shadowRoot).find((element) => element.localName === 'app-sidebar-menu-action');
  action.shadowRoot.querySelector('button').click();
  assert.equal(menu.count, 1);
  assert.equal(menu.detail.id, 't1');
  assert.equal(menu.detail.anchor, action);
});

test('app-sidebar collapse keeps rail icons left aligned during width transition', async () => {
  const el = mount('app-sidebar');
  await Promise.resolve();

  el.shadowRoot.querySelector('app-sidebar-toggle').shadowRoot.querySelector('app-button').shadowRoot.querySelector('button').click();
  await Promise.resolve();

  const panel = el.shadowRoot.querySelector('app-sidebar-panel');
  assert.equal(panel.getAttribute('state'), 'collapsed');

  const navButtons = Array.from(el.shadowRoot.querySelectorAll('sidebar-navigation-item'))
    .map((item) => item.shadowRoot.querySelector('app-sidebar-menu-button'));
  assert.ok(navButtons.every((button) => button.getAttribute('data-collapsed') === 'true'));
  const css = navButtons[0].shadowRoot.querySelector('style').textContent;
  assert.match(css, /:host\(\[data-collapsed="true"\]\) \.control\s*{[^}]*width:\s*32px/s);
  assert.doesNotMatch(css, /justify-content:\s*center/);
});

test('app-message renders html for agent text', () => {
  const el = mount('app-message', { kind: 'agent', format: 'html', text: '<p>plain <strong>bold</strong> text</p>' });
  const html = el.shadowRoot.querySelector('auto-markdown');
  assert.ok(html);
  assert.match(html.shadowRoot.innerHTML, /<strong>bold<\/strong>/);
});

test('app-message escaped text stays text in html renderer', () => {
  const el = mount('app-message', { kind: 'agent', format: 'html', text: 'a &lt;tag&gt; &amp; more' });
  const html = el.shadowRoot.querySelector('auto-markdown');
  assert.equal(html.shadowRoot.querySelector('tag'), null);
  assert.match(html.shadowRoot.textContent, /a <tag> & more/);
});

test('app-message decodes escaped html code blocks as text', () => {
  const el = mount('app-message', {
    kind: 'agent',
    format: 'html',
    text: '<pre><code>#include &amp;lt;stdio.h&amp;gt;\n&amp;lt;script&amp;gt;alert(1)&amp;lt;/script&amp;gt;</code></pre>',
  });
  const html = el.shadowRoot.querySelector('auto-markdown');
  const code = html.shadowRoot.querySelector('pre code');
  assert.equal(code?.textContent, '#include <stdio.h>\n<script>alert(1)</script>');
  assert.equal(html.shadowRoot.querySelector('script'), null);
});

test('app-message sanitizes html renderer output defensively', () => {
  const el = mount('app-message', {
    kind: 'agent',
    format: 'html',
    text: '<p onclick="alert(1)">ok <strong data-x="1">bold</strong></p><script>alert(1)</script><a href="javascript:alert(1)" onclick="x()">bad</a><a href="/safe?q=1&amp;x=2">safe</a><iframe src="https://evil.example/widget.html"></iframe><img src="javascript:alert(1)" onerror="x()" alt="A">',
  });
  const html = el.shadowRoot.querySelector('auto-markdown');
  assert.equal(html.shadowRoot.querySelector('script'), null);
  assert.equal(html.shadowRoot.querySelector('p')?.hasAttribute('onclick'), false);
  assert.equal(html.shadowRoot.querySelector('strong')?.hasAttribute('data-x'), false);
  assert.equal(html.shadowRoot.querySelector('a')?.hasAttribute('href'), false);
  assert.equal(html.shadowRoot.querySelector('a[href="/safe?q=1&x=2"]')?.getAttribute('rel'), 'noopener noreferrer');
  assert.equal(html.shadowRoot.querySelector('iframe'), null);
  assert.equal(html.shadowRoot.querySelector('img'), null);
});

test('app-message renders markdown for agent text by default', () => {
  const el = mount('app-message', { kind: 'agent', text: '# Title\n\nHello **boss**' });
  const html = el.shadowRoot.querySelector('auto-markdown');
  assert.ok(html);
  assert.equal(html.shadowRoot.querySelector('h1')?.textContent, 'Title');
  assert.equal(html.shadowRoot.querySelector('strong')?.textContent, 'boss');
});

test('app-message decodes escaped markdown text before rendering', () => {
  const el = mount('app-message', {
    kind: 'agent',
    text: 'That&#39;s the proof of concept right there\n\n```c\n#include &lt;stdio.h&gt;\n```',
  });
  const html = el.shadowRoot.querySelector('auto-markdown');
  assert.match(html.shadowRoot.textContent, /That's the proof of concept right there/);
  assert.equal(html.shadowRoot.querySelector('pre code')?.textContent, '#include <stdio.h>');
});

test('app-message renders markdown tables', () => {
  const el = mount('app-message', {
    kind: 'agent',
    text: '| Name | Meaning |\n| --- | --- |\n| **A** | `alpha` |\n| B | beta |',
  });
  const html = el.shadowRoot.querySelector('auto-markdown');
  const table = html.shadowRoot.querySelector('table');
  assert.ok(table);
  assert.equal(table.querySelectorAll('thead th').length, 2);
  assert.equal(table.querySelector('tbody tr:first-child td:first-child strong')?.textContent, 'A');
  assert.equal(table.querySelector('tbody tr:first-child td:last-child code')?.textContent, 'alpha');
});

test('app-message wraps html tables for horizontal scrolling', () => {
  const el = mount('app-message', {
    kind: 'agent',
    format: 'html',
    text: '<table><tr><th>First</th><th>Second</th></tr><tr><td>A</td><td>B</td></tr></table>',
  });
  const html = el.shadowRoot.querySelector('auto-markdown');
  const wrap = html.shadowRoot.querySelector('.table-wrap');
  assert.ok(wrap);
  assert.equal(wrap.querySelector('table')?.tagName, 'TABLE');
});

test('app-message tool output folds into a spoiler', () => {
  const spoiler = mount('app-tool-activity', { title: 'Shell', content: 'output' });
  assert.ok(spoiler);
  assert.match(spoiler.shadowRoot.innerHTML, /Shell/);
  assert.equal(spoiler.shadowRoot.querySelector('button').getAttribute('aria-expanded'), 'false');
  spoiler.shadowRoot.querySelector('button').click();
  assert.match(spoiler.shadowRoot.innerHTML, /output/);

  spoiler.content = 'streamed output';
  assert.match(spoiler.shadowRoot.innerHTML, /streamed output/);
});

test('chat timeline merges calls with outputs and includes clickable subagents', async () => {
  const { buildClientTimeline } = await import('../dist/argon/components/chat-timeline.js');
  const { threadStream } = await import('../dist/argon/stores/thread-stream.js');
  threadStream.subagents = [{ id: 'agent-1', name: 'Research options', model: 'helper', result: 'child', finished: true, parentToolCallId: 'call-2', agentPath: 'agent-1', createdAt: 1 }];
  const base = { format: 'markdown', thinking: null, tool_call_name: null, tool_call_id: null, tool_calls: [] };
  const timeline = buildClientTimeline([
    { ...base, id: 'assistant', seq: 1, role: 'agent', content: '', tool_calls: [
      { id: 'call-1', name: 'shell', arguments: '{"command":"ls -la"}' },
      { id: 'call-2', name: 'collaboration.spawn_agent', arguments: '{}' },
    ] },
    { ...base, id: 'output-1', seq: 2, role: 'tool', content: 'files', tool_call_id: 'call-1' },
    { ...base, id: 'output-2', seq: 3, role: 'tool', content: 'child', tool_call_id: 'call-2' },
  ]);
  assert.equal(timeline.length, 2);
  assert.equal(timeline[0].id, 'tool:call-1');
  assert.equal(timeline[0].toolName, 'Ran command');
  assert.equal(timeline[0].toolDetail, 'ls -la');
  assert.equal(timeline[0].content, 'files');
  assert.equal(timeline[1].toolName, 'Research options');
  assert.equal(timeline[1].subagentKey, 'agent-1');
});

test('chat turns fold all reasoning and tools while leaving the final answer visible', async () => {
  const { buildChatTurns } = await import('../dist/argon/shared/timeline.js');
  const item = (overrides) => ({ id: '', seq: 0, createdAt: 0, role: 'agent', kind: 'agent', format: 'markdown', text: '', thinking: '', toolName: '', toolDetail: '', status: 'finished', isError: false, pending: false, ...overrides });
  const turns = buildChatTurns([
    item({ id: 'user', seq: 1, createdAt: 1_000, role: 'user', kind: 'user', text: 'Question' }),
    item({ id: 'thinking', seq: 2, createdAt: 2_000, thinking: 'Checking the real source.' }),
    item({ id: 'tool-1', seq: 3, createdAt: 5_000, role: 'tool', kind: 'tool_activity', toolName: 'Ran command', toolDetail: 'pwd', text: '/repo' }),
    item({ id: 'tool-2', seq: 4, createdAt: 7_000, role: 'tool', kind: 'tool_activity', toolName: 'Ran command', toolDetail: 'rg flags', text: 'matches' }),
    item({ id: 'answer', seq: 5, createdAt: 28_000, text: 'Final answer', thinking: 'Now summarize the findings.' }),
  ], false);
  assert.equal(turns.length, 1);
  assert.equal(turns[0].workLabel, 'Worked for 27s');
  assert.equal(turns[0].segments.length, 2);
  assert.equal(turns[0].segments[0].commentary, 'Checking the real source.');
  assert.equal(turns[0].segments[0].tools.length, 2);
  assert.equal(turns[0].segments[1].commentary, 'Now summarize the findings.');
  assert.equal(turns[0].answer.text, 'Final answer');
});

test('completed message copy action writes the exact message text', async () => {
  let copied = '';
  Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText: async (value) => { copied = value; } } });
  const el = mount('app-message', { kind: 'agent', text: 'Copy this exactly', pending: false });
  const actions = el.shadowRoot.querySelector('app-message-actions');
  actions.shadowRoot.querySelector('app-button').shadowRoot.querySelector('button').click();
  await tick();
  assert.equal(copied, 'Copy this exactly');
  assert.equal(actions.shadowRoot.querySelector('app-tooltip').text, 'Copied');
});

test('work group owns reasoning and tool disclosures without remounting during streams', async () => {
  const el = mount('app-work-group', {
    label: 'Worked for 12s',
    segments: [{
      id: 'segment-1',
      commentary: 'Checking the source.',
      tools: [{ id: 'tool-1', seq: 1, createdAt: 1, role: 'tool', kind: 'tool_activity', format: 'markdown', text: 'token 0', thinking: '', toolName: 'Ran command', toolDetail: 'rg flags', status: 'running', isError: false, pending: true }],
    }],
  });
  const fold = el.shadowRoot.querySelector('.fold-toggle');
  assert.equal(fold.getAttribute('aria-expanded'), 'false');
  assert.equal(el.shadowRoot.querySelector('app-tool-activity'), null);
  fold.click();
  assert.equal(fold.getAttribute('aria-expanded'), 'true');
  const cluster = el.shadowRoot.querySelector('app-tool-cluster');
  const tool = cluster.shadowRoot.querySelector('app-tool-activity');
  assert.ok(tool);
  tool.shadowRoot.querySelector('button').click();

  for (let i = 1; i <= 20; i += 1) {
    el.segments = [{
      id: 'segment-1',
      commentary: `Checking the source ${i}.`,
      tools: [{ id: 'tool-1', seq: 1, createdAt: 1, role: 'tool', kind: 'tool_activity', format: 'markdown', text: `token ${i}`, thinking: '', toolName: 'Ran command', toolDetail: 'rg flags', status: 'running', isError: false, pending: true }],
    }];
    await tick();
    const current = el.shadowRoot.querySelector('app-tool-cluster').shadowRoot.querySelector('app-tool-activity');
    assert.equal(current, tool);
    assert.equal(fold.getAttribute('aria-expanded'), 'true');
    assert.equal(current.shadowRoot.querySelector('button').getAttribute('aria-expanded'), 'true');
  }

  assert.match(tool.shadowRoot.innerHTML, /token 20/);
  assert.match(el.shadowRoot.querySelector('auto-markdown').shadowRoot.textContent, /Checking the source 20/);
});

test('app-prompt-input submits on Enter and clears', () => {
  const el = mount('app-prompt-input');
  const submitted = lastEvent(el, 'prompt-submit');
  const textarea = el.shadowRoot.querySelector('textarea');
  textarea.value = '  hello stride  ';
  textarea.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
  assert.equal(submitted.detail.value, 'hello stride');
  assert.equal(textarea.value, '');
});

test('app-prompt-input populates model picker when models prop updates', () => {
  const el = mount('app-prompt-input');
  const picker = () => el.shadowRoot.querySelector('app-model-picker');
  assert.match(picker().shadowRoot.textContent, /No models available/);

  el.models = [
    { value: 'default', label: 'GPT-4.1', description: 'OpenAI flagship model', vision: true },
    { value: 'claude_sonnet_4', label: 'Claude Sonnet 4', description: 'Fast general-purpose model', vision: false },
  ];
  el.selectedModel = 'claude_sonnet_4';
  el.selectedModelLabel = 'Claude Sonnet 4';
  assert.match(picker().shadowRoot.textContent, /Claude Sonnet 4/);
  assert.equal(picker().hasAttribute('disabled'), false);
});

test('app-prompt-input submits the selected model', () => {
  const el = mount('app-prompt-input', {
    models: [
      { value: 'default', label: 'Default', description: 'Balanced model', vision: false },
      { value: 'fast-model', label: 'Fast Model', description: 'Quick replies', vision: false },
    ],
    selectedModel: 'default',
  });
  const submitted = lastEvent(el, 'prompt-submit');
  const changed = lastEvent(el, 'model-change');
  const picker = el.shadowRoot.querySelector('app-model-picker');
  const textarea = el.shadowRoot.querySelector('textarea');

  picker.shadowRoot.querySelector('.trigger-button').click();
  assert.equal(picker.hasAttribute('open'), true);
  const option = picker.shadowRoot.querySelectorAll('model-picker-option')[1];
  option.shadowRoot.querySelector('button').click();
  textarea.value = 'use fast model';
  textarea.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));

  assert.equal(changed.detail.value, 'fast-model');
  assert.equal(submitted.detail.model, 'fast-model');
});

test('app-prompt-input escapes model picker labels and descriptions', () => {
  const el = mount('app-prompt-input', {
    models: [{ value: 'model"quoted', label: '<strong>Quoted</strong>', description: '<em>Unsafe</em>', vision: true }],
    selectedModel: 'model"quoted',
  });
  const picker = el.shadowRoot.querySelector('app-model-picker');
  picker.shadowRoot.querySelector('.trigger-button').click();
  const option = picker.shadowRoot.querySelector('model-picker-option');

  assert.match(option.shadowRoot.textContent, /<strong>Quoted<\/strong>/);
  assert.match(option.shadowRoot.textContent, /<em>Unsafe<\/em>/);
  assert.equal(option.shadowRoot.querySelector('strong'), null);
  assert.equal(option.shadowRoot.querySelector('em'), null);
});

test('app-prompt-input swaps its primary action for stop while running', () => {
  const el = mount('app-prompt-input');
  const action = () => el.shadowRoot.querySelector('.primary-action app-button');
  assert.equal(action().getAttribute('aria-label'), 'Record voice message');
  el.running = true;
  const stopped = lastEvent(el, 'prompt-stop');
  action().shadowRoot.querySelector('button').click();
  assert.equal(stopped.count, 1);
  assert.ok(el.shadowRoot.querySelector('textarea').disabled);
});

test('app-prompt-input uses its primary action for voice recording', () => {
  const el = mount('app-prompt-input');
  const mic = el.shadowRoot.querySelector('.primary-action app-button');
  assert.ok(mic);
  assert.equal(mic.getAttribute('aria-pressed'), 'false');
  assert.equal(mic.hasAttribute('disabled'), false);
});

test('app-text-input exposes a focus control for page controllers', () => {
  const el = mount('app-text-input');
  assert.equal(typeof el.focusControl, 'function');
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
  assert.equal(action.detail.action, 'open');
  assert.equal(action.detail.rowId, 'dir/sub');
  assert.equal(typeof action.detail.left, 'number');
  assert.equal(typeof action.detail.top, 'number');
});

test('app-file-explorer opens version history from file click', async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (url) => {
    assert.match(String(url), /\/api\/threads\/t1\/file-versions\?path=mortgage\.pdf/);
    return new Response(JSON.stringify({
      path: 'mortgage.pdf',
      versions: [{ version: 2, size: 9100, created_at: 1760000000000, mime_type: 'application/pdf' }],
    }), { status: 200, headers: { 'Content-Type': 'application/json' } });
  };
  try {
    const el = mount('app-file-explorer', {
      threadId: 't1',
      paneActive: false,
    });
    el.entries = [{ name: 'mortgage.pdf', path: 'mortgage.pdf', kind: 'file', sizeLabel: '8.9 KB', updatedLabel: 'Jul 4, 2026', mimeType: 'application/pdf' }];
    await tick();
    const table = el.shadowRoot.querySelector('app-data-table');
    table.shadowRoot.querySelector('button[data-row-action="open"]').click();
    await tick();
    await tick();

    assert.match(el.shadowRoot.innerHTML, /Version 2/);
    assert.match(el.shadowRoot.innerHTML, /Restore/);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test('app-file-explorer file menu renders supported actions', async () => {
  const el = mount('app-file-explorer', {
    threadId: 't1',
    paneActive: false,
  });
  el.entries = [{ name: 'mortgage.pdf', path: 'mortgage.pdf', kind: 'file', sizeLabel: '8.9 KB', updatedLabel: 'Jul 4, 2026', mimeType: 'application/pdf' }];
  await tick();
  const table = el.shadowRoot.querySelector('app-data-table');
  table.shadowRoot.querySelector('button[data-row-action="menu"]').click();
  await tick();

  const menu = el.shadowRoot.querySelector('app-dropdown-menu');
  assert.match(menu.shadowRoot.innerHTML, /Download/);
  assert.match(menu.shadowRoot.innerHTML, /Preview/);
  assert.doesNotMatch(menu.shadowRoot.innerHTML, /\[object Object\]/);
});

test('app-file-explorer file menu closes on outside click and toggles from menu button', async () => {
  const el = mount('app-file-explorer', {
    threadId: 't1',
    paneActive: false,
  });
  el.entries = [{ name: 'mortgage.pdf', path: 'mortgage.pdf', kind: 'file', sizeLabel: '8.9 KB', updatedLabel: 'Jul 4, 2026', mimeType: 'application/pdf' }];
  await tick();
  const menuButton = el.shadowRoot
    .querySelector('app-data-table')
    .shadowRoot.querySelector('button[data-row-action="menu"]');

  menuButton.click();
  await tick();
  await nextFrame();
  assert.equal(el.menuOpen, true);

  document.body.click();
  await tick();
  assert.equal(el.menuOpen, false);

  menuButton.click();
  await tick();
  assert.equal(el.menuOpen, true);

  menuButton.click();
  await tick();
  assert.equal(el.menuOpen, false);
});

test('app-dialog close icon fits its button', async () => {
  const el = mount('app-dialog', { open: true, title: 'Title', dialogId: 'test' });
  await tick();
  const icon = el.shadowRoot.querySelector('.close .icon > *');
  assert.ok(icon);
  assert.equal(getComputedStyle(icon).width, '16px');
  assert.equal(getComputedStyle(icon).height, '16px');
});

test('app-file-explorer closes version dialog from close button', async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async () => new Response(JSON.stringify({
    path: 'mortgage.pdf',
    versions: [{ version: 1, size: 100, created_at: 1760000000000, mime_type: 'application/pdf' }],
  }), { status: 200, headers: { 'Content-Type': 'application/json' } });
  try {
    const el = mount('app-file-explorer', { threadId: 't1', paneActive: false });
    el.entries = [{ name: 'mortgage.pdf', path: 'mortgage.pdf', kind: 'file', sizeLabel: '8.9 KB', updatedLabel: 'Jul 4, 2026', mimeType: 'application/pdf' }];
    await tick();
    el.shadowRoot.querySelector('app-data-table').shadowRoot.querySelector('button[data-row-action="open"]').click();
    await tick();
    await tick();
    assert.equal(el.versionsOpen, true);
    const versionsDialog = [...el.shadowRoot.querySelectorAll("app-dialog")].find(
      (dialog) => dialog.dataset.dialog === "versions",
    );
    versionsDialog.shadowRoot.querySelector(".close").click();
    await tick();
    assert.equal(el.versionsOpen, false);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test('app-settings switches sections and lists integrations', () => {
  const el = mount('app-settings');
  assert.match(el.shadowRoot.innerHTML, /Settings/);

  const layout = el.shadowRoot.querySelector('.layout');
  assert.equal(layout.getAttribute('data-active'), 'connections');
  const github = el.shadowRoot.querySelector('app-settings-github');
  assert.ok(github, 'GitHub settings component missing');
  assert.match(github.shadowRoot.innerHTML, /GitHub/);
  assert.ok(el.shadowRoot.querySelector('[data-section="email"]'), 'email tab missing');

  el.shadowRoot.querySelector('[data-section="email"]').click();
  assert.equal(layout.getAttribute('data-active'), 'email');
  assert.ok(el.shadowRoot.querySelector('app-settings-email'), 'email settings component missing');

  el.shadowRoot.querySelector('[data-section="mcp"]').click();
  assert.equal(layout.getAttribute('data-active'), 'mcp');
  assert.ok(el.shadowRoot.querySelector('app-settings-mcp'), 'MCP settings component missing');

  el.shadowRoot.querySelector('[data-section="memories"]').click();
  assert.equal(layout.getAttribute('data-active'), 'memories');
  assert.ok(el.shadowRoot.querySelector('app-settings-memory'), 'memory settings component missing');

  el.shadowRoot.querySelector('[data-section="models"]').click();
  assert.equal(layout.getAttribute('data-active'), 'models');
  assert.ok(el.shadowRoot.querySelector('app-settings-models'), 'model settings component missing');
});

test('app-settings-email lists accounts and escapes names', async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (input) => {
    if (String(input) === '/api/settings/email') {
      return Response.json([{ id: 'm1', name: '<script>x</script>', email: 'a@b.c', host: 'h', port: 1, username: 'u', inbox_mailbox: 'INBOX', sent_mailbox: 'Sent', drafts_mailbox: 'Drafts', created_at: 1 }]);
    }
    return Response.json({});
  };
  try {
    const el = mount('app-settings-email');
    await tick();
    assert.doesNotMatch(el.shadowRoot.innerHTML, /<script>x<\/script>/);
    assert.match(el.shadowRoot.innerHTML, /&lt;script&gt;/);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test('app-settings-mcp lists servers', async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (input) => {
    if (String(input) === '/api/settings/mcp') {
      return Response.json([{ id: 's1', name: 'deepwiki', url: 'https://mcp.example.com/mcp', enabled: true, created_at: 1, header_names: [], has_authorization: false }]);
    }
    return Response.json({});
  };
  try {
    const el = mount('app-settings-mcp');
    await tick();
    assert.match(el.shadowRoot.innerHTML, /deepwiki/);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test('app-settings-files lists writable folders', async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (input) => {
    if (String(input) === '/api/settings/writable-dirs') {
      return Response.json([{ id: 'd1', path: 'Documents/Notes', created_at: 1 }]);
    }
    return Response.json({});
  };
  try {
    const el = mount('app-settings-files');
    await tick();
    assert.match(el.shadowRoot.innerHTML, /Documents\/Notes/);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test('app-settings-threads renders retention settings', async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (input) => {
    if (String(input) === '/api/settings/thread-retention') {
      return Response.json({ archive_after_days: 7, remove_after_days: null });
    }
    return Response.json({});
  };
  try {
    const el = mount('app-settings-threads');
    await tick();
    assert.match(el.shadowRoot.innerHTML, /Archive inactive threads/);
    assert.equal(el.shadowRoot.querySelector('input[name="archive-days"]').getAttribute('value'), '7');
    assert.equal(el.shadowRoot.querySelector('input[name="remove-days"]').getAttribute('value'), '90');
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test('app-settings-skills lists skills and escapes titles', async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (input) => {
    if (String(input) === '/api/settings/skills') {
      return Response.json([{ id: 'sk1', name: 'debug', title: '<b>Debug</b>', description: 'Trace failures', content: 'Steps' }]);
    }
    return Response.json({});
  };
  try {
    const el = mount('app-settings-skills');
    await tick();
    assert.doesNotMatch(el.shadowRoot.innerHTML, /<b>Debug<\/b>/);
    assert.match(el.shadowRoot.innerHTML, /&lt;b&gt;Debug&lt;\/b&gt;/);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test('app-settings-memory renders memory palace management', async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (input) => {
    if (String(input) === '/api/settings/memories') {
      return Response.json({
        wings: [{ id: 'w1', name: 'stride-project', description: 'Project memory', rooms: 1, memories: 1, created_at: 1 }],
        rooms: [{ id: 'r1', wing: 'stride-project', name: 'settings', description: 'Settings work', memories: 1, created_at: 1 }],
        memories: [{ id: 'd1', wing: 'stride-project', room: 'settings', title: 'Memory UI direction', summary: 'Use a ledger and palace map.', content: 'Full stored memory text.', source: 'thread', keywords: 'settings memory', created_at: 1 }],
      });
    }
    return Response.json({});
  };
  try {
    const el = mount('app-settings-memory');
    await tick();

    assert.match(el.shadowRoot.innerHTML, /Memory palace/);
    assert.match(el.shadowRoot.innerHTML, /stride-project/);
    assert.match(el.shadowRoot.innerHTML, /Memory UI direction/);
    assert.match(el.shadowRoot.innerHTML, /Remove/);

    const search = el.shadowRoot.querySelector('input[name="memory-query"]');
    search.value = 'missing';
    search.dispatchEvent(new Event('input', { bubbles: true }));
    await tick();
    assert.match(el.shadowRoot.innerHTML, /No memories match this search/);
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test('app-settings-models renders model settings', async () => {
  const originalFetch = globalThis.fetch;
  globalThis.fetch = async (input) => {
    switch (String(input)) {
      case '/api/models':
        return Response.json([
          { key: 'main', slug: 'gpt-5', display_name: 'Main Model', description: 'Default chat model', source: 'config', provider: 'openai', vision: true, reasoning_effort: null },
          { key: 'helper', slug: 'gpt-5-mini', display_name: 'Helper Model', description: '', source: 'config', provider: 'openai', vision: false, reasoning_effort: null },
        ]);
      case '/api/settings/providers':
        return Response.json([{ id: 'p1', name: 'openai-main', kind: 'openai', url: 'https://api.openai.com/v1', created_at: 1 }]);
      case '/api/settings/user-models':
        return Response.json([{ id: 'u1', name: 'custom-sonnet', slug: 'claude-sonnet', display_name: 'Custom Sonnet', description: 'Coding helper', provider_id: 'p1', provider_name: 'anthropic', vision: false, reasoning_effort: null, created_at: 1 }]);
      case '/api/settings/agent':
        return Response.json({ subagent_allowed_models: ['helper'], subagent_guidelines: 'Prefer helper for quick scans.', using_server_defaults: false, server_default_guidelines: '' });
      default:
        return Response.json({});
    }
  };
  try {
    const el = mount('app-settings-models');
    await tick();

    assert.match(el.shadowRoot.innerHTML, /Server models/);
    assert.match(el.shadowRoot.innerHTML, /Main Model/);
    assert.match(el.shadowRoot.innerHTML, /openai-main/);
    assert.match(el.shadowRoot.innerHTML, /Custom Sonnet/);
    assert.match(el.shadowRoot.innerHTML, /Prefer helper for quick scans/);
    assert.equal(
      el.shadowRoot.querySelector('app-checkbox[data-model="helper"]').getAttribute('checked'),
      'true',
    );
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test('auth-form switches mode via a plain link', () => {
  const el = mount('auth-form', { mode: 'login' });
  assert.match(el.shadowRoot.innerHTML, /Log in/);
  // The mode toggle is a plain navigation link, robust on every page.
  const link = el.shadowRoot.querySelector('.switch a');
  assert.equal(link.getAttribute('href'), '/auth/register');
});

test('app-automations renders and opens the create modal on click', () => {
  const el = mount('app-automations', { items: [], loading: false });
  // Inline handlers must wire up without throwing (regression: onClick ref).
  assert.match(el.shadowRoot.innerHTML, /Automations/);
  const newButton = buttonWithText(el.shadowRoot, 'New automation');
  assert.ok(newButton, 'New automation button is missing');
  newButton.click();
  assert.match(el.shadowRoot.innerHTML, /New automation/);
  assert.ok(el.shadowRoot.querySelector('input[name="schedule"]'), 'create form did not open');
});

test('app-automations lists tasks and renders row controls', () => {
  const el = mount('app-automations', {
    items: [{ id: 'a1', name: 'Daily', schedule: '0 9 * * *', kind: 'agent', payload: 'x', enabled: true, created_at: 1, last_run: null, trigger_kind: 'cron', notify_kind: 'telegram' }],
  });
  assert.match(el.shadowRoot.innerHTML, /Daily/);
  assert.match(el.shadowRoot.innerHTML, /0 9 \* \* \*/);
  assert.match(el.shadowRoot.innerHTML, /telegram/);
  assert.ok(buttonWithText(el.shadowRoot, 'Run'), 'run button is missing');
  const toggle = buttonWithText(el.shadowRoot, 'On');
  assert.ok(toggle, 'toggle button is missing');
  assert.equal(toggle.textContent.trim(), 'On');
});


test('app-automations run button posts and renders returned output', async () => {
  const originalFetch = globalThis.fetch;
  const originalSetTimeout = window.setTimeout;
  const calls = [];
  let runPolls = 0;

  window.setTimeout = (callback) => {
    queueMicrotask(callback);
    return 0;
  };

  globalThis.fetch = async (input, init = {}) => {
    const path = String(input);
    calls.push({ path, method: init.method ?? 'GET' });
    if (path === '/api/automations') {
      return Response.json([{ id: 'a1', name: 'Daily', schedule: '0 9 * * *', kind: 'agent', payload: 'x', enabled: true, created_at: 1, last_run: null, trigger_kind: 'cron', notify_kind: 'none' }]);
    }
    if (path.endsWith('/run')) {
      return new Response('', { status: 202 });
    }
    if (path.endsWith('/runs')) {
      runPolls += 1;
      return Response.json(runPolls < 2 ? [] : [{
        id: 'r1',
        started_at: Math.floor(Date.now() / 1000),
        finished_at: Math.floor(Date.now() / 1000),
        status: 'success',
        output: 'automation output',
      }]);
    }
    return Response.json({});
  };

  try {
    const el = mount('app-automations', {
      items: [{ id: 'a1', name: 'Daily', schedule: '0 9 * * *', kind: 'agent', payload: 'x', enabled: true, created_at: 1, last_run: null, trigger_kind: 'cron', notify_kind: 'none' }],
    });
    buttonWithText(el.shadowRoot, 'Run').click();
    for (let i = 0; i < 8; i += 1) {
      await new Promise((resolve) => setTimeout(resolve, 0));
    }

    assert.ok(calls.some((call) => call.path === '/api/automations/a1/run' && call.method === 'POST'));
    assert.match(el.shadowRoot.innerHTML, /automation output/);
  } finally {
    globalThis.fetch = originalFetch;
    window.setTimeout = originalSetTimeout;
  }
});

test('app-badge renders slotted content', () => {
  const el = mount('app-badge');
  el.textContent = 'New';
  assert.ok(el.shadowRoot.querySelector('.badge slot'));
  assert.equal(el.textContent, 'New');
});

test('app-alert renders title and description slot', () => {
  const el = mount('app-alert', { title: 'Heads up' });
  assert.match(el.shadowRoot.innerHTML, /Heads up/);
  assert.ok(el.shadowRoot.querySelector('.description slot'));
});

test('app-progress reflects the clamped value', () => {
  const el = mount('app-progress', { value: '40' });
  assert.equal(el.shadowRoot.querySelector('.indicator').style.width, '40%');
  assert.equal(el.shadowRoot.querySelector('.track').getAttribute('aria-valuenow'), '40');

  const over = mount('app-progress', { value: '250' });
  assert.equal(over.shadowRoot.querySelector('.indicator').style.width, '100%');
});

// The interactive controls are fully controlled: a click only dispatches the
// proposed next state, and the rendered state re-syncs when the prop changes.
test('app-checkbox is controlled: dispatches intent, renders from prop', () => {
  const el = mount('app-checkbox');
  const changed = lastEvent(el, 'change');
  const box = el.shadowRoot.querySelector('button[role="checkbox"]');
  assert.equal(box.getAttribute('aria-checked'), 'false');
  box.click();
  // No self-mutation: state still reflects the (unchanged) prop.
  assert.equal(changed.detail.checked, true);
  assert.equal(el.shadowRoot.querySelector('button[role="checkbox"]').getAttribute('aria-checked'), 'false');
  // Parent applies the change; the box re-syncs.
  el.checked = true;
  assert.equal(el.shadowRoot.querySelector('button[role="checkbox"]').getAttribute('aria-checked'), 'true');
});

test('app-checkbox honours disabled', () => {
  const el = mount('app-checkbox', { disabled: true });
  const changed = lastEvent(el, 'change');
  el.shadowRoot.querySelector('button[role="checkbox"]').click();
  assert.equal(changed.count, 0);
});

test('app-switch is controlled: dispatches intent, renders from prop', () => {
  const el = mount('app-switch');
  const changed = lastEvent(el, 'change');
  el.shadowRoot.querySelector('button[role="switch"]').click();
  assert.equal(changed.detail.checked, true);
  assert.equal(el.shadowRoot.querySelector('button[role="switch"]').getAttribute('aria-checked'), 'false');
  el.checked = true;
  assert.equal(el.shadowRoot.querySelector('button[role="switch"]').getAttribute('aria-checked'), 'true');
});

test('app-toggle is controlled: dispatches intent, renders from prop', () => {
  const el = mount('app-toggle');
  const pressed = lastEvent(el, 'pressed-change');
  el.shadowRoot.querySelector('button').click();
  assert.equal(pressed.detail.pressed, true);
  assert.equal(el.shadowRoot.querySelector('button').getAttribute('aria-pressed'), 'false');
  el.pressed = true;
  assert.equal(el.shadowRoot.querySelector('button').getAttribute('aria-pressed'), 'true');
});

test('app-radio-group is controlled: dispatches intent, renders from prop', () => {
  const el = mount('app-radio-group', { options: [{ value: 'a', label: 'Alpha' }, { value: 'b', label: 'Beta' }] });
  assert.match(el.shadowRoot.innerHTML, /Alpha/);
  const changed = lastEvent(el, 'value-change');
  el.shadowRoot.querySelector('[data-value="b"]').click();
  assert.equal(changed.detail.value, 'b');
  assert.equal(el.shadowRoot.querySelector('[data-value="b"]').getAttribute('aria-checked'), 'false');
  el.value = 'b';
  assert.equal(el.shadowRoot.querySelector('[data-value="b"]').getAttribute('aria-checked'), 'true');
});

test('app-slider dispatches numeric value on input', () => {
  const el = mount('app-slider', { value: '20' });
  const changed = lastEvent(el, 'value-change');
  const input = el.shadowRoot.querySelector('input[type="range"]');
  input.value = '65';
  input.dispatchEvent(new Event('input', { bubbles: true }));
  assert.equal(changed.detail.value, 65);
});

test('app-tabs is controlled: dispatches intent, renders from prop', () => {
  const el = mount('app-tabs', { tabs: [{ value: 'one', label: 'One' }, { value: 'two', label: 'Two' }] });
  // Falls back to the first tab when no value is supplied.
  assert.equal(el.shadowRoot.querySelector('[data-value="one"]').getAttribute('aria-selected'), 'true');
  const changed = lastEvent(el, 'tab-change');
  el.shadowRoot.querySelector('[data-value="two"]').click();
  assert.equal(changed.detail.value, 'two');
  assert.equal(el.shadowRoot.querySelector('[data-value="two"]').getAttribute('aria-selected'), 'false');
  el.value = 'two';
  assert.equal(el.shadowRoot.querySelector('[data-value="two"]').getAttribute('aria-selected'), 'true');
});

test('app-side-panel switches nested tabs and removes the close control', () => {
  const panel = mount('app-side-panel', {
    open: true,
    tabs: [{ value: 'files', label: 'Files' }, { value: 'subagents', label: 'Subagents' }],
    activeTab: 'files',
  });
  const tabs = panel.shadowRoot.querySelector('app-tabs');
  const triggers = tabs.shadowRoot.querySelectorAll('[role="tab"]');
  assert.equal(triggers.length, 2);
  triggers[1].click();
  assert.equal(tabs.shadowRoot.querySelector('[aria-selected="true"]').textContent, 'Subagents');
  assert.equal(panel.shadowRoot.querySelector('slot[name="subagents"]').getAttribute('style'), '');
  assert.equal(panel.shadowRoot.querySelector('[aria-label^="Close"]'), null);
});

test('app-subagent-view loads one transcript once and renders persisted markdown', async () => {
  const originalFetch = globalThis.fetch;
  let transcriptRequests = 0;
  globalThis.fetch = async (input) => {
    const url = String(input);
    if (url.endsWith('/agents/agent-1/messages')) {
      transcriptRequests += 1;
      return Response.json([{
        id: 'message-1', seq: 3, role: 'agent', format: 'markdown', content: 'Persisted subagent answer',
        thinking: null, tool_call_name: null, tool_call_id: null, tool_calls: [], created_at: 3,
      }]);
    }
    return Response.json([]);
  };
  try {
    const view = document.createElement('app-subagent-view');
    view._agentsLoadedThread = 'thread-1';
    Object.assign(view, {
      threadId: 'thread-1', active: true,
      agents: [{ id: 'agent-1', name: 'Research options', model: 'helper', result: '', finished: true, parentToolCallId: 'call-1', agentPath: 'agent-1', createdAt: 1 }],
      selectedKey: 'agent-1',
    });
    document.body.appendChild(view);
    await tick();
    await tick();
    await tick();
    assert.equal(transcriptRequests, 1);
    assert.equal(view.transcript.length, 1);
    assert.equal(view.transcript[0].content, 'Persisted subagent answer');
    const chat = view.shadowRoot.querySelector('app-chat-view');
    assert.equal(chat.turns.length, 1);
    assert.equal(chat.turns[0].answer.text, 'Persisted subagent answer');
    view.dispatchEvent(new CustomEvent('transcript-update', { detail: { item: { ...view.transcript[0], content: 'Persisted subagent answer, streamed tail', pending: true } } }));
    assert.equal(view.shadowRoot.querySelector('app-chat-view').turns[0].answer.text, 'Persisted subagent answer, streamed tail');
  } finally {
    globalThis.fetch = originalFetch;
  }
});

test('app-accordion is controlled: dispatches intent, renders from prop', () => {
  const el = mount('app-accordion', { items: [{ value: 'a', title: 'First', content: 'Body A' }] });
  const changed = lastEvent(el, 'value-change');
  assert.doesNotMatch(el.shadowRoot.innerHTML, /Body A/);
  el.shadowRoot.querySelector('.trigger').click();
  assert.deepEqual(changed.detail.value, ['a']);
  assert.doesNotMatch(el.shadowRoot.innerHTML, /Body A/);
  el.value = ['a'];
  assert.match(el.shadowRoot.innerHTML, /Body A/);
  // Toggling an open item proposes an empty set.
  el.shadowRoot.querySelector('.trigger').click();
  assert.deepEqual(changed.detail.value, []);
});

test('app-select is controlled: opens internally, value driven by prop', () => {
  const el = mount('app-select', { options: [{ value: 'a', label: 'Alpha' }, { value: 'b', label: 'Beta' }], placeholder: 'Pick' });
  assert.match(el.shadowRoot.innerHTML, /Pick/);
  // Open/close of the dropdown is internal UI state.
  el.shadowRoot.querySelector('.trigger').click();
  assert.ok(el.hasAttribute('open'));
  const changed = lastEvent(el, 'value-change');
  el.shadowRoot.querySelector('[data-value="b"]').click();
  assert.equal(changed.detail.value, 'b');
  assert.ok(!el.hasAttribute('open'));
  // Value only shows once the prop is applied.
  assert.match(el.shadowRoot.innerHTML, /Pick/);
  el.value = 'b';
  assert.match(el.shadowRoot.innerHTML, /Beta/);
  assert.equal(el.shadowRoot.querySelector('[data-value="b"]').getAttribute('aria-selected'), 'true');
});

test('app-breadcrumb renders links and a current page', () => {
  const el = mount('app-breadcrumb', { items: [{ label: 'Home', href: '/' }, { label: 'Now' }] });
  const link = el.shadowRoot.querySelector('a');
  assert.equal(link.getAttribute('href'), '/');
  assert.match(el.shadowRoot.innerHTML, /aria-current="page"[^>]*>Now/);
});

test('app-pagination is controlled: disables edges and dispatches page-change', () => {
  const el = mount('app-pagination', { total: '5', page: '1' });
  assert.ok(el.shadowRoot.querySelector('.prev').disabled);
  const changed = lastEvent(el, 'page-change');
  el.shadowRoot.querySelector('[data-page="3"]').click();
  assert.equal(changed.detail.page, 3);
  // No self-mutation; the active page only moves when the prop changes.
  assert.equal(el.shadowRoot.querySelector('[aria-current="page"]').textContent, '1');
  el.page = '3';
  assert.equal(el.shadowRoot.querySelector('[aria-current="page"]').textContent, '3');
});

test('app-dialog is controlled: visibility from prop, close dispatches only', () => {
  const el = mount('app-dialog', { title: 'Title' });
  assert.match(el.shadowRoot.querySelector('.overlay').getAttribute('style'), /display:\s*none/);
  el.open = true;
  assert.match(el.shadowRoot.querySelector('.overlay').getAttribute('style'), /display:\s*flex/);
  const closed = lastEvent(el, 'close');
  el.shadowRoot.querySelector('.close').click();
  assert.equal(closed.count, 1);
  // Still open until the parent applies the close.
  assert.match(el.shadowRoot.querySelector('.overlay').getAttribute('style'), /display:\s*flex/);
  el.open = false;
  assert.match(el.shadowRoot.querySelector('.overlay').getAttribute('style'), /display:\s*none/);
});

test('app-dropdown-menu renders object items with labels', async () => {
  const el = mount('app-dropdown-menu', {
    open: true,
    items: [
      { label: 'Rename', action: 'rename' },
      { label: 'Delete', action: 'delete', variant: 'destructive' },
    ],
  });
  await tick();
  assert.match(el.shadowRoot.innerHTML, /Rename/);
  assert.match(el.shadowRoot.innerHTML, /Delete/);
  assert.doesNotMatch(el.shadowRoot.innerHTML, /\[object Object\]/);
});

test('app-alert-dialog is controlled: reports confirm and cancel', () => {
  const el = mount('app-alert-dialog', { open: true });
  const answered = lastEvent(el, 'response');
  el.shadowRoot.querySelector('.action').click();
  assert.equal(answered.detail.confirmed, true);
  // No self-close; the dialog stays until the parent closes it.
  assert.match(el.shadowRoot.querySelector('.overlay').getAttribute('style'), /display:\s*flex/);
  el.shadowRoot.querySelector('.cancel').click();
  assert.equal(answered.detail.confirmed, false);
});
