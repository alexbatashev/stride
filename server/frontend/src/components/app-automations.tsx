import { Component, onMount } from "@frontiers-labs/argon";
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
import { automationStyles } from "./app-automations-styles.js";

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

function triggerKindFromForm(value: string): Automation["trigger_kind"] {
  if (value === "webhook") return "webhook";
  if (value === "manual") return "manual";
  if (value === "vfs_change") return "vfs_change";
  if (value === "email") return "email";
  if (value === "gmail") return "gmail";
  return "cron";
}

function submitCreateAutomation(host: AutomationsHost, event: Event): void {
  event.preventDefault();
  const data = new FormData(event.currentTarget as HTMLFormElement);
  host.formError = "";
  const triggerKind = triggerKindFromForm(String(data.get("trigger") ?? "cron"));
  const notify = String(data.get("notify") ?? "none");
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
      host.creating = false;
      if (created.webhook_secret) {
        host.webhookUrl = `${location.origin}/api/automations/${created.id}/webhook`;
        host.webhookSecret = created.webhook_secret;
        host.webhookOpen = true;
      }
      await load(host);
      await selectAutomation(host, created);
    })
    .catch((err: unknown) => {
      host.formError =
        err instanceof Error && err.message === "400"
          ? "Check the name, cron schedule, and task."
          : "Failed to create automation.";
    });
}

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
      <style>{automationStyles}</style>
      <div class="root">
        <div class="content">
          <header class="hero">
            <div>
              <p class="eyebrow">Operations</p>
              <h1>Automations</h1>
              <p class="muted">
                Schedule recurring work, expose webhook tasks, react to file changes, and inspect every run output in one place.
              </p>
            </div>
            <button
              class="primary"
              type="button"
              onClick={() => {
                this.createTrigger = "cron";
                this.creating = true;
              }}
            >
              New automation
            </button>
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
                        <button
                          class="ghost"
                          type="button"
                          aria-label={`Select ${item.name}`}
                          onClick={() => { void selectAutomation(this, item); }}
                        >
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
                          <button
                            type="button"
                            aria-label={`Run ${item.name}`}
                            onClick={() => { void runAndRefresh(this, item).catch((runError) => reportRunError(this, runError)); }}
                          >
                            Run
                          </button>
                          <button
                            type="button"
                            aria-label={`${item.enabled ? "Pause" : "Enable"} ${item.name}`}
                            onClick={() => {
                              void setAutomationEnabled(item.id, !item.enabled)
                                .then(() => load(this))
                                .catch(() => {
                                  this.error = "Failed to update automation.";
                                });
                            }}
                          >
                            {item.enabled ? "On" : "Off"}
                          </button>
                          <button
                            class="danger"
                            type="button"
                            aria-label={`Delete ${item.name}`}
                            onClick={() => {
                              if (!window.confirm(`Delete automation "${item.name}"?`)) return;
                              void deleteAutomation(item.id)
                                .then(() => load(this))
                                .catch(() => {
                                  this.error = "Failed to delete automation.";
                                });
                            }}
                          >
                            Delete
                          </button>
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
          <div
            class="modal"
            onClick={(event: Event) => {
              if (event.target === event.currentTarget) this.creating = false;
            }}
          >
            <form class="modal-card" onSubmit={(event: Event) => { submitCreateAutomation(this, event); }}>
              <div class="modal-title">
                <div>
                  <h2>New automation</h2>
                  <p class="muted">Define when it runs, what it does, and where notifications go.</p>
                </div>
                <button type="button" onClick={() => { this.creating = false; }}>Close</button>
              </div>
              <div class="form-grid">
                <label>Name<input name="name" required placeholder="Daily report" /></label>
                <label>Trigger<select name="trigger" onChange={(event: Event) => { this.createTrigger = (event.target as HTMLSelectElement).value; }}><option value="cron">Cron schedule</option><option value="email">Incoming email</option><option value="gmail">Incoming Gmail</option><option value="webhook">Webhook (HTTP)</option><option value="vfs_change">File change</option><option value="manual">Manual only</option></select></label>
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
                <button type="button" onClick={() => { this.creating = false; }}>Cancel</button>
                <button class="primary" type="submit">Create automation</button>
              </div>
              <div class="error">{formError}</div>
            </form>
          </div>
        ) : ""}

        {webhookOpen ? (
          <div
            class="modal"
            onClick={(event: Event) => {
              if (event.target === event.currentTarget) this.webhookOpen = false;
            }}
          >
            <div class="modal-card">
              <div class="modal-title">
                <div>
                  <h2>Webhook created</h2>
                  <p class="muted">Copy the secret now. It is shown only once.</p>
                </div>
                <button type="button" onClick={() => { this.webhookOpen = false; }}>Close</button>
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
