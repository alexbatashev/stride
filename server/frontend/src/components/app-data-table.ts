import { Component, css, html } from "@frontiers-labs/argon";

export type DataTableColumn<T = Record<string, unknown>> = {
  key: string;
  header: string;
  width?: string;
  mobileHidden?: boolean;
  html?: boolean;
  render?: (row: T) => string | number | null | undefined;
};

type RowIdGetter<T> = (row: T) => string;

const styles = css`
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

  @media (max-width: 767px) {
    th[data-mobile-hidden="true"],
    td[data-mobile-hidden="true"] {
      display: none;
    }
  }
`;

function defaultRowId(row: unknown): string {
  const record = row as { id?: string; path?: string };
  return String(record.id ?? record.path ?? "");
}

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function escapeAttribute(value: string): string {
  return escapeHtml(value);
}

function toBoolean(value: boolean | string): boolean {
  return value === true || value === "" || value === "true";
}

function columnStyle<T>(column: DataTableColumn<T>): string {
  return column.width ? `width: ${escapeAttribute(column.width)}` : "";
}

function renderHead<T>(column: DataTableColumn<T>): string {
  return `<th style="${columnStyle(column)}" data-mobile-hidden="${column.mobileHidden ? "true" : "false"}">${escapeHtml(column.header)}</th>`;
}

function renderSelectAll<T>(rows: T[], selectedIds: Set<string>, getRowId: RowIdGetter<T>): string {
  const allSelected = rows.length > 0 && rows.every((row) => selectedIds.has(getRowId(row)));
  return `<input type="checkbox" aria-label="Select all rows" data-select="all"${allSelected ? " checked" : ""}>`;
}

function renderRowSelect(rowId: string, selectedIds: Set<string>): string {
  return `<input type="checkbox" aria-label="Select row" data-row-id="${escapeAttribute(rowId)}"${selectedIds.has(rowId) ? " checked" : ""}>`;
}

function renderCell<T>(row: T, column: DataTableColumn<T>): string {
  const value = column.render ? column.render(row) : (row as Record<string, unknown>)[column.key];
  const content = value == null ? "" : String(value);

  return `<td style="${columnStyle(column)}" data-mobile-hidden="${column.mobileHidden ? "true" : "false"}">${column.html ? content : escapeHtml(content)}</td>`;
}

function renderRow<T>(
  row: T,
  columns: DataTableColumn<T>[],
  selectedIds: Set<string>,
  selectable: boolean,
  getRowId: RowIdGetter<T>,
): string {
  const rowId = getRowId(row);
  const select = selectable ? `<td class="select">${renderRowSelect(rowId, selectedIds)}</td>` : "";
  const cells = columns.map((column) => renderCell(row, column)).join("");
  return `<tr>${select}${cells}</tr>`;
}

function renderTable<T>(
  rows: T[],
  columns: DataTableColumn<T>[],
  selectedIds: Set<string>,
  selectable: boolean | string,
  loading: boolean | string,
  loadingText: string,
  emptyText: string,
  getRowId: RowIdGetter<T>,
): string {
  if (toBoolean(loading) && rows.length === 0) {
    return `<div class="empty">${escapeHtml(loadingText)}</div>`;
  }

  if (rows.length === 0) {
    return `<div class="empty">${escapeHtml(emptyText)}</div>`;
  }

  const canSelect = toBoolean(selectable);
  const selectHead = canSelect ? `<th class="select">${renderSelectAll(rows, selectedIds, getRowId)}</th>` : "";
  const heads = columns.map((column) => renderHead(column)).join("");
  const body = rows.map((row) => renderRow(row, columns, selectedIds, canSelect, getRowId)).join("");

  return `<div class="table-wrap"><table><thead><tr>${selectHead}${heads}</tr></thead><tbody>${body}</tbody></table></div>`;
}

function dispatchSelection<T>(
  host: EventTarget,
  rows: T[],
  selectedIds: Set<string>,
  getRowId: RowIdGetter<T>,
  target: HTMLInputElement,
): void {
  const next = new Set(selectedIds);

  if (target.dataset.select === "all") {
    const allSelected = target.checked;
    const selected = allSelected ? new Set(rows.map((row) => getRowId(row))) : new Set<string>();
    host.dispatchEvent(new CustomEvent("selection-change", { bubbles: true, composed: true, detail: { selectedIds: selected } }));
    return;
  }

  const rowId = target.dataset.rowId;
  if (!rowId) return;

  if (target.checked) {
    next.add(rowId);
  } else {
    next.delete(rowId);
  }

  host.dispatchEvent(new CustomEvent("selection-change", { bubbles: true, composed: true, detail: { selectedIds: next } }));
}

function handleTableChange<T>(event: Event, rows: T[], selectedIds: Set<string>, getRowId: RowIdGetter<T>): void {
  const target = event.target;
  if (!(target instanceof HTMLInputElement) || target.type !== "checkbox") return;

  const root = (event.currentTarget as Element).getRootNode();
  const host = root instanceof ShadowRoot ? root.host : null;
  if (!host) return;

  dispatchSelection(host, rows, selectedIds, getRowId, target);
}

function handleTableClick(event: Event): void {
  const target = event.target;
  if (!(target instanceof Element)) return;

  const action = target.closest<HTMLElement>("[data-row-action]");
  if (!action) return;

  const root = (event.currentTarget as Element).getRootNode();
  const host = root instanceof ShadowRoot ? root.host : null;
  if (!host) return;

  host.dispatchEvent(
    new CustomEvent("row-action", {
      bubbles: true,
      composed: true,
      detail: {
        action: action.dataset.rowAction ?? "",
        rowId: action.dataset.rowId ?? "",
      },
    }),
  );
}

export function AppDataTable<T>({
  columns = [],
  emptyText = "No results.",
  getRowId = defaultRowId as RowIdGetter<T>,
  loading = false,
  loadingText = "Loading...",
  rows = [],
  selectable = true,
  selectedIds = new Set<string>(),
}: {
  columns?: DataTableColumn<T>[];
  emptyText?: string;
  getRowId?: RowIdGetter<T>;
  loading?: boolean | string;
  loadingText?: string;
  rows?: T[];
  selectable?: boolean | string;
  selectedIds?: Set<string>;
}): Component<"app-data-table"> {
  const content = renderTable(rows, columns, selectedIds, selectable, loading, loadingText, emptyText, getRowId);

  return html`
    <style>${styles}</style>
    <div
      class="table-root"
      @change="${(event: Event) => handleTableChange(event, rows, selectedIds, getRowId)}"
      @click="${(event: Event) => handleTableClick(event)}"
    >
      ${content}
    </div>
  `;
}
