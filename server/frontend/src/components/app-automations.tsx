import { Component, css, onMount } from "@frontiers-labs/argon";
import {
  type Automation,
  type AutomationRun,
  createAutomation,
  deleteAutomation,
  listAutomationRuns,
  listAutomations,
  runAutomation,
  setAutomationEnabled,
} from "../api/automations.js";
import { listEmailAccounts, type EmailAccount } from "../api/settings.js";

type RunView = {
  id: string;
  status: string;
  statusLabel: string;
  startedAt: number;
  finishedAt: number | null;
  startedLabel: string;
  finishedLabel: string;
  output: string;
  outputText: string;
};

type AutomationItem = Automation & {
  kindLabel: string;
  nameLabel: string;
  notifyLabel: string;
  triggerLabel: string;
  lastRunLabel: string;
};

type AutomationsHost = HTMLElement & {
  items: AutomationItem[];
  loading: boolean;
  error: string;
  creating: boolean;
  formError: string;
  selectedId: string;
  selectedName: string;
  runs: RunView[];
  runsLoading: boolean;
  webhookOpen: boolean;
  webhookUrl: string;
  webhookSecret: string;
  emailAccounts: EmailAccount[];
  createTrigger: string;
};

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function formatDate(seconds: number | null): string {
  if (!seconds) return "—";
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(seconds * 1000));
}

function titleCase(value: string): string {
  return value.replace(/_/g, " ").replace(/\b\w/g, (letter) => letter.toUpperCase());
}

function describeTrigger(item: Automation, accounts: EmailAccount[]): string {
  if (item.trigger_kind === "cron") return item.schedule || "Cron schedule";
  if (item.trigger_kind === "email") {
    const accountId = String(item.trigger_config?.account_id ?? "");
    const account = accounts.find((candidate) => candidate.id === accountId);
    return account ? `New mail in ${account.name}` : "New incoming email";
  }
  if (item.trigger_kind === "gmail") return "New Gmail";
  if (item.trigger_kind === "webhook") return "Webhook";
  if (item.trigger_kind === "vfs_change") return "File change";
  return "Manual";
}

function describeLastRun(item: Automation): string {
  return item.last_run ? `Last run ${formatDate(item.last_run)}` : "Never run";
}

function toAutomationItem(item: Automation, accounts: EmailAccount[]): AutomationItem {
  return {
    ...item,
    kindLabel: titleCase(item.kind),
    nameLabel: escapeHtml(item.name),
    notifyLabel: item.notify_kind !== "none" ? `• Notify ${titleCase(item.notify_kind)}` : "",
    triggerLabel: escapeHtml(describeTrigger(item, accounts)),
    lastRunLabel: describeLastRun(item),
  };
}

function emailAccountList(accounts: EmailAccount[] | unknown): EmailAccount[] {
  return Array.isArray(accounts) ? accounts : [];
}

function automationItemFor(host: AutomationsHost, id: string): AutomationItem | undefined {
  const item = host.items.find((candidate) => candidate.id === id);
  if (!item) return undefined;
  return item.nameLabel === undefined ? toAutomationItem(item, emailAccountList(host.emailAccounts)) : item;
}

function reportRunError(host: AutomationsHost, error: unknown): void {
  host.error = error instanceof Error ? error.message : "Failed to start run.";
}

async function load(host: AutomationsHost): Promise<void> {
  host.loading = true;
  host.error = "";
  try {
    host.items = (await listAutomations()).map((item) => toAutomationItem(item, emailAccountList(host.emailAccounts)));
    if (!host.selectedId && host.items.length > 0) {
      await selectAutomation(host, host.items[0]);
    } else if (host.selectedId && !host.items.some((item) => item.id === host.selectedId)) {
      host.selectedId = "";
      host.selectedName = "";
      host.runs = [];
    }
  } catch {
    host.error = "Failed to load automations.";
  } finally {
    host.loading = false;
  }
}

async function selectAutomation(host: AutomationsHost, item: Automation, clearRuns = true): Promise<void> {
  host.selectedId = item.id;
  host.selectedName = item.name;
  host.runsLoading = true;
  if (clearRuns) {
    host.runs = [];
  }
  try {
    const runs = await listAutomationRuns(item.id);
    host.runs = runs.map((run: AutomationRun): RunView => {
      const outputText = run.output.trim();
      return {
        id: run.id,
        status: run.status,
        statusLabel: titleCase(run.status),
        startedAt: run.started_at,
        finishedAt: run.finished_at,
        startedLabel: formatDate(run.started_at),
        finishedLabel: run.finished_at ? formatDate(run.finished_at) : "Still running",
        output: escapeHtml(outputText || "Run completed, but it did not produce text output."),
        outputText,
      };
    });
  } catch {
    host.runs = [];
    host.error = "Failed to load automation runs.";
  } finally {
    host.runsLoading = false;
  }
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => window.setTimeout(resolve, ms));
}

async function runAndRefresh(host: AutomationsHost, item: Automation): Promise<void> {
  host.error = "";
  if (host.selectedId !== item.id) {
    await selectAutomation(host, item);
  }

  const requestedAt = Math.floor(Date.now() / 1000);
  const previousLatestRunId = host.runs[0]?.id ?? "";
  const pendingRun: RunView = {
    id: `pending-${Date.now()}`,
    status: "running",
    statusLabel: "Running",
    startedAt: requestedAt,
    finishedAt: null,
    startedLabel: "Starting now",
    finishedLabel: "Still running",
    output: "Run request accepted. Waiting for scheduler output…",
    outputText: "",
  };

  host.selectedId = item.id;
  host.selectedName = item.name;
  host.runs = [pendingRun, ...host.runs];

  await runAutomation(item.id);

  for (let attempt = 0; attempt < 16; attempt += 1) {
    await delay(attempt < 4 ? 500 : 1000);
    await selectAutomation(host, item, false);
    const latest = host.runs[0];
    const isRequestedRun = latest && latest.id !== previousLatestRunId && latest.startedAt >= requestedAt - 1;
    const hasFinishedWithOutput = isRequestedRun && latest.finishedAt !== null && latest.outputText.length > 0;
    const hasSettledWithoutOutput = isRequestedRun && latest.finishedAt !== null && attempt === 15;
    if (hasFinishedWithOutput || hasSettledWithoutOutput) {
      break;
    }
  }

  await load(host);
  await selectAutomation(host, item, false);
}

const styles = css`
  :host {
    display: block;
    height: 100%;
    min-height: 0;
    overflow: auto;
  }

  .root {
    box-sizing: border-box;
    min-height: 100%;
    padding: 32px;
  }

  .content {
    display: grid;
    gap: 20px;
    margin: 0 auto;
    max-width: 1180px;
    width: 100%;
  }

  .hero {
    align-items: flex-start;
    display: flex;
    gap: 16px;
    justify-content: space-between;
  }

  .eyebrow {
    color: var(--muted-foreground);
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.08em;
    margin: 0 0 8px;
    text-transform: uppercase;
  }

  h1, h2, h3, p {
    margin: 0;
  }

  h1 {
    color: var(--foreground);
    font-size: 32px;
    letter-spacing: -0.03em;
    line-height: 1.1;
  }

  .muted {
    color: var(--muted-foreground);
    font-size: 14px;
    line-height: 1.6;
    margin-top: 10px;
    max-width: 720px;
  }

  .stats {
    display: grid;
    gap: 12px;
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }

  .stat, .panel, .modal-card {
    background: color-mix(in srgb, var(--card, var(--background)) 92%, transparent);
    border: 1px solid var(--border);
    border-radius: 14px;
    box-shadow: 0 1px 2px rgb(0 0 0 / 12%);
  }

  .stat {
    padding: 16px;
  }

  .stat span {
    color: var(--muted-foreground);
    display: block;
    font-size: 12px;
    font-weight: 500;
  }

  .stat strong {
    color: var(--foreground);
    display: block;
    font-size: 24px;
    margin-top: 6px;
  }

  .workspace {
    display: grid;
    gap: 20px;
    grid-template-columns: minmax(360px, 0.95fr) minmax(420px, 1.05fr);
    min-height: 520px;
  }

  .panel {
    min-width: 0;
    overflow: hidden;
  }

  .panel-head {
    align-items: center;
    border-bottom: 1px solid var(--border);
    display: flex;
    justify-content: space-between;
    padding: 16px 18px;
  }

  .panel-head h2 {
    color: var(--foreground);
    font-size: 16px;
  }

  .panel-body {
    padding: 10px;
  }

  button, .button {
    align-items: center;
    background: var(--secondary);
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--foreground);
    cursor: pointer;
    display: inline-flex;
    font: inherit;
    font-size: 14px;
    font-weight: 500;
    gap: 6px;
    height: 36px;
    justify-content: center;
    padding: 0 12px;
    transition: background-color 140ms ease, border-color 140ms ease, color 140ms ease;
    white-space: nowrap;
  }

  button:hover { background: var(--accent); }
  button.primary { background: var(--primary); border-color: var(--primary); color: var(--primary-foreground); }
  button.primary:hover { opacity: 0.9; }
  button.ghost { background: transparent; border-color: transparent; }
  button.danger { color: var(--destructive); }

  .automation-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .automation-card {
    align-items: stretch;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 12px;
    box-sizing: border-box;
    display: grid;
    gap: 12px;
    grid-template-columns: 1fr auto;
    height: auto;
    justify-content: stretch;
    padding: 14px;
    text-align: left;
    width: 100%;
  }

  .automation-card:hover, .automation-card.selected {
    background: var(--accent);
    border-color: var(--border);
  }

  .automation-card > button.ghost {
    align-items: flex-start;
    display: block;
    height: auto;
    justify-content: flex-start;
    min-width: 0;
    padding: 0;
    text-align: left;
    white-space: normal;
  }

  .name-row {
    align-items: center;
    display: flex;
    gap: 8px;
    min-width: 0;
  }

  .name {
    color: var(--foreground);
    font-size: 15px;
    font-weight: 650;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .badge {
    border: 1px solid var(--border);
    border-radius: 999px;
    color: var(--muted-foreground);
    flex: 0 0 auto;
    font-size: 11px;
    font-weight: 600;
    line-height: 1;
    padding: 4px 7px;
  }

  .badge.on { color: #22c55e; }
  .badge.off { color: var(--muted-foreground); }
  .badge.failed { color: var(--destructive); }
  .badge.running { color: #f59e0b; }

  .meta {
    color: var(--muted-foreground);
    display: flex;
    flex-wrap: wrap;
    font-size: 13px;
    gap: 8px;
    margin-top: 8px;
  }

  .row-actions {
    align-items: center;
    display: flex;
    gap: 6px;
  }

  .empty {
    align-items: center;
    color: var(--muted-foreground);
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 280px;
    justify-content: center;
    padding: 24px;
    text-align: center;
  }

  .run-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .run {
    border: 1px solid var(--border);
    border-radius: 12px;
    overflow: hidden;
  }

  .run summary {
    align-items: center;
    cursor: pointer;
    display: flex;
    gap: 10px;
    list-style: none;
    padding: 12px 14px;
  }

  .run summary::-webkit-details-marker { display: none; }

  .run-meta {
    color: var(--muted-foreground);
    font-size: 12px;
    margin-left: auto;
  }

  pre {
    background: var(--muted);
    border-top: 1px solid var(--border);
    color: var(--foreground);
    font: 12px/1.6 ui-monospace, SFMono-Regular, Menlo, monospace;
    margin: 0;
    max-height: 360px;
    overflow: auto;
    padding: 14px;
    white-space: pre-wrap;
  }

  .error {
    color: var(--destructive);
    font-size: 13px;
    min-height: 18px;
  }

  .error:empty { display: none; }

  .modal {
    align-items: center;
    background: rgb(0 0 0 / 58%);
    display: flex;
    inset: 0;
    justify-content: center;
    padding: 24px;
    position: fixed;
    z-index: 50;
  }

  .modal-card {
    box-sizing: border-box;
    max-height: min(860px, 90vh);
    max-width: 720px;
    overflow: auto;
    padding: 24px;
    width: 100%;
  }

  .modal-title {
    align-items: flex-start;
    display: flex;
    gap: 16px;
    justify-content: space-between;
    margin-bottom: 20px;
  }

  .form-grid {
    display: grid;
    gap: 14px;
  }

  label {
    color: var(--foreground);
    display: flex;
    flex-direction: column;
    font-size: 13px;
    font-weight: 500;
    gap: 7px;
  }

  label.inline {
    align-items: center;
    flex-direction: row;
  }

  input, select, textarea {
    background: var(--background);
    border: 1px solid var(--border);
    border-radius: 9px;
    box-sizing: border-box;
    color: var(--foreground);
    font: inherit;
    min-height: 38px;
    padding: 8px 10px;
    width: 100%;
  }

  textarea {
    font: 13px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace;
    min-height: 140px;
    resize: vertical;
  }

  input[type="checkbox"] {
    accent-color: var(--primary);
    min-height: 0;
    width: auto;
  }

  .hint {
    color: var(--muted-foreground);
    font-size: 12px;
    font-weight: 400;
    line-height: 1.4;
  }

  .actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    margin-top: 18px;
  }

  code, .secret {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }

  code {
    background: var(--muted);
    border-radius: 5px;
    padding: 2px 5px;
  }

  @media (max-width: 980px) {
    .root { padding: 20px; }
    .hero { flex-direction: column; }
    .stats, .workspace { grid-template-columns: 1fr; }
  }
`;

export function AppAutomations({
  items = [],
  loading = false,
  error = "",
  creating = false,
  formError = "",
  selectedId = "",
  selectedName = "",
  runs = [],
  runsLoading = false,
  webhookOpen = false,
  webhookUrl = "",
  webhookSecret = "",
  emailAccounts = [],
  createTrigger = "cron",
}: {
  items?: AutomationItem[];
  loading?: boolean;
  error?: string;
  creating?: boolean;
  formError?: string;
  selectedId?: string;
  selectedName?: string;
  runs?: RunView[];
  runsLoading?: boolean;
  webhookOpen?: boolean;
  webhookUrl?: string;
  webhookSecret?: string;
  emailAccounts?: EmailAccount[];
  createTrigger?: string;
}): Component {
  onMount(() => {
    void listEmailAccounts()
      .then((accounts) => { this.emailAccounts = accounts; })
      .catch(() => { this.emailAccounts = []; })
      .then(() => load(this));
  });

  const safeEmailAccounts = emailAccountList(emailAccounts);
  const viewItems = items.map((item) =>
    item.nameLabel === undefined ? toAutomationItem(item, safeEmailAccounts) : item,
  );
  const emailAccountViews = safeEmailAccounts.map((account) => ({
    id: account.id,
    label: escapeHtml(`${account.name} — ${account.email}`),
  }));
  const activeCount = viewItems.filter((item) => item.enabled).length;
  const lastRunCount = viewItems.filter((item) => item.last_run).length;

  return (
    <>
      <style>{styles}</style>
      <div
        class="root"
        onClick={(event: Event) => {
          const node = event.target as HTMLElement;
          if (node.dataset.backdrop === "create") {
            this.creating = false;
            return;
          }
          if (node.dataset.backdrop === "webhook") {
            this.webhookOpen = false;
            return;
          }
          const target = node.closest<HTMLElement>("[data-action]");
          if (!target) return;
          switch (target.dataset.action) {
            case "open-create":
              this.createTrigger = "cron";
              this.creating = true;
              return;
            case "close-create":
              this.creating = false;
              return;
            case "close-webhook":
              this.webhookOpen = false;
              return;
          }
          const item = automationItemFor(this, target.dataset.id ?? "");
          if (!item) return;
          switch (target.dataset.action) {
            case "select":
              void selectAutomation(this, item);
              break;
            case "run":
              void runAndRefresh(this, item).catch((error) => reportRunError(this, error));
              break;
            case "toggle":
              void setAutomationEnabled(item.id, !item.enabled)
                .then(() => load(this))
                .catch(() => {
                  this.error = "Failed to update automation.";
                });
              break;
            case "delete":
              if (!window.confirm(`Delete automation "${item.name}"?`)) return;
              void deleteAutomation(item.id)
                .then(() => load(this))
                .catch(() => {
                  this.error = "Failed to delete automation.";
                });
              break;
          }
        }}
        onChange={(event: Event) => {
          const target = event.target as HTMLSelectElement;
          if (target.name === "trigger") this.createTrigger = target.value;
        }}
        onSubmit={(event: Event) => {
          event.preventDefault();
          const data = new FormData(event.target as HTMLFormElement);
          this.formError = "";
          const trigger = String(data.get("trigger") ?? "cron");
          const notify = String(data.get("notify") ?? "none");
          const triggerKind =
            trigger === "webhook"
              ? "webhook"
              : trigger === "manual"
                ? "manual"
                : trigger === "vfs_change"
                  ? "vfs_change"
                  : trigger === "email"
                    ? "email"
                    : trigger === "gmail"
                      ? "gmail"
                  : "cron";
          void createAutomation({
            name: String(data.get("name") ?? "").trim(),
            schedule: String(data.get("schedule") ?? "").trim(),
            kind: data.get("kind") === "python" ? "python" : "agent",
            payload: String(data.get("payload") ?? ""),
            enabled: data.get("enabled") !== null,
            trigger_kind: triggerKind,
            notify_kind: notify === "telegram" ? "telegram" : "none",
            ...(triggerKind === "vfs_change"
              ? { trigger_config: { path: String(data.get("watch_path") ?? "").trim() } }
              : triggerKind === "email"
                ? { trigger_config: { account_id: String(data.get("email_account") ?? "") } }
              : {}),
          })
            .then(async (created) => {
              this.creating = false;
              if (created.webhook_secret) {
                this.webhookUrl = `${location.origin}/api/automations/${created.id}/webhook`;
                this.webhookSecret = created.webhook_secret;
                this.webhookOpen = true;
              }
              await load(this);
              await selectAutomation(this, created);
            })
            .catch((err: unknown) => {
              this.formError =
                err instanceof Error && err.message === "400"
                  ? "Check the name, cron schedule, and task."
                  : "Failed to create automation.";
            });
        }}
      >
        <div class="content">
          <header class="hero">
            <div>
              <p class="eyebrow">Operations</p>
              <h1>Automations</h1>
              <p class="muted">
                Schedule recurring work, expose webhook tasks, react to file changes, and inspect every run output in one place.
              </p>
            </div>
            <button class="primary" type="button" data-action="open-create">New automation</button>
          </header>

          <section class="stats" aria-label="Automation summary">
            <div class="stat"><span>Total</span><strong>{viewItems.length}</strong></div>
            <div class="stat"><span>Enabled</span><strong>{activeCount}</strong></div>
            <div class="stat"><span>With runs</span><strong>{lastRunCount}</strong></div>
          </section>

          <main class="workspace">
            <section class="panel">
              <div class="panel-head">
                <h2>Automations</h2>
                <span class="badge">{loading ? "Loading" : `${viewItems.length} total`}</span>
              </div>
              <div class="panel-body">
                {viewItems.length === 0 ? (
                  <div class="empty">
                    <strong>{loading ? "Loading automations…" : "No automations yet"}</strong>
                    <span>Create one to run tasks on a schedule, webhook, file change, or manually.</span>
                  </div>
                ) : (
                  <div class="automation-list">
                    {viewItems.map((item) => (
                      <div class={`automation-card ${item.id === selectedId ? "selected" : ""}`} data-notify-kind={item.notify_kind} key={item.id}>
                        <button class="ghost" type="button" data-action="select" data-id={item.id}>
                          <div>
                            <div class="name-row">
                              <span class="name">{item.nameLabel || item.name}</span>
                              <span class={`badge ${item.enabled ? "on" : "off"}`}>{item.enabled ? "Enabled" : "Paused"}</span>
                            </div>
                            <div class="meta">
                              <span>{item.kindLabel || item.kind}</span>
                              <span>•</span>
                              <span>{item.triggerLabel || item.schedule || item.trigger_kind}</span>
                              <span>•</span>
                              <span>{item.lastRunLabel || "Never run"}</span>
                              {item.notifyLabel ? <span>{item.notifyLabel}</span> : item.notify_kind !== "none" ? <span>{item.notify_kind}</span> : ""}
                            </div>
                          </div>
                        </button>
                        <div class="row-actions">
                          <button type="button" data-action="run" data-id={item.id}>Run</button>
                          <button type="button" data-action="toggle" data-id={item.id}>{item.enabled ? "On" : "Off"}</button>
                          <button class="danger" type="button" data-action="delete" data-id={item.id}>Delete</button>
                        </div>
                      </div>
                    )).join("")}
                  </div>
                )}
              </div>
            </section>

            <section class="panel">
              <div class="panel-head">
                <h2>{selectedName || "Run output"}</h2>
                <span class="badge">History</span>
              </div>
              <div class="panel-body">
                {!selectedId ? (
                  <div class="empty">
                    <strong>Select an automation</strong>
                    <span>Run logs and captured output will appear here.</span>
                  </div>
                ) : runs.length === 0 ? (
                  <div class="empty">
                    <strong>{runsLoading ? "Loading runs…" : "No executions yet"}</strong>
                    <span>{runsLoading ? "Fetching the latest run history." : "Click Run to start it and capture output."}</span>
                  </div>
                ) : (
                  <div class="run-list">
                    {runs.map((run, index) => (
                      <details class="run" open={index === 0} key={run.id}>
                        <summary>
                          <span class={`badge ${run.status}`}>{run.statusLabel}</span>
                          <span>{run.startedLabel}</span>
                          <span class="run-meta">Finished {run.finishedLabel}</span>
                        </summary>
                        <pre>{run.output}</pre>
                      </details>
                    )).join("")}
                  </div>
                )}
              </div>
            </section>
          </main>
          <div class="error">{error}</div>
        </div>

        {creating ? (
          <div class="modal" data-backdrop="create">
            <form class="modal-card">
              <div class="modal-title">
                <div>
                  <h2>New automation</h2>
                  <p class="muted">Define when it runs, what it does, and where notifications go.</p>
                </div>
                <button type="button" data-action="close-create">Close</button>
              </div>
              <div class="form-grid">
                <label>Name<input name="name" required placeholder="Daily report" /></label>
                <label>Trigger<select name="trigger"><option value="cron">Cron schedule</option><option value="email">Incoming email</option><option value="gmail">Incoming Gmail</option><option value="webhook">Webhook (HTTP)</option><option value="vfs_change">File change</option><option value="manual">Manual only</option></select></label>
                {createTrigger === "cron" ? <label>Schedule<input name="schedule" required placeholder="*/30 * * * *" /><span class="hint">Standard five-field cron expression in UTC.</span></label> : <input name="schedule" type="hidden" value="" />}
                {createTrigger === "email" ? <label>Inbox<select name="email_account" required><option value="">Choose an inbox</option>{emailAccountViews.map((account) => <option value={account.id}>{account.label}</option>).join("")}</select><span class="hint">Add IMAP accounts in Settings. Existing mail is ignored when the automation is created.</span></label> : ""}
                {createTrigger === "gmail" ? <span class="hint">Fires when new mail arrives in your connected Gmail inbox. Connect Google in Settings first. Existing mail is ignored when the automation is created.</span> : ""}
                {createTrigger === "vfs_change" ? <label>Watch path<input name="watch_path" placeholder="reports/ (empty means all files)" /><span class="hint">Leave empty to watch all files.</span></label> : ""}
                <label>Type<select name="kind"><option value="agent">Agent prompt</option><option value="python">Python script</option></select></label>
                <label>Notify<select name="notify"><option value="none">Store output only</option><option value="telegram">Telegram</option></select></label>
                <label>Task<textarea name="payload" required placeholder="Describe the task or paste Python code..."></textarea></label>
                <label class="inline"><input type="checkbox" name="enabled" checked /> Enabled</label>
              </div>
              <div class="actions">
                <button type="button" data-action="close-create">Cancel</button>
                <button class="primary" type="submit">Create automation</button>
              </div>
              <div class="error">{formError}</div>
            </form>
          </div>
        ) : ""}

        {webhookOpen ? (
          <div class="modal" data-backdrop="webhook">
            <div class="modal-card">
              <div class="modal-title">
                <div>
                  <h2>Webhook created</h2>
                  <p class="muted">Copy the secret now. It is shown only once.</p>
                </div>
                <button type="button" data-action="close-webhook">Close</button>
              </div>
              <div class="form-grid">
                <label>URL<input class="secret" value={webhookUrl} readonly /></label>
                <label>Secret<input class="secret" value={webhookSecret} readonly /></label>
                <p class="muted">Send a POST request with <code>X-Stride-Webhook-Secret</code> or <code>?token=</code>. JSON bodies are forwarded to the task.</p>
              </div>
            </div>
          </div>
        ) : ""}
      </div>
    </>
  );
}
