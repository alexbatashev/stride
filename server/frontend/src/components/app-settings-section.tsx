import { Component, css } from "@frontiers-labs/argon";

const styles = css`
  :host { display: block; }
  .section { border-bottom: 1px solid var(--border); display: grid; gap: 16px; padding: 0 0 24px; }
  .header { display: grid; gap: 4px; }
  .title { color: var(--foreground); font-size: 14px; font-weight: 600; line-height: 1.4; }
  .description { color: var(--muted-foreground); font-size: 13px; line-height: 1.5; max-width: 72ch; }
  .title:empty, .description:empty { display: none; }
  .header:not(:has(.title:not(:empty))):not(:has(.description:not(:empty))) { display: none; }
  .content { min-width: 0; }
  .footer { align-items: center; display: flex; gap: 8px; }
  .footer:not(:has(::slotted(*))) { display: none; }
`;

export function AppSettingsSection({ title = "", description = "" }: { title?: string; description?: string }): Component {
  return <><style>{styles}</style><section class="section"><div class="header"><div class="title">{title}</div><div class="description">{description}</div><slot name="header"></slot></div><div class="content"><slot></slot></div><div class="footer"><slot name="footer"></slot></div></section></>;
}
