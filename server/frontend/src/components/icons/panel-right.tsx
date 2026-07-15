import { Component, css } from "@frontiers-labs/argon";

const styles = css`
  :host { align-items: center; display: inline-flex; height: 16px; width: 16px; }
  svg { height: 100%; width: 100%; }
`;

export function IconPanelRight(): Component {
  return <><style>{styles}</style><svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2" /><path d="M15 4v16" /></svg></>;
}
