import { Component, css } from "@frontiers-labs/argon";

const styles = css`:host { display: inline-flex; height: 14px; width: 14px; } svg { fill: none; height: 100%; stroke: currentColor; stroke-linecap: round; stroke-linejoin: round; stroke-width: 2; width: 100%; }`;

export function IconCopy(): Component {
  return <><style>{styles}</style><svg viewBox="0 0 24 24" aria-hidden="true"><rect width="14" height="14" x="8" y="8" rx="2"></rect><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"></path></svg></>;
}
