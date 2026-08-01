import { Component, onMount } from "@frontiers-labs/argon";
import {
  approveInboxItem,
  declineInboxItem,
  deleteInboxItem,
  listInbox,
  type InboxItem,
} from "../api/inbox.js";
import { inbox } from "../stores/inbox-state.js";
import { inboxStyles } from "./app-inbox-styles.js";
import { IconCheck } from "./icons/check.js";
import { IconClock } from "./icons/clock.js";
import { IconInbox } from "./icons/inbox.js";
import { IconMessagesSquare } from "./icons/messages-square.js";
import { IconSparkles } from "./icons/sparkles.js";
import { IconX } from "./icons/x.js";

type Fact = { label: string; value: string; mono: boolean };

/// Every string here except `titleText` is already HTML-escaped and is
/// interpolated as markup.
///
/// ARGON-WORKAROUND(argon: text interpolation inside a `.map()` body is emitted
/// unescaped because the element type is lowered as `unknown` rather than
/// `string`): escape suggestion text ourselves instead of relying on Argon's
/// default text escaping. Attribute positions are escaped correctly, so
/// `titleText` stays raw for `aria-label`.
type InboxView = {
  id: string;
  kind: string;
  kindLabel: string;
  status: string;
  statusLabel: string;
  title: string;
  titleText: string;
  rationale: string;
  facts: Fact[];
  bodyLabel: string;
  body: string;
  stamp: string;
  outcomeHref: string;
  pending: boolean;
};

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

type InboxHost = HTMLElement & {
  items: InboxItem[];
  loading: boolean;
  error: string;
  filter: string;
  busyId: string;
};

const KIND_LABELS: Record<string, string> = {
  follow_up: "Follow-up",
  automation: "Automation",
  skill: "Skill",
};

const STATUS_LABELS: Record<string, string> = {
  pending: "Awaiting review",
  approved: "Approved",
  declined: "Declined",
  failed: "Failed",
};

function detail(item: InboxItem, key: string): string {
  const value = item.details?.[key];
  if (value === null || value === undefined) return "";
  return String(value);
}

function formatDate(seconds: number | null): string {
  if (!seconds) return "";
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(seconds * 1000));
}

function fact(label: string, value: string, mono = false): Fact[] {
  return value ? [{ label, value: escapeHtml(value), mono }] : [];
}

function factsFor(item: InboxItem): Fact[] {
  if (item.kind === "follow_up") {
    return fact("Project", detail(item, "project"));
  }
  if (item.kind === "automation") {
    const trigger = detail(item, "trigger") || "cron";
    const notify = detail(item, "notify");
    return [
      ...fact("Runs", detail(item, "automation_kind") === "python" ? "Python script" : "Agent prompt"),
      ...fact("Trigger", trigger),
      ...fact("Schedule", detail(item, "schedule"), true),
      ...fact("Notify", notify && notify !== "none" ? notify : ""),
    ];
  }
  return fact("Name", detail(item, "name"), true);
}

function bodyFor(item: InboxItem): { label: string; body: string } {
  if (item.kind === "follow_up") {
    return { label: "Opening message", body: detail(item, "message") };
  }
  if (item.kind === "automation") {
    return { label: "Task", body: detail(item, "task") };
  }
  return { label: "Skill content", body: detail(item, "content") };
}

function stampFor(item: InboxItem): string {
  if (item.status === "pending") {
    const created = formatDate(item.created_at);
    return created ? `Suggested ${created}` : "Suggested just now";
  }
  const resolved = formatDate(item.resolved_at);
  const reviewed = resolved ? `Reviewed ${resolved}` : "Reviewed";
  return item.outcome ? `${reviewed} · ${item.outcome}` : reviewed;
}

function toView(item: InboxItem): InboxView {
  const { label, body } = bodyFor(item);
  return {
    id: item.id,
    kind: item.kind,
    kindLabel: KIND_LABELS[item.kind] ?? item.kind,
    status: item.status,
    statusLabel: STATUS_LABELS[item.status] ?? item.status,
    title: escapeHtml(item.title),
    titleText: item.title,
    rationale: escapeHtml(item.rationale),
    facts: factsFor(item),
    bodyLabel: label,
    body: escapeHtml(body),
    stamp: escapeHtml(stampFor(item)),
    outcomeHref: item.outcome_href ?? "",
    pending: item.status === "pending",
  };
}

async function refresh(host: InboxHost): Promise<void> {
  host.loading = true;
  try {
    const list = await listInbox();
    host.items = list.items;
    inbox.pending = list.pending;
    host.error = "";
  } catch {
    host.error = "Failed to load the inbox.";
  } finally {
    host.loading = false;
  }
}

async function decide(host: InboxHost, id: string, approved: boolean): Promise<void> {
  if (host.busyId) return;
  host.busyId = id;
  host.error = "";
  try {
    await (approved ? approveInboxItem(id) : declineInboxItem(id));
  } catch (error) {
    host.error =
      error instanceof Error && error.message
        ? `Could not ${approved ? "approve" : "decline"} the suggestion: ${error.message}`
        : `Could not ${approved ? "approve" : "decline"} the suggestion.`;
  } finally {
    host.busyId = "";
    await refresh(host);
  }
}

/// Watch the user-events stream so a suggestion parked by a running agent shows
/// up without a reload. Mirrors the sidebar's connection: reconnect on close and
/// whenever the tab regains focus.
function watchInboxEvents(host: InboxHost): () => void {
  let socket: WebSocket | null = null;
  let retry: ReturnType<typeof setTimeout> | null = null;
  let stopped = false;

  const connect = () => {
    if (retry) {
      clearTimeout(retry);
      retry = null;
    }
    const previous = socket;
    socket = null;
    previous?.close();
    const protocol = location.protocol === "https:" ? "wss:" : "ws:";
    const next = new WebSocket(`${protocol}//${location.host}/api/events`);
    socket = next;
    next.onmessage = (event) => {
      const message = JSON.parse(event.data as string) as { kind?: { type?: string } };
      if (message.kind?.type === "inbox_updated") void refresh(host);
    };
    next.onclose = () => {
      if (socket !== next) return;
      socket = null;
      if (!stopped) retry = setTimeout(connect, 2000 + Math.random() * 3000);
    };
  };

  const reconnectOnFocus = () => connect();
  window.addEventListener("focus", reconnectOnFocus);
  connect();

  return () => {
    stopped = true;
    window.removeEventListener("focus", reconnectOnFocus);
    if (retry) clearTimeout(retry);
    socket?.close();
  };
}

async function dismiss(host: InboxHost, id: string): Promise<void> {
  if (host.busyId) return;
  host.busyId = id;
  try {
    await deleteInboxItem(id);
  } catch {
    host.error = "Could not remove the suggestion.";
  } finally {
    host.busyId = "";
    await refresh(host);
  }
}

export function AppInbox({
  items = [],
  loading = false,
  error = "",
  filter = "pending",
  busyId = "",
}: {
  items?: InboxItem[];
  loading?: boolean;
  error?: string;
  filter?: string;
  busyId?: string;
}): Component {
  onMount(() => {
    void refresh(this);
    return watchInboxEvents(this);
  });

  const all = Array.isArray(items) ? items : [];
  const pendingCount = all.filter((item) => item.status === "pending").length;
  const reviewedCount = all.length - pendingCount;
  const showPending = filter !== "reviewed";
  const visible = all
    .filter((item) => (showPending ? item.status === "pending" : item.status !== "pending"))
    .map(toView);

  return (
    <>
      <style>{inboxStyles}</style>
      <div class="root">
        <div class="content">
          <header class="hero">
            <div>
              <p class="eyebrow">
                <IconSparkles />
                Suggested by Stride
              </p>
              <h1>Inbox</h1>
              <p class="lead">
                When Stride spots something worth doing but is not sure you want it, the idea waits
                here. Approve it and it happens right away; decline it and it disappears.
              </p>
            </div>
            <div class="counter">
              <strong>{pendingCount}</strong>
              <span>awaiting you</span>
            </div>
          </header>

          <div class="tabs" role="tablist">
            <button
              class="tab"
              type="button"
              role="tab"
              aria-selected={showPending ? "true" : "false"}
              onClick={() => { this.filter = "pending"; }}
            >
              Awaiting review<span class="count">{pendingCount}</span>
            </button>
            <button
              class="tab"
              type="button"
              role="tab"
              aria-selected={showPending ? "false" : "true"}
              onClick={() => { this.filter = "reviewed"; }}
            >
              Reviewed<span class="count">{reviewedCount}</span>
            </button>
          </div>

          {visible.length === 0 ? (
            <div class="empty">
              <span class="glyph" aria-hidden="true"><IconInbox /></span>
              <strong>
                {loading ? "Loading suggestions…" : showPending ? "Nothing waiting on you" : "No reviewed suggestions yet"}
              </strong>
              <span>
                {showPending
                  ? "Keep chatting with Stride. Ideas it wants your blessing for — follow-ups, automations, new skills — will show up here."
                  : "Suggestions you approve or decline are kept here so you can look back at what you decided."}
              </span>
            </div>
          ) : (
            <div class="list">
              {visible.map((view) => (
                <article class="card" data-kind={view.kind} data-status={view.status} key={view.id}>
                  <div class="card-head">
                    <span class="kind-mark" aria-hidden="true">
                      {view.kind === "follow_up" ? <IconMessagesSquare /> : view.kind === "automation" ? <IconClock /> : <IconSparkles />}
                    </span>
                    <div class="headline">
                      <div class="title-row">
                        <span class="title">{view.title}</span>
                        <span class="chip kind">{view.kindLabel}</span>
                        {view.pending ? "" : <span class={`chip ${view.status}`}>{view.statusLabel}</span>}
                      </div>
                      {view.rationale ? <p class="rationale">{view.rationale}</p> : ""}
                    </div>
                  </div>

                  {view.facts.length > 0 ? (
                    <div class="facts">
                      {view.facts.map((entry) => (
                        <span class="fact" key={`${view.id}-${entry.label}`}>
                          <b>{entry.label}</b>
                          {entry.mono ? <code>{entry.value}</code> : <span>{entry.value}</span>}
                        </span>
                      )).join("")}
                    </div>
                  ) : ""}

                  {view.body ? (
                    <details class="body">
                      <summary>{view.bodyLabel}</summary>
                      <pre>{view.body}</pre>
                    </details>
                  ) : ""}

                  <div class="card-foot">
                    <span class="stamp">
                      {view.stamp}
                      {view.outcomeHref ? <a class="open" href={view.outcomeHref}>Open</a> : ""}
                    </span>
                    <div class="decision">
                      {view.pending ? (
                        <>
                          <button
                            class="decide approve"
                            type="button"
                            title="Approve"
                            aria-label={`Approve: ${view.titleText}`}
                            disabled={busyId === view.id}
                            onClick={() => { void decide(this, view.id, true); }}
                          >
                            <IconCheck />
                            Approve
                          </button>
                          <button
                            class="decide reject"
                            type="button"
                            title="Decline"
                            aria-label={`Decline: ${view.titleText}`}
                            disabled={busyId === view.id}
                            onClick={() => { void decide(this, view.id, false); }}
                          >
                            <IconX />
                            Decline
                          </button>
                        </>
                      ) : (
                        <button
                          class="decide quiet"
                          type="button"
                          aria-label={`Remove: ${view.titleText}`}
                          disabled={busyId === view.id}
                          onClick={() => { void dismiss(this, view.id); }}
                        >
                          <IconX />
                          Remove
                        </button>
                      )}
                    </div>
                  </div>
                </article>
              )).join("")}
            </div>
          )}

          <p class="error">{error}</p>
        </div>
      </div>
    </>
  );
}
