import { Component, css, state } from "@frontiers-labs/argon";
import { TimelineItem } from "../shared/timeline.js";
import { AppToolActivity } from "./app-tool-activity.js";
import { IconChevronDown } from "./icons/chevron-down.js";

const styles = css`
  :host { display: block; min-width: 0; }
  .tools { display: flex; flex-direction: column; gap: 1px; }
  .toggle { align-items: center; background: transparent; border: 0; border-radius: var(--radius-sm); color: color-mix(in oklab, var(--foreground) 82%, transparent); cursor: pointer; display: flex; font: inherit; font-size: 0.75rem; font-weight: 500; gap: 6px; line-height: 20px; padding: 2px; text-align: left; width: 100%; }
  .toggle:hover { background: color-mix(in oklab, var(--accent) 20%, transparent); }
  .toggle:focus-visible { box-shadow: 0 0 0 2px var(--ring-shadow); outline: none; }
  .toggle-icon { align-items: center; color: color-mix(in oklab, var(--muted-foreground) 65%, transparent); display: inline-flex; height: 20px; justify-content: center; width: 20px; }
  .toggle-icon > * { height: 14px; transition: transform 160ms ease; width: 14px; }
  .toggle-icon.expanded > * { transform: rotate(180deg); }
  @media (prefers-reduced-motion: reduce) { .toggle-icon > * { transition: none; } }
`;

export function AppToolCluster({ tools = [] }: { tools?: TimelineItem[] }): Component {
  let previousVisible = state(false);
  const previous = tools.slice(0, -1);
  const newest = tools.slice(-1);
  return <><style>{styles}</style><div class="tools">{previousVisible && previous.map((tool) => <AppToolActivity key={tool.id} activityId={tool.id} seq={tool.seq} title={tool.toolName !== "" ? tool.toolName : "Tool output"} detail={tool.toolDetail} content={tool.text} status={tool.status} isError={tool.isError} />).join("")}{newest.map((tool) => <AppToolActivity key={tool.id} activityId={tool.id} seq={tool.seq} title={tool.toolName !== "" ? tool.toolName : "Tool output"} detail={tool.toolDetail} content={tool.text} status={tool.status} isError={tool.isError} />).join("")}{previous.length > 0 && <button class="toggle" type="button" aria-expanded={previousVisible ? "true" : "false"} onClick={() => { previousVisible = !previousVisible; }}><span class={`toggle-icon${previousVisible ? " expanded" : ""}`} aria-hidden="true"><IconChevronDown /></span><span>{previousVisible ? "Show fewer tool calls" : `+${previous.length} previous tool call${previous.length === 1 ? "" : "s"}`}</span></button>}</div></>;
}
