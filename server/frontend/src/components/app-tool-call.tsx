import { Component, css, onMount, state } from "@frontiers-labs/argon";
import { IconChevronDown } from "./icons/chevron-down.js";
import { IconChevronRight } from "./icons/chevron-right.js";
import { IconCheck } from "./icons/check.js";
import { IconX } from "./icons/x.js";
import { AutoMarkdown } from "./auto-markdown.js";

const styles = css`
  :host {
    display: block;
  }

  .row {
    border: 1px solid var(--border, #e4e4e7);
    border-radius: 8px;
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
    padding: 8px 10px;
    text-align: left;
    width: 100%;
  }

  .header:hover {
    background: var(--muted, rgba(0, 0, 0, 0.03));
  }

  .status {
    align-items: center;
    color: var(--muted-foreground, #71717a);
    display: inline-flex;
    flex: 0 0 14px;
    height: 14px;
    justify-content: center;
    width: 14px;
  }

  .status > * {
    height: 14px;
    width: 14px;
  }

  .spinner {
    animation: spin 0.8s linear infinite;
    border: 2px solid var(--border, #e4e4e7);
    border-radius: 50%;
    border-top-color: var(--primary, #18181b);
    box-sizing: border-box;
    height: 12px;
    width: 12px;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .status.finished {
    color: var(--primary, #18181b);
  }

  .status.failed,
  .status.cancelled,
  .status.interrupted {
    color: var(--destructive, #dc2626);
  }

  .name {
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .timer {
    color: var(--muted-foreground, #71717a);
    font-size: 0.8rem;
    margin-left: auto;
    white-space: nowrap;
  }

  .badge {
    background: var(--secondary, #f4f4f5);
    border-radius: 6px;
    color: var(--secondary-foreground, #52525b);
    font-size: 0.7rem;
    font-weight: 500;
    padding: 1px 6px;
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

  .body {
    border-top: 1px solid var(--border, #e4e4e7);
    padding: 10px;
  }

  .plain {
    font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
    font-size: 0.85rem;
    margin: 0;
    overflow-x: auto;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .result {
    border-top: 1px solid var(--border, #e4e4e7);
    margin-top: 10px;
    padding-top: 10px;
  }

  .result-label {
    color: var(--muted-foreground, #71717a);
    font-size: 0.75rem;
    font-weight: 600;
    margin-bottom: 6px;
    text-transform: uppercase;
  }
`;

function trunc(value: number): number {
  const nonneg = value < 0 ? 0 : value;
  return nonneg - (nonneg % 1);
}

function formatDuration(ms: number): string {
  const total = trunc(ms / 1000);
  const hours = trunc(total / 3600);
  const minutes = trunc((total % 3600) / 60);
  const seconds = total % 60;
  return hours > 0
    ? `${hours}h ${minutes}m ${seconds}s`
    : minutes > 0
      ? `${minutes}m ${seconds}s`
      : `${seconds}s`;
}

export function AppToolCall({
  toolCallId = "",
  name = "",
  status = "running",
  background = false,
  startedAtMs = 0,
  finishedAtMs = 0,
  open = false,
  content = "",
  format = "markdown",
  resultText = "",
}: {
  toolCallId?: string;
  name?: string;
  status?: string;
  background?: boolean;
  startedAtMs?: number;
  finishedAtMs?: number;
  open?: boolean;
  content?: string;
  format?: string;
  resultText?: string;
}): Component {
  let nowMs = state(0);
  const endMs = status === "running" ? nowMs : finishedAtMs > 0 ? finishedAtMs : nowMs;
  const elapsedMs = startedAtMs > 0 ? endMs - startedAtMs : 0;
  const timer = formatDuration(elapsedMs);
  const body = format === "markdown" ? <AutoMarkdown text={content} format="markdown" /> : <pre class="plain">{content}</pre>;
  const result = format === "markdown" ? <AutoMarkdown text={resultText} format="markdown" /> : <pre class="plain">{resultText}</pre>;
  onMount(() => {
    const tick = () => {
      nowMs = Date.now();
    };
    tick();
    const timerId = setInterval(tick, 1000);
    (timerId as { unref?: () => void }).unref?.();
    return () => clearInterval(timerId);
  });
  return (
    <>
      <style>{styles}</style>
      <div class="row" data-tool-call-id={toolCallId}>
        <button
          type="button"
          class="header"
          aria-expanded={open ? "true" : "false"}
          onClick={() => {
            this.dispatchEvent(
              new CustomEvent("toolcall-toggle", {
                bubbles: true,
                composed: true,
                detail: { open: !open },
              }),
            );
          }}
        >
          <span class={`status ${status}`} aria-hidden="true">
            {status === "running" ? (
              <span class="spinner"></span>
            ) : status === "finished" ? (
              <IconCheck />
            ) : (
              <IconX />
            )}
          </span>
          <span class="name">{name !== "" ? name : "Tool"}</span>
          {background && <span class="badge">background</span>}
          <span class="timer">{timer}</span>
          <span class="chevron" aria-hidden="true">
            {open ? <IconChevronDown /> : <IconChevronRight />}
          </span>
        </button>
        {open ? (
          <div class="body">
            {body}
            {resultText !== "" ? (
              <div class="result">
                <div class="result-label">Result</div>
                {result}
              </div>
            ) : (
              ""
            )}
          </div>
        ) : (
          ""
        )}
      </div>
    </>
  );
}
