import { css } from "@frontiers-labs/argon";

export const tableStyles = css`
  :host {
    display: block;
    height: 100%;
    min-height: 0;
  }

  .table-root {
    height: 100%;
    min-height: 0;
  }

  .table-wrap {
    height: 100%;
    overflow: auto;
  }

  table {
    border-collapse: collapse;
    table-layout: fixed;
    width: 100%;
  }

  th {
    background: var(--background);
    border-bottom: 1px solid var(--border);
    color: var(--muted-foreground);
    font-size: 11px;
    font-weight: 600;
    height: 32px;
    position: sticky;
    text-align: left;
    top: 0;
    z-index: 1;
  }

  td {
    border-bottom: 1px solid var(--border);
    color: var(--foreground);
    font-size: 13px;
    height: 42px;
    overflow: hidden;
    padding: 0 8px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  th.select,
  td.select {
    padding-left: 12px;
    width: 34px;
  }

  th.col-size,
  td.col-size {
    width: var(--table-size-width, 90px);
  }

  th.col-updated,
  td.col-updated {
    width: var(--table-updated-width, 120px);
  }

  th.col-actions,
  td.col-actions {
    padding-right: 12px;
    text-align: right;
    width: 42px;
  }

  tr:hover td {
    background: var(--accent);
  }

  input[type="checkbox"] {
    accent-color: var(--primary);
    height: 14px;
    margin: 0;
    width: 14px;
  }

  .empty {
    align-content: center;
    color: var(--muted-foreground);
    display: grid;
    font-size: 13px;
    height: 100%;
    justify-items: center;
    padding: 24px;
    text-align: center;
  }

  .cell-action {
    align-items: center;
    background: transparent;
    border: 0;
    color: inherit;
    cursor: pointer;
    display: inline-flex;
    font: inherit;
    gap: 8px;
    max-width: 100%;
    min-width: 0;
    padding: 0;
    text-align: left;
  }

  .cell-action span:last-child {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .cell-icon {
    align-items: center;
    color: var(--muted-foreground);
    display: inline-flex;
    flex: 0 0 16px;
    height: 16px;
    justify-content: center;
    width: 16px;
  }

  .cell-icon > * {
    height: 16px;
    width: 16px;
  }

  .row-menu {
    align-items: center;
    background: transparent;
    border: 0;
    border-radius: 6px;
    color: var(--muted-foreground);
    cursor: pointer;
    display: inline-flex;
    height: 28px;
    justify-content: center;
    padding: 0;
    width: 28px;
  }

  .row-menu:hover,
  .row-menu:focus-visible {
    background: var(--accent);
    color: var(--accent-foreground);
    outline: none;
  }

  .row-menu > * {
    height: 16px;
    width: 16px;
  }

  @media (max-width: 767px) {
    th.col-size,
    td.col-size,
    th.col-updated,
    td.col-updated {
      display: none;
    }
  }
`;

export const browserStyles = css`
  :host {
    background: var(--background);
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    overflow: hidden;
  }

  header {
    align-items: center;
    border-bottom: 1px solid var(--border);
    box-sizing: border-box;
    display: flex;
    flex: 0 0 auto;
    gap: 8px;
    min-height: 56px;
    padding: 12px 20px;
  }

  h1 {
    color: var(--foreground);
    flex: 1;
    font-size: 18px;
    font-weight: 650;
    margin: 0;
  }

  .action-button,
  .icon-button {
    align-items: center;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 8px;
    color: var(--foreground);
    cursor: pointer;
    display: inline-flex;
    font: inherit;
    font-size: 13px;
    font-weight: 500;
    gap: 6px;
    height: 34px;
    justify-content: center;
    outline: none;
    padding: 0 12px;
    white-space: nowrap;
  }

  .icon-button {
    padding: 0;
    width: 34px;
  }

  .action-button:hover:not(:disabled),
  .icon-button:hover:not(:disabled) {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .action-button:disabled,
  .icon-button:disabled {
    cursor: default;
    opacity: 0.5;
  }

  /* Labels are <span>; icons are custom elements. Only size the icon so a
     text-only button (e.g. Rename) isn't squeezed into a 16px box. */
  .action-button > :not(span),
  .icon-button > * {
    height: 16px;
    width: 16px;
  }

  .toolbar {
    align-items: center;
    border-bottom: 1px solid var(--border);
    display: flex;
    flex: 0 0 auto;
    gap: 6px;
    padding: 8px 20px;
  }

  .path {
    align-items: center;
    border-bottom: 1px solid var(--border);
    color: var(--muted-foreground);
    display: flex;
    flex: 0 0 auto;
    font-size: 13px;
    gap: 6px;
    min-height: 38px;
    padding: 0 20px;
  }

  .path span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .error {
    color: var(--destructive);
    font-size: 13px;
    padding: 10px 20px 0;
  }

  .error:empty {
    display: none;
  }

  input[type="file"] {
    display: none;
  }

  app-data-table {
    flex: 1;
    min-height: 0;
    padding: 0 8px;
  }

  app-file-explorer {
    flex: 1;
    min-height: 0;
  }
`;

// Inner file-explorer chrome (toolbar, path, table) shared by the /files page
// and the thread side panel. Page/panel headers live in the hosting component.
export const explorerStyles = css`
  :host {
    background: var(--background);
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    overflow: hidden;
  }

  .action-button,
  .icon-button {
    align-items: center;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 8px;
    color: var(--foreground);
    cursor: pointer;
    display: inline-flex;
    font: inherit;
    font-size: 13px;
    font-weight: 500;
    gap: 6px;
    height: 34px;
    justify-content: center;
    outline: none;
    padding: 0 12px;
    white-space: nowrap;
  }

  .icon-button {
    padding: 0;
    width: 34px;
  }

  .action-button:hover:not(:disabled),
  .icon-button:hover:not(:disabled) {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .action-button:focus-visible,
  .icon-button:focus-visible {
    box-shadow: 0 0 0 3px var(--ring-shadow);
  }

  .action-button:disabled,
  .icon-button:disabled {
    cursor: default;
    opacity: 0.5;
  }

  /* Labels are <span>; icons are custom elements. Only size the icon so a
     text-only button (e.g. Rename) isn't squeezed into a 16px box. */
  .action-button > :not(span),
  .icon-button > * {
    height: 16px;
    width: 16px;
  }

  .toolbar {
    align-items: center;
    border-bottom: 1px solid var(--border);
    display: flex;
    flex: 0 0 auto;
    gap: 6px;
    padding: 8px 20px;
  }

  .path {
    align-items: center;
    border-bottom: 1px solid var(--border);
    color: var(--muted-foreground);
    display: flex;
    flex: 0 0 auto;
    font-size: 13px;
    gap: 6px;
    min-height: 38px;
    padding: 0 20px;
  }

  .path span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .error {
    color: var(--destructive);
    font-size: 13px;
    padding: 10px 20px 0;
  }

  .error:empty {
    display: none;
  }

  input[type="file"] {
    display: none;
  }

  app-data-table {
    flex: 1;
    min-height: 0;
    padding: 0 8px;
  }

  /* Compact density for the narrow thread side panel. */
  :host([data-compact]) .toolbar {
    gap: 4px;
    min-height: 40px;
    padding: 4px 10px;
  }

  :host([data-compact]) .path {
    font-size: 12px;
    gap: 4px;
    min-height: 36px;
    padding: 0 12px;
  }

  :host([data-compact]) .error {
    font-size: 12px;
    padding: 8px 12px 0;
  }

  :host([data-compact]) .action-button,
  :host([data-compact]) .icon-button {
    font-size: 12px;
    height: 28px;
    padding: 0 8px;
  }

  :host([data-compact]) .icon-button {
    padding: 0;
    width: 28px;
  }

  :host([data-compact]) app-data-table {
    padding: 0;
    --table-size-width: 70px;
    --table-updated-width: 96px;
  }

  @media (max-width: 767px) {
    .toolbar {
      overflow-x: auto;
    }
  }
`;

export const fileManagementStyles = css`
  app-dialog[data-dialog="versions"]::part(dialog) {
    max-width: 680px;
  }

  .versions {
    display: grid;
    gap: 10px;
  }

  .version-row {
    align-items: center;
    border: 1px solid var(--border);
    border-radius: 10px;
    display: grid;
    gap: 12px;
    grid-template-columns: minmax(0, 1fr) auto auto;
    padding: 10px 12px;
  }

  .version-main {
    display: grid;
    gap: 4px;
    min-width: 0;
  }

  .version-title {
    align-items: center;
    color: var(--foreground);
    display: flex;
    font-size: 13px;
    font-weight: 600;
    gap: 8px;
  }

  .version-meta {
    color: var(--muted-foreground);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .badge {
    background: var(--accent);
    border-radius: 999px;
    color: var(--accent-foreground);
    font-size: 11px;
    font-weight: 600;
    padding: 2px 7px;
  }

  .text-button,
  .version-menu {
    align-items: center;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--foreground);
    cursor: pointer;
    display: inline-flex;
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    height: 30px;
    justify-content: center;
    padding: 0 10px;
  }

  .version-menu {
    padding: 0;
    width: 30px;
  }

  .text-button:hover,
  .version-menu:hover {
    background: var(--accent);
  }

  .version-menu > * {
    height: 16px;
    width: 16px;
  }

  .dialog-empty {
    color: var(--muted-foreground);
    font-size: 13px;
    padding: 8px 0;
  }

  .preview-frame {
    background: var(--muted);
    border: 1px solid var(--border);
    border-radius: 10px;
    height: min(72dvh, 720px);
    width: min(78dvw, 980px);
  }
`;
