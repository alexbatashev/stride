import { Component, css, onMount, state } from "@frontiers-labs/argon";
import { IconChevronDown } from "./icons/chevron-down.js";
import { IconChevronRight } from "./icons/chevron-right.js";

const styles = css`
  :host {
    display: block;
  }

  .group {
    border: 1px solid var(--border, #e4e4e7);
    border-radius: 12px;
    overflow: hidden;
  }

  .header {
    align-items: center;
    background: transparent;
    border: 0;
    color: var(--foreground, #18181b);
    cursor: pointer;
    display: flex;
    font: inherit;
    font-size: 0.9rem;
    gap: 8px;
    padding: 10px 12px;
    text-align: left;
    width: 100%;
  }

  .header:hover {
    background: var(--muted, rgba(0, 0, 0, 0.03));
  }

  .chevron {
    align-items: center;
    color: var(--muted-foreground, #71717a);
    display: inline-flex;
    flex: 0 0 1em;
    height: 1em;
    justify-content: center;
    width: 1em;
  }

  .chevron > * {
    height: 1em;
    width: 1em;
  }

  .label {
    font-weight: 500;
  }

  .label.running {
    color: var(--muted-foreground, #71717a);
  }

  .label.failed {
    color: var(--destructive, #dc2626);
  }

  .label.cancelled,
  .label.interrupted {
    color: var(--muted-foreground, #71717a);
    font-style: italic;
  }

  .body {
    border-top: 1px solid var(--border, #e4e4e7);
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 12px;
  }
`;

function formatDuration(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const seconds = total % 60;
  if (hours > 0) return `${hours}h ${minutes}m ${seconds}s`;
  if (minutes > 0) return `${minutes}m ${seconds}s`;
  return `${seconds}s`;
}

function runLabel(status: string, elapsedMs: number): string {
  if (status === "running") return `Working… ${formatDuration(elapsedMs)}`;
  if (status === "failed") return `Failed after ${formatDuration(elapsedMs)}`;
  if (status === "cancelled") return "Cancelled";
  if (status === "interrupted") return "Interrupted";
  return `Worked for ${formatDuration(elapsedMs)}`;
}

export function AppRunGroup({
  runId = "",
  status = "running",
  startedAtMs = 0,
  finishedAtMs = 0,
  open = true,
}: {
  runId?: string;
  status?: string;
  startedAtMs?: number;
  finishedAtMs?: number;
  open?: boolean;
}): Component {
  let nowMs = state(0);
  const endMs = status === "running" ? nowMs : finishedAtMs > 0 ? finishedAtMs : nowMs;
  const elapsedMs = startedAtMs > 0 ? endMs - startedAtMs : 0;
  const label = runLabel(status, elapsedMs);
  onMount(() => {
    const tick = () => {
      nowMs = Date.now();
    };
    tick();
    const timer = setInterval(tick, 1000);
    (timer as { unref?: () => void }).unref?.();
    return () => clearInterval(timer);
  });
  return (
    <>
      <style>{styles}</style>
      <div class="group" data-run-id={runId}>
        <button
          type="button"
          class="header"
          aria-expanded={open ? "true" : "false"}
          onClick={() => {
            this.dispatchEvent(
              new CustomEvent("rungroup-toggle", {
                bubbles: true,
                composed: true,
                detail: { open: !open },
              }),
            );
          }}
        >
          <span class="chevron" aria-hidden="true">
            {open ? <IconChevronDown /> : <IconChevronRight />}
          </span>
          <span class={`label ${status}`}>{label}</span>
        </button>
        {open && (
          <div class="body">
            <slot></slot>
          </div>
        )}
      </div>
    </>
  );
}
