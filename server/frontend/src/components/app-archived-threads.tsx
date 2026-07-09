import { Component, css, emit, onMount } from "@frontiers-labs/argon";
import { listArchivedThreads, type ArchivedThread } from "../api/threads.js";

type Host = HTMLElement & {
  threads: ArchivedThread[];
  loaded: boolean;
  error: string;
};

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function formatDate(ms: number): string {
  if (!ms) return "unknown";
  return new Intl.DateTimeFormat(undefined, { dateStyle: "medium" }).format(new Date(ms));
}

async function refresh(host: Host): Promise<void> {
  try {
    host.threads = await listArchivedThreads();
    host.loaded = true;
    host.error = "";
  } catch {
    host.error = "Failed to load archived threads.";
  }
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
    padding: 32px 24px 64px;
  }

  .shell {
    display: flex;
    flex-direction: column;
    gap: 20px;
    margin: 0 auto;
    max-width: 760px;
    width: 100%;
  }

  h1,
  p {
    margin: 0;
  }

  .page-title {
    color: var(--foreground);
    font-size: 26px;
    letter-spacing: -0.02em;
    line-height: 1.2;
  }

  .lead {
    color: var(--muted-foreground);
    font-size: 14px;
    line-height: 1.5;
    margin-top: 6px;
  }

  .list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .row {
    align-items: center;
    border: 1px solid var(--border);
    border-radius: 10px;
    display: flex;
    gap: 16px;
    justify-content: space-between;
    padding: 12px 14px;
  }

  .info {
    min-width: 0;
  }

  .name {
    color: var(--foreground);
    font-size: 14px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .meta {
    color: var(--muted-foreground);
    font-size: 12px;
    margin-top: 3px;
  }

  .row-menu {
    align-items: center;
    background: transparent;
    border-radius: 6px;
    color: var(--muted-foreground);
    cursor: pointer;
    display: inline-flex;
    flex: 0 0 auto;
    font-size: 18px;
    height: 32px;
    justify-content: center;
    line-height: 1;
    user-select: none;
    width: 32px;
  }

  .row-menu:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .muted {
    color: var(--muted-foreground);
    font-size: 14px;
    line-height: 1.5;
  }

  @media (max-width: 760px) {
    .root {
      padding: 24px 16px 48px;
    }
  }
`;

export function AppArchivedThreads({
  threads = [],
  loaded = false,
  error = "",
}: {
  threads?: ArchivedThread[];
  loaded?: boolean;
  error?: string;
}): Component {
  onMount(() => {
    void refresh(this);
  });

  const rows = threads.map((thread) => ({
    id: thread.id,
    title: escapeHtml(thread.title || "Untitled"),
    meta: `Last active ${formatDate(thread.last_activity_at)} · Archived ${formatDate(thread.archived_at)}`,
  }));

  return (
    <>
      <style>{styles}</style>
      <div
        class="root"
        onClick={(event: Event) => {
          const trigger = (event.target as HTMLElement).closest<HTMLElement>('[data-action="thread-menu"]');
          if (!trigger) return;
          const id = trigger.dataset.threadId ?? "";
          const thread = threads.find((item) => item.id === id);
          if (!thread) return;
          emit(this, "thread-menu", { id, title: thread.title, archived: true, anchor: trigger });
        }}
      >
        <div class="shell">
          <header>
            <h1 class="page-title">Archived threads</h1>
            <p class="lead">
              Archived threads are hidden from the sidebar but keep their messages and files. Unarchive to bring one back, or delete it permanently.
            </p>
          </header>

          {error ? <p class="muted">{error}</p> : ""}

          {rows.length > 0
            ? (
              <div class="list">
                {rows
                  .map((row) => (
                    <div class="row" key={row.id}>
                      <div class="info">
                        <div class="name">{row.title}</div>
                        <div class="meta">{row.meta}</div>
                      </div>
                      <span
                        class="row-menu"
                        role="button"
                        title="Thread actions"
                        aria-label="Thread actions"
                        data-action="thread-menu"
                        data-thread-id={row.id}
                      >⋯</span>
                    </div>
                  ))
                  .join("")}
              </div>
            )
            : <p class="muted">{loaded ? "No archived threads yet." : "Loading…"}</p>}
        </div>
      </div>
    </>
  );
}
