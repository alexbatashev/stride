/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css } from "@frontiers-labs/argon";

const styles = css`
  :host { display: inline-flex; max-width: 100%; min-width: 0; }
  .attachment { align-items: center; background: var(--card); border: 1px solid var(--border); border-radius: var(--radius-xl, 14px); color: var(--card-foreground); display: flex; flex-wrap: wrap; font-size: 0.875rem; gap: 8px; min-width: 160px; max-width: 100%; padding: 8px; position: relative; transition: background-color 150ms ease, border-color 150ms ease, box-shadow 150ms ease; }
  .attachment:hover { background: color-mix(in oklab, var(--muted) 50%, transparent); }
  .attachment:focus-within { box-shadow: 0 0 0 1px var(--ring-shadow); }
  :host([state="idle"]) .attachment { border-style: dashed; }
  :host([state="error"]) .attachment { border-color: color-mix(in oklab, var(--destructive) 30%, transparent); }
  .media { align-items: center; aspect-ratio: 1; background: var(--muted); border-radius: var(--radius-md, 8px); display: flex; flex: 0 0 40px; justify-content: center; overflow: hidden; width: 40px; }
  :host([state="error"]) .media { background: var(--destructive-muted); color: var(--destructive); }
  .content { flex: 1; line-height: 1.25; max-width: 100%; min-width: 0; }
  .title { display: block; font-weight: 500; max-width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  :host([state="uploading"]) .title,
  :host([state="processing"]) .title {
    animation: attachment-shimmer 1.5s linear infinite;
    background: linear-gradient(90deg, var(--card-foreground) 20%, color-mix(in oklab, var(--card-foreground) 35%, transparent) 50%, var(--card-foreground) 80%);
    background-clip: text;
    background-size: 200% 100%;
    color: transparent;
  }
  .description { color: var(--muted-foreground); display: block; font-size: 0.75rem; margin-top: 2px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  :host([state="error"]) .description { color: color-mix(in oklab, var(--destructive) 80%, transparent); }
  .actions { align-items: center; display: flex; flex: 0 0 auto; position: relative; z-index: 2; }
  :host([size="sm"]) .attachment { font-size: 0.75rem; gap: 10px; padding: 6px; }
  :host([size="sm"]) .media { flex-basis: 32px; width: 32px; }
  :host([size="xs"]) .attachment { border-radius: var(--radius-md, 8px); font-size: 0.75rem; gap: 6px; padding: 4px; }
  :host([size="xs"]) .media { flex-basis: 28px; width: 28px; }
  :host([orientation="vertical"]) { width: 96px; }
  :host([orientation="vertical"]) .attachment { align-items: stretch; flex-direction: column; min-width: 0; width: 100%; }
  :host([orientation="vertical"]) .media { flex-basis: auto; width: 100%; }
  :host([orientation="vertical"]) .actions { position: absolute; right: 12px; top: 12px; }
  ::slotted(img[slot="media"]) { height: 100%; object-fit: cover; width: 100%; }

  @keyframes attachment-shimmer {
    from { background-position: 200% 0; }
    to { background-position: -200% 0; }
  }

  @media (prefers-reduced-motion: reduce) {
    :host([state="uploading"]) .title,
    :host([state="processing"]) .title {
      animation: none;
      background: none;
      color: inherit;
    }
  }
`;

export function AppAttachment({ title = "", description = "" }: { title?: string; description?: string }): Component {
  return <><style>{styles}</style><div class="attachment"><div class="media"><slot name="media"></slot></div><div class="content"><span class="title">{title}</span><span class="description">{description}</span><slot></slot></div><div class="actions"><slot name="actions"></slot></div></div></>;
}
