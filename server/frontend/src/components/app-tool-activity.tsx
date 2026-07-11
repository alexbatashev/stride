import { Component, css, state } from "@frontiers-labs/argon";
import { AppSpinner } from "./app-spinner.js";
import { IconCheck } from "./icons/check.js";
import { IconChevronDown } from "./icons/chevron-down.js";
import { IconTerminal } from "./icons/terminal.js";

const styles = css`
  :host { display: block; min-width: 0; }
  .activity { color: var(--muted-foreground); }
  button { align-items: center; background: transparent; border: 0; border-radius: var(--radius-sm); color: inherit; cursor: pointer; display: flex; font: inherit; gap: 6px; max-width: 100%; padding: 2px; text-align: left; width: 100%; }
  button:hover { background: color-mix(in oklab, var(--accent) 20%, transparent); }
  button:focus-visible { box-shadow: 0 0 0 2px var(--ring-shadow); outline: none; }
  .tool-icon { align-items: center; color: color-mix(in oklab, var(--muted-foreground) 65%, transparent); display: inline-flex; flex: 0 0 auto; height: 20px; justify-content: center; width: 20px; }
  .tool-icon > * { height: 14px; opacity: 0.8; width: 14px; }
  .summary { align-items: baseline; display: flex; flex: 1; gap: 6px; min-width: 0; overflow: hidden; }
  .title { color: color-mix(in oklab, var(--foreground) 82%, transparent); flex: 0 0 auto; font-size: 0.75rem; font-weight: 500; line-height: 20px; }
  .detail { color: color-mix(in oklab, var(--muted-foreground) 55%, transparent); flex: 1; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 0.75rem; line-height: 20px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .controls { align-items: center; color: color-mix(in oklab, var(--muted-foreground) 55%, transparent); display: flex; flex: 0 0 auto; gap: 1px; }
  .control { align-items: center; display: inline-flex; height: 16px; justify-content: center; width: 16px; }
  .control > * { height: 12px; width: 12px; }
  .control.chevron > * { transition: transform 160ms ease; }
  .control.chevron.expanded > * { transform: rotate(180deg); }
  .error { color: var(--destructive); }
  pre { border-left: 1px solid color-mix(in oklab, var(--border) 45%, transparent); color: var(--muted-foreground); cursor: text; font-family: ui-monospace, SFMono-Regular, Menlo, monospace; font-size: 0.6875rem; line-height: 1.65; margin: 4px 0 2px 28px; max-height: 256px; overflow: auto; padding: 2px 0 2px 12px; white-space: pre-wrap; }
  @media (prefers-reduced-motion: reduce) { .control.chevron > * { transition: none; } }
`;

export function AppToolActivity({ activityId = "", seq = 0, title = "Tool", detail = "", content = "", status = "finished", isError = false }: { activityId?: string; seq?: number; title?: string; detail?: string; content?: string; status?: string; isError?: boolean }): Component {
  let visible = state(false);
  return <><style>{styles}</style><div class={`activity${isError ? " error" : ""}`}><button type="button" data-activity-id={activityId} data-seq={seq} aria-expanded={visible ? "true" : "false"} onClick={() => { visible = !visible; }}><span class="tool-icon" aria-hidden="true"><IconTerminal /></span><span class="summary"><span class="title">{status === "running" ? `Running ${title}` : title}</span>{detail !== "" && <span class="detail">{detail}</span>}</span><span class="controls" aria-hidden="true"><span class={`control chevron${visible ? " expanded" : ""}`}><IconChevronDown /></span><span class="control">{status === "running" ? <AppSpinner /> : <IconCheck />}</span></span></button>{visible && <pre>{content !== "" ? content : status === "running" ? "Waiting for output…" : "No output"}</pre>}</div></>;
}
