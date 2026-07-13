import { Component, css, effect, state } from "@frontiers-labs/argon";
import { WorkSegment } from "../shared/timeline.js";
import { AppSpinner } from "./app-spinner.js";
import { AppToolCluster } from "./app-tool-cluster.js";
import { AutoMarkdown } from "./auto-markdown.js";
import { IconChevronDown } from "./icons/chevron-down.js";
import { IconChevronRight } from "./icons/chevron-right.js";

const styles = css`
  :host { display: block; min-width: 0; width: 100%; }
  .fold { border-bottom: 1px solid color-mix(in oklab, var(--border) 60%, transparent); padding: 4px 0 8px; }
  .fold-toggle { align-items: center; background: transparent; border: 0; border-radius: var(--radius-sm); color: var(--muted-foreground); cursor: pointer; display: inline-flex; font: inherit; font-size: 0.75rem; gap: 4px; line-height: 20px; padding: 1px 4px; text-align: left; }
  .fold-toggle:hover { color: var(--foreground); }
  .fold-toggle:focus-visible { box-shadow: 0 0 0 2px var(--ring-shadow); outline: none; }
  .fold-toggle app-spinner, .fold-toggle .chevron, .fold-toggle .chevron > * { height: 14px; width: 14px; }
  .chevron { align-items: center; display: inline-flex; justify-content: center; }
  .work-log { padding: 18px 4px 0; }
  .segment + .segment { margin-top: 14px; }
  .commentary { color: color-mix(in oklab, var(--foreground) 82%, transparent); font-size: 0.9375rem; line-height: 1.55; }
  .commentary + app-tool-cluster { margin-top: 10px; }
  @media (max-width: 767px) { .work-log { padding-left: 2px; padding-right: 2px; } }
`;

export function AppWorkGroup({ label = "Worked", segments = [], running = false, startedAt = 0 }: { label?: string; segments?: WorkSegment[]; running?: boolean; startedAt?: number }): Component {
  let expanded = state(running);
  let elapsed = state(0);
  effect(() => {
    if (!running) return;
    const update = () => { elapsed = startedAt > 0 ? Math.max(0, Math.floor((Date.now() - startedAt) / 1000)) : 0; };
    update();
    const timer = setInterval(update, 1000);
    return () => clearInterval(timer);
  });
  const title = running ? elapsed > 0 ? `Working for ${elapsed}s` : "Working…" : label;
  return <><style>{styles}</style><section class="fold"><button class="fold-toggle" type="button" aria-expanded={expanded ? "true" : "false"} onClick={() => { expanded = !expanded; }}><span>{title}</span>{running ? <AppSpinner /> : <span class="chevron" aria-hidden="true">{expanded ? <IconChevronDown /> : <IconChevronRight />}</span>}</button>{expanded && <div class="work-log">{segments.map((segment) => <div class="segment" key={segment.id}>{segment.commentary !== "" && <AutoMarkdown class="commentary" text={segment.commentary} format="markdown"><div class="rendered">{segment.commentary}</div></AutoMarkdown>}{segment.tools.length > 0 && <AppToolCluster tools={segment.tools} />}</div>).join("")}</div>}</section></>;
}
