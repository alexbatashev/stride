import { css } from "@frontiers-labs/argon";

export const automationStyles = css`
  :host {
    display: block;
    height: 100%;
    min-height: 0;
    overflow: auto;
  }

  .root {
    box-sizing: border-box;
    min-height: 100%;
    padding: 32px;
  }

  .content {
    display: grid;
    gap: 20px;
    margin: 0 auto;
    max-width: 1180px;
    width: 100%;
  }

  .hero {
    align-items: flex-start;
    display: flex;
    gap: 16px;
    justify-content: space-between;
  }

  .eyebrow {
    color: var(--muted-foreground);
    font-size: 12px;
    font-weight: 600;
    letter-spacing: 0.08em;
    margin: 0 0 8px;
    text-transform: uppercase;
  }

  h1, h2, h3, p {
    margin: 0;
  }

  h1 {
    color: var(--foreground);
    font-size: 32px;
    letter-spacing: -0.03em;
    line-height: 1.1;
  }

  .muted {
    color: var(--muted-foreground);
    font-size: 14px;
    line-height: 1.6;
    margin-top: 10px;
    max-width: 720px;
  }

  .stats {
    display: grid;
    gap: 12px;
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }

  .stat, .panel, .modal-card {
    background: color-mix(in srgb, var(--card, var(--background)) 92%, transparent);
    border: 1px solid var(--border);
    border-radius: 14px;
    box-shadow: 0 1px 2px rgb(0 0 0 / 12%);
  }

  .stat {
    padding: 16px;
  }

  .stat span {
    color: var(--muted-foreground);
    display: block;
    font-size: 12px;
    font-weight: 500;
  }

  .stat strong {
    color: var(--foreground);
    display: block;
    font-size: 24px;
    margin-top: 6px;
  }

  .workspace {
    display: grid;
    gap: 20px;
    grid-template-columns: minmax(360px, 0.95fr) minmax(420px, 1.05fr);
    min-height: 520px;
  }

  .panel {
    min-width: 0;
    overflow: hidden;
  }

  .panel-head {
    align-items: center;
    border-bottom: 1px solid var(--border);
    display: flex;
    justify-content: space-between;
    padding: 16px 18px;
  }

  .panel-head h2 {
    color: var(--foreground);
    font-size: 16px;
  }

  .panel-body {
    padding: 10px;
  }

  button, .button {
    align-items: center;
    background: var(--secondary);
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--foreground);
    cursor: pointer;
    display: inline-flex;
    font: inherit;
    font-size: 14px;
    font-weight: 500;
    gap: 6px;
    height: 36px;
    justify-content: center;
    padding: 0 12px;
    transition: background-color 140ms ease, border-color 140ms ease, color 140ms ease;
    white-space: nowrap;
  }

  button:hover { background: var(--accent); }
  button.primary { background: var(--primary); border-color: var(--primary); color: var(--primary-foreground); }
  button.primary:hover { opacity: 0.9; }
  button.ghost { background: transparent; border-color: transparent; }
  button.danger { color: var(--destructive); }

  .automation-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .automation-card {
    align-items: stretch;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 12px;
    box-sizing: border-box;
    display: grid;
    gap: 12px;
    grid-template-columns: 1fr auto;
    height: auto;
    justify-content: stretch;
    padding: 14px;
    text-align: left;
    width: 100%;
  }

  .automation-card:hover, .automation-card.selected {
    background: var(--accent);
    border-color: var(--border);
  }

  .automation-card > button.ghost {
    align-items: flex-start;
    display: block;
    height: auto;
    justify-content: flex-start;
    min-width: 0;
    padding: 0;
    text-align: left;
    white-space: normal;
  }

  .name-row {
    align-items: center;
    display: flex;
    gap: 8px;
    min-width: 0;
  }

  .name {
    color: var(--foreground);
    font-size: 15px;
    font-weight: 650;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .badge {
    border: 1px solid var(--border);
    border-radius: 999px;
    color: var(--muted-foreground);
    flex: 0 0 auto;
    font-size: 11px;
    font-weight: 600;
    line-height: 1;
    padding: 4px 7px;
  }

  .badge.on { color: #22c55e; }
  .badge.off { color: var(--muted-foreground); }
  .badge.failed { color: var(--destructive); }
  .badge.running { color: #f59e0b; }

  .meta {
    color: var(--muted-foreground);
    display: flex;
    flex-wrap: wrap;
    font-size: 13px;
    gap: 8px;
    margin-top: 8px;
  }

  .row-actions {
    align-items: center;
    display: flex;
    gap: 6px;
  }

  .empty {
    align-items: center;
    color: var(--muted-foreground);
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-height: 280px;
    justify-content: center;
    padding: 24px;
    text-align: center;
  }

  .run-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .run {
    border: 1px solid var(--border);
    border-radius: 12px;
    overflow: hidden;
  }

  .run summary {
    align-items: center;
    cursor: pointer;
    display: flex;
    gap: 10px;
    list-style: none;
    padding: 12px 14px;
  }

  .run summary::-webkit-details-marker { display: none; }

  .run-meta {
    color: var(--muted-foreground);
    font-size: 12px;
    margin-left: auto;
  }

  pre {
    background: var(--muted);
    border-top: 1px solid var(--border);
    color: var(--foreground);
    font: 12px/1.6 ui-monospace, SFMono-Regular, Menlo, monospace;
    margin: 0;
    max-height: 360px;
    overflow: auto;
    padding: 14px;
    white-space: pre-wrap;
  }

  .error {
    color: var(--destructive);
    font-size: 13px;
    min-height: 18px;
  }

  .error:empty { display: none; }

  .modal {
    align-items: center;
    background: rgb(0 0 0 / 58%);
    display: flex;
    inset: 0;
    justify-content: center;
    padding: 24px;
    position: fixed;
    z-index: 50;
  }

  .modal-card {
    box-sizing: border-box;
    max-height: min(860px, 90vh);
    max-width: 720px;
    overflow: auto;
    padding: 24px;
    width: 100%;
  }

  .modal-title {
    align-items: flex-start;
    display: flex;
    gap: 16px;
    justify-content: space-between;
    margin-bottom: 20px;
  }

  .form-grid {
    display: grid;
    gap: 14px;
  }

  label {
    color: var(--foreground);
    display: flex;
    flex-direction: column;
    font-size: 13px;
    font-weight: 500;
    gap: 7px;
  }

  label.inline {
    align-items: center;
    flex-direction: row;
  }

  input, select, textarea {
    background: var(--background);
    border: 1px solid var(--border);
    border-radius: 9px;
    box-sizing: border-box;
    color: var(--foreground);
    font: inherit;
    min-height: 38px;
    padding: 8px 10px;
    width: 100%;
  }

  textarea {
    font: 13px/1.5 ui-monospace, SFMono-Regular, Menlo, monospace;
    min-height: 140px;
    resize: vertical;
  }

  input[type="checkbox"] {
    accent-color: var(--primary);
    min-height: 0;
    width: auto;
  }

  .hint {
    color: var(--muted-foreground);
    font-size: 12px;
    font-weight: 400;
    line-height: 1.4;
  }

  .actions {
    display: flex;
    gap: 8px;
    justify-content: flex-end;
    margin-top: 18px;
  }

  code, .secret {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  }

  code {
    background: var(--muted);
    border-radius: 5px;
    padding: 2px 5px;
  }

  @media (max-width: 980px) {
    .root { padding: 20px; }
    .hero { flex-direction: column; }
    .stats, .workspace { grid-template-columns: 1fr; }
  }
`;
