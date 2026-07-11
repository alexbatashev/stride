import { Component, css, state } from "@frontiers-labs/argon";
import { AppButton } from "./app-button.js";
import { AppTooltip } from "./app-tooltip.js";
import { IconCheck } from "./icons/check.js";
import { IconCopy } from "./icons/copy.js";

const styles = css`
  :host { display: block; min-height: 26px; }
  .actions { align-items: center; display: flex; gap: 2px; opacity: 0; transition: opacity 140ms ease; }
  :host(:hover) .actions, .actions:focus-within { opacity: 1; }
  app-button { color: var(--muted-foreground); }
  @media (hover: none) { .actions { opacity: 1; } }
  @media (prefers-reduced-motion: reduce) { .actions { transition: none; } }
`;

export function AppMessageActions({ text = "", align = "start" }: { text?: string; align?: string }): Component {
  let copied = state(false);
  const label = copied ? "Copied" : "Copy message";
  return <><style>{styles}</style><div class="actions" style={`justify-content: ${align === "end" ? "flex-end" : "flex-start"}`}><AppTooltip text={label}><AppButton size="icon-xs" variant="ghost" aria-label={label} onClick={async () => {
    await navigator.clipboard.writeText(text);
    copied = true;
    setTimeout(() => { copied = false; }, 1600);
  }}>{copied ? <IconCheck /> : <IconCopy />}</AppButton></AppTooltip></div></>;
}
