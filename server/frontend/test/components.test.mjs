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
    'app-file-manager', 'app-automations', 'app-settings', 'icon-arrow-up', 'icon-x',
    'app-badge', 'app-label', 'app-separator', 'app-skeleton', 'app-aspect-ratio',
    'app-card', 'app-avatar', 'app-alert', 'app-progress', 'app-checkbox',
    'app-switch', 'app-toggle', 'app-textarea', 'app-radio-group', 'app-slider',
    'app-breadcrumb', 'app-tabs', 'app-accordion', 'app-pagination', 'app-dialog',
    'app-alert-dialog', 'app-sheet', 'app-tooltip', 'app-popover', 'app-hover-card',
    'app-select', 'icon-check',
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

test('app-sidebar collapse keeps rail icons left aligned during width transition', async () => {
  const el = mount('app-sidebar');
  await Promise.resolve();

  window.dispatchEvent(new CustomEvent('app-sidebar-toggle'));
  await Promise.resolve();

  const root = el.shadowRoot.querySelector('.root');
  assert.match(root.getAttribute('class'), /collapsed/);

  const css = el.shadowRoot.querySelector('style').textContent;
  assert.match(css, /\.root\.collapsed \.nav-item a\s*{[^}]*width: var\(--sidebar-menu-button-size\)/s);
  assert.match(css, /\.root\.collapsed \.nav-item a\s*{[^}]*padding: 0 8px/s);
  assert.doesNotMatch(css, /\.root\.collapsed \.nav-item a\s*{[^}]*justify-content: center/s);
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
  const el = mount('app-message', { kind: 'tool_output', toolName: 'Shell', text: 'output' });
  const spoiler = el.shadowRoot.querySelector('app-spoiler');
  assert.ok(spoiler);
  assert.match(spoiler.shadowRoot.innerHTML, /Shell/);
  assert.doesNotMatch(spoiler.shadowRoot.innerHTML, /output/);
  spoiler.shadowRoot.querySelector('button').click();
  assert.match(spoiler.shadowRoot.innerHTML, /output/);
});

test('app-spoiler keeps open state across content updates', () => {
  const el = mount('app-spoiler', { title: 'Shell', content: 'first' });
  el.shadowRoot.querySelector('button').click();
  assert.equal(el.shadowRoot.querySelector('button').getAttribute('aria-expanded'), 'true');
  assert.match(el.shadowRoot.innerHTML, /first/);

  el.content = 'first second';
  assert.equal(el.shadowRoot.querySelector('button').getAttribute('aria-expanded'), 'true');
  assert.match(el.shadowRoot.innerHTML, /first second/);
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
  const select = () => el.shadowRoot.querySelector('select.model-picker');
  assert.equal(select().options.length, 0);

  el.models = [
    { value: 'default', label: 'GPT-4.1' },
    { value: 'claude_sonnet_4', label: 'Claude Sonnet 4' },
  ];
  el.selectedModel = 'claude_sonnet_4';
  assert.equal(select().options.length, 2);
  assert.equal(select().value, 'claude_sonnet_4');
  assert.equal(select().disabled, false);
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

test('app-prompt-input exposes a voice record button', () => {
  const el = mount('app-prompt-input');
  const mic = el.shadowRoot.querySelector('button[aria-label="Record voice message"]');
  assert.ok(mic);
  assert.equal(mic.getAttribute('aria-pressed'), 'false');
  assert.equal(mic.disabled, false);
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

test('app-settings switches sections and lists integrations', () => {
  const el = mount('app-settings', {
    emails: [{ id: 'm1', name: 'Work', email: 'you@example.com', host: 'imap.example.com', port: 993, username: 'you', inbox_mailbox: 'INBOX', sent_mailbox: 'Sent', drafts_mailbox: 'Drafts', created_at: 1 }],
    emailLoaded: true,
    mcps: [{ id: 's1', name: 'deepwiki', url: 'https://mcp.example.com/mcp', enabled: true, created_at: 1, header_names: [], has_authorization: false }],
    mcpLoaded: true,
  });
  assert.match(el.shadowRoot.innerHTML, /Settings/);

  const layout = el.shadowRoot.querySelector('.layout');
  assert.equal(layout.getAttribute('data-active'), 'connections');
  assert.match(el.shadowRoot.innerHTML, /GitHub/);
  assert.ok(el.shadowRoot.querySelector('[data-section="email"]'), 'email tab missing');

  el.shadowRoot.querySelector('[data-section="email"]').click();
  assert.equal(layout.getAttribute('data-active'), 'email');
  assert.match(el.shadowRoot.innerHTML, /Work/);

  el.shadowRoot.querySelector('[data-section="mcp"]').click();
  assert.equal(layout.getAttribute('data-active'), 'mcp');
  assert.match(el.shadowRoot.innerHTML, /deepwiki/);
});

test('app-settings renders memory palace management', () => {
  const el = mount('app-settings', {
    activeSection: 'memories',
    memoryLoaded: true,
    memoryWings: [{ id: 'w1', name: 'stride-project', description: 'Project memory', rooms: 1, memories: 1, created_at: 1 }],
    memoryRooms: [{ id: 'r1', wing: 'stride-project', name: 'settings', description: 'Settings work', memories: 1, created_at: 1 }],
    memories: [{ id: 'd1', wing: 'stride-project', room: 'settings', title: 'Memory UI direction', summary: 'Use a ledger and palace map.', content: 'Full stored memory text.', source: 'thread', keywords: 'settings memory', created_at: 1 }],
  });

  assert.match(el.shadowRoot.innerHTML, /Memory palace/);
  assert.match(el.shadowRoot.innerHTML, /stride-project/);
  assert.match(el.shadowRoot.innerHTML, /Memory UI direction/);
  assert.ok(el.shadowRoot.querySelector('[data-action="del-memory"][data-id="d1"]'), 'remove memory button missing');

  const search = el.shadowRoot.querySelector('input[name="memory-query"]');
  search.value = 'missing';
  search.dispatchEvent(new Event('input', { bubbles: true }));
  assert.match(el.shadowRoot.innerHTML, /No memories match this search/);
});

test('app-settings escapes account names', () => {
  const el = mount('app-settings', {
    activeSection: 'email',
    emails: [{ id: 'm1', name: '<script>x</script>', email: 'a@b.c', host: 'h', port: 1, username: 'u', inbox_mailbox: 'INBOX', sent_mailbox: 'Sent', drafts_mailbox: 'Drafts', created_at: 1 }],
    emailLoaded: true,
  });
  assert.doesNotMatch(el.shadowRoot.innerHTML, /<script>x<\/script>/);
  assert.match(el.shadowRoot.innerHTML, /&lt;script&gt;/);
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
  const newButton = el.shadowRoot.querySelector('[data-action="open-create"]');
  assert.ok(newButton, 'New automation button is missing');
  newButton.click();
  assert.match(el.shadowRoot.innerHTML, /New automation/);
  assert.ok(el.shadowRoot.querySelector('input[name="schedule"]'), 'create form did not open');
});

test('app-automations lists tasks and toggles enable through delegation', () => {
  const el = mount('app-automations', {
    items: [{ id: 'a1', name: 'Daily', schedule: '0 9 * * *', kind: 'agent', payload: 'x', enabled: true, created_at: 1, last_run: null, trigger_kind: 'cron', notify_kind: 'telegram' }],
  });
  assert.match(el.shadowRoot.innerHTML, /Daily/);
  assert.match(el.shadowRoot.innerHTML, /0 9 \* \* \*/);
  assert.match(el.shadowRoot.innerHTML, /telegram/);
  assert.ok(el.shadowRoot.querySelector('[data-action="run"][data-id="a1"]'), 'run button is missing');
  const toggle = el.shadowRoot.querySelector('[data-action="toggle"][data-id="a1"]');
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
    el.shadowRoot.querySelector('[data-action="run"][data-id="a1"]').click();
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
