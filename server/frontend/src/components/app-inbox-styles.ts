import { css } from "@frontiers-labs/argon";

export const inboxStyles = css`
  :host {
    --inbox-approve: #16a34a;
    --inbox-approve-ink: #15803d;
    --inbox-follow-up: #6366f1;
    --inbox-automation: #0284c7;
    --inbox-skill: #d97706;
    display: block;
    height: 100%;
    min-height: 0;
    overflow: auto;
  }

  @media (prefers-color-scheme: dark) {
    :host {
      --inbox-approve: #22c55e;
      --inbox-approve-ink: #4ade80;
      --inbox-follow-up: #a5b4fc;
      --inbox-automation: #7dd3fc;
      --inbox-skill: #fcd34d;
    }
  }

  .root {
    box-sizing: border-box;
    min-height: 100%;
    padding: 40px 32px 72px;
  }

  .content {
    display: grid;
    gap: 24px;
    margin: 0 auto;
    max-width: 900px;
    width: 100%;
  }

  h1,
  h2,
  h3,
  p {
    margin: 0;
  }

  .hero {
    align-items: flex-start;
    display: flex;
    gap: 20px;
    justify-content: space-between;
  }

  .eyebrow {
    align-items: center;
    color: var(--muted-foreground);
    display: flex;
    font-size: 12px;
    font-weight: 600;
    gap: 8px;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .eyebrow icon-sparkles {
    height: 14px;
    width: 14px;
  }

  h1 {
    color: var(--foreground);
    font-size: 34px;
    letter-spacing: -0.03em;
    line-height: 1.1;
    margin-top: 10px;
  }

  .lead {
    color: var(--muted-foreground);
    font-size: 14px;
    line-height: 1.6;
    margin-top: 10px;
    max-width: 620px;
  }

  .counter {
    align-items: baseline;
    background: linear-gradient(
      145deg,
      color-mix(in srgb, var(--primary) 12%, transparent),
      transparent 70%
    );
    border: 1px solid var(--border);
    border-radius: 16px;
    display: flex;
    flex: 0 0 auto;
    gap: 10px;
    padding: 16px 20px;
  }

  .counter strong {
    color: var(--foreground);
    font-size: 30px;
    font-weight: 650;
    letter-spacing: -0.03em;
    line-height: 1;
  }

  .counter span {
    color: var(--muted-foreground);
    font-size: 12px;
    font-weight: 500;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .tabs {
    border-bottom: 1px solid var(--border);
    display: flex;
    gap: 4px;
  }

  .tab {
    background: transparent;
    border: 0;
    border-bottom: 2px solid transparent;
    color: var(--muted-foreground);
    cursor: pointer;
    font: inherit;
    font-size: 14px;
    font-weight: 500;
    margin-bottom: -1px;
    outline: none;
    padding: 10px 14px;
    transition: color 140ms ease, border-color 140ms ease;
  }

  .tab:hover,
  .tab:focus-visible {
    color: var(--foreground);
  }

  .tab[aria-selected="true"] {
    border-bottom-color: var(--primary);
    color: var(--foreground);
  }

  .tab .count {
    color: var(--muted-foreground);
    font-size: 12px;
    margin-left: 6px;
  }

  .list {
    display: grid;
    gap: 14px;
  }

  .card {
    background: color-mix(in srgb, var(--card, var(--background)) 94%, transparent);
    border: 1px solid var(--border);
    border-radius: 16px;
    box-shadow: 0 1px 2px rgb(0 0 0 / 6%);
    display: grid;
    gap: 14px;
    padding: 18px 18px 16px;
    position: relative;
    transition: border-color 160ms ease, box-shadow 160ms ease, transform 160ms ease;
  }

  .card::before {
    background: var(--accent-color, var(--primary));
    border-radius: 16px 0 0 16px;
    bottom: 12px;
    content: "";
    left: 0;
    opacity: 0.65;
    position: absolute;
    top: 12px;
    width: 3px;
  }

  .card[data-status="pending"]:hover {
    border-color: color-mix(in srgb, var(--primary) 45%, var(--border));
    box-shadow: 0 8px 26px rgb(0 0 0 / 8%);
    transform: translateY(-1px);
  }

  .card[data-status="declined"],
  .card[data-status="approved"],
  .card[data-status="failed"] {
    opacity: 0.78;
  }

  .card[data-kind="follow_up"] {
    --accent-color: var(--inbox-follow-up);
  }

  .card[data-kind="automation"] {
    --accent-color: var(--inbox-automation);
  }

  .card[data-kind="skill"] {
    --accent-color: var(--inbox-skill);
  }

  .card-head {
    align-items: flex-start;
    display: flex;
    gap: 14px;
  }

  .kind-mark {
    align-items: center;
    background: color-mix(in srgb, var(--accent-color, var(--primary)) 14%, transparent);
    border-radius: 11px;
    color: var(--accent-color, var(--primary));
    display: inline-flex;
    flex: 0 0 38px;
    height: 38px;
    justify-content: center;
    width: 38px;
  }

  .kind-mark > * {
    height: 19px;
    width: 19px;
  }

  .headline {
    display: grid;
    gap: 5px;
    min-width: 0;
  }

  .title-row {
    align-items: center;
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .title {
    color: var(--foreground);
    font-size: 15px;
    font-weight: 620;
    letter-spacing: -0.01em;
    line-height: 1.35;
  }

  .rationale {
    color: var(--muted-foreground);
    font-size: 13px;
    line-height: 1.55;
  }

  .chip {
    align-items: center;
    border: 1px solid var(--border);
    border-radius: 999px;
    color: var(--muted-foreground);
    display: inline-flex;
    font-size: 11px;
    font-weight: 600;
    gap: 5px;
    letter-spacing: 0.03em;
    line-height: 1;
    padding: 4px 9px;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .chip.kind {
    background: color-mix(in srgb, var(--accent-color, var(--primary)) 12%, transparent);
    border-color: transparent;
    color: var(--accent-color, var(--primary));
  }

  .chip.approved {
    background: color-mix(in srgb, var(--inbox-approve) 14%, transparent);
    border-color: transparent;
    color: var(--inbox-approve-ink);
  }

  .chip.declined {
    background: var(--muted, color-mix(in srgb, currentColor 8%, transparent));
    border-color: transparent;
  }

  .chip.failed {
    background: color-mix(in srgb, var(--destructive, #dc2626) 14%, transparent);
    border-color: transparent;
    color: var(--destructive, #dc2626);
  }

  .facts {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 8px;
    padding-left: 52px;
  }

  .fact {
    align-items: baseline;
    background: var(--muted, color-mix(in srgb, currentColor 6%, transparent));
    border-radius: 7px;
    color: var(--foreground);
    display: inline-flex;
    font-size: 12px;
    gap: 6px;
    padding: 4px 9px;
  }

  .fact b {
    color: var(--muted-foreground);
    font-weight: 600;
  }

  .fact code {
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11.5px;
  }

  .body {
    border: 1px solid var(--border);
    border-radius: 10px;
    margin-left: 52px;
    overflow: hidden;
  }

  .body summary {
    align-items: center;
    color: var(--muted-foreground);
    cursor: pointer;
    display: flex;
    font-size: 12px;
    font-weight: 600;
    gap: 6px;
    letter-spacing: 0.02em;
    list-style: none;
    padding: 9px 12px;
    user-select: none;
  }

  .body summary::-webkit-details-marker {
    display: none;
  }

  .body summary::after {
    content: "▾";
    font-size: 10px;
    margin-left: auto;
    transition: transform 140ms ease;
  }

  .body[open] summary::after {
    transform: rotate(180deg);
  }

  .body summary:hover {
    color: var(--foreground);
  }

  .body pre {
    border-top: 1px solid var(--border);
    color: var(--foreground);
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 12.5px;
    line-height: 1.6;
    margin: 0;
    max-height: 320px;
    overflow: auto;
    padding: 12px;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .card-foot {
    align-items: center;
    display: flex;
    gap: 12px;
    justify-content: space-between;
    padding-left: 52px;
  }

  .stamp {
    color: var(--muted-foreground);
    font-size: 12px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .stamp .open {
    color: var(--primary);
    margin-left: 8px;
    text-decoration: none;
  }

  .stamp .open:hover {
    text-decoration: underline;
  }

  .decision {
    display: flex;
    flex: 0 0 auto;
    gap: 8px;
  }

  .decide {
    align-items: center;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 9px;
    color: var(--foreground);
    cursor: pointer;
    display: inline-flex;
    font: inherit;
    font-size: 13px;
    font-weight: 550;
    gap: 7px;
    height: 34px;
    outline: none;
    padding: 0 13px;
    transition: background 140ms ease, border-color 140ms ease, color 140ms ease;
  }

  .decide > icon-check,
  .decide > icon-x {
    height: 15px;
    width: 15px;
  }

  .decide:focus-visible {
    box-shadow: 0 0 0 2px var(--ring-shadow, rgb(24 24 27 / 14%));
  }

  .decide:disabled {
    cursor: progress;
    opacity: 0.55;
  }

  .decide.approve {
    background: color-mix(in srgb, var(--inbox-approve) 12%, transparent);
    border-color: color-mix(in srgb, var(--inbox-approve) 32%, transparent);
    color: var(--inbox-approve-ink);
  }

  .decide.approve:hover:not(:disabled) {
    background: var(--inbox-approve);
    border-color: var(--inbox-approve);
    color: #05230f;
  }

  .decide.reject:hover:not(:disabled) {
    background: color-mix(in srgb, var(--destructive, #dc2626) 12%, transparent);
    border-color: color-mix(in srgb, var(--destructive, #dc2626) 32%, transparent);
    color: var(--destructive, #dc2626);
  }

  .decide.quiet {
    border-color: transparent;
    color: var(--muted-foreground);
    height: 30px;
    padding: 0 10px;
  }

  .decide.quiet:hover:not(:disabled) {
    background: var(--accent);
    color: var(--foreground);
  }

  .empty {
    align-items: center;
    border: 1px dashed var(--border);
    border-radius: 16px;
    display: grid;
    gap: 8px;
    justify-items: center;
    padding: 56px 24px;
    text-align: center;
  }

  .empty .glyph {
    align-items: center;
    background: color-mix(in srgb, var(--primary) 10%, transparent);
    border-radius: 14px;
    color: var(--primary);
    display: inline-flex;
    height: 46px;
    justify-content: center;
    margin-bottom: 4px;
    width: 46px;
  }

  .empty .glyph > * {
    height: 22px;
    width: 22px;
  }

  .empty strong {
    color: var(--foreground);
    font-size: 15px;
  }

  .empty span {
    color: var(--muted-foreground);
    font-size: 13px;
    line-height: 1.6;
    max-width: 420px;
  }

  .error {
    color: var(--destructive, #dc2626);
    font-size: 13px;
    line-height: 1.5;
    min-height: 18px;
  }

  @media (max-width: 760px) {
    .root {
      padding: 24px 16px 56px;
    }

    h1 {
      font-size: 26px;
    }

    .hero {
      flex-direction: column;
    }

    .counter {
      width: 100%;
    }

    .facts,
    .body,
    .card-foot {
      margin-left: 0;
      padding-left: 0;
    }

    .card-foot {
      align-items: stretch;
      flex-direction: column;
    }

    .decision {
      justify-content: flex-end;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .card,
    .decide,
    .tab,
    .body summary::after {
      transition: none;
    }
  }
`;
