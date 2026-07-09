import { Component, effect, emit } from "@frontiers-labs/argon";
import { tableStyles } from "./app-files-styles.js";
import type { FileItem } from "./app-files-support.js";
import { IconEllipsisVertical } from "./icons/ellipsis-vertical.js";
import { IconFile } from "./icons/file.js";
import { IconFolder } from "./icons/folder.js";

function parseRows(rows: unknown): FileItem[] {
  return Array.isArray(rows) ? rows as FileItem[] : JSON.parse(String(rows || "[]")) as FileItem[];
}

export function AppDataTable({
  rows = "[]",
  selected = [],
  selectable = true,
  loading = false,
  loadingText = "Loading...",
  emptyText = "No results.",
}: {
  rows?: string;
  selected?: string[];
  selectable?: boolean;
  loading?: boolean;
  loadingText?: string;
  emptyText?: string;
}): Component {
  const rowItems = parseRows(rows);

  // Checkbox checked state is a DOM property, not an attribute, so it is
  // synced imperatively after every rows/selected update.
  effect(() => {
    const currentRows = parseRows(rows);
    const picked = new Set(selected);
    const root = this.shadowRoot!;
    for (const box of root.querySelectorAll<HTMLInputElement>("input[data-row-id]")) {
      box.checked = picked.has(box.dataset.rowId!);
    }
    const all = root.querySelector<HTMLInputElement>('input[data-select="all"]');
    if (all) all.checked = currentRows.length > 0 && currentRows.every((row) => picked.has(row.path));
  });

  return (
    <>
      <style>{tableStyles}</style>
      <div
        class="table-root"
        onChange={(event: Event) => {
          const box = event.target as HTMLInputElement;
          if (box.type !== "checkbox") return;
          let next: string[];
          if (box.dataset.select === "all") {
            next = box.checked ? rowItems.map((row) => row.path) : [];
          } else {
            next = selected.filter((id) => id !== box.dataset.rowId);
            if (box.checked) next.push(box.dataset.rowId!);
          }
          emit(this, "selection-change", { selectedIds: next });
        }}
        onClick={(event: Event) => {
          const action = (event.target as Element).closest<HTMLElement>("[data-row-action]");
          if (!action) return;
          emit(this, "row-action", {
            action: action.dataset.rowAction ?? "",
            rowId: action.dataset.rowId ?? "",
            left: action.getBoundingClientRect().right,
            top: action.getBoundingClientRect().bottom,
          });
        }}
      >
        {rowItems.length === 0 ? (
          <div class="empty">{loading ? loadingText : emptyText}</div>
        ) : (
          <div class="table-wrap">
            <table>
              <thead>
                <tr>
                  {selectable && (
                    <th class="select">
                      <input type="checkbox" aria-label="Select all rows" data-select="all" />
                    </th>
                  )}
                  <th>Name</th>
                  <th class="col-size">Size</th>
                  <th class="col-updated">Updated</th>
                  <th class="col-actions" aria-label="Actions"></th>
                </tr>
              </thead>
              <tbody>
                {rowItems.map((row) => (
                  <tr key={row.path}>
                    {selectable && (
                      <td class="select">
                        <input type="checkbox" aria-label="Select row" data-row-id={row.path} />
                      </td>
                    )}
                    <td>
                      <button class="cell-action" type="button" data-row-action="open" data-row-id={row.path}>
                        <span class="cell-icon">{row.kind === "directory" ? <IconFolder /> : <IconFile />}</span>
                        <span>{row.name}</span>
                      </button>
                    </td>
                    <td class="col-size">{row.sizeLabel}</td>
                    <td class="col-updated">{row.updatedLabel}</td>
                    <td class="col-actions">
                      {row.kind === "file" ? (
                        <button
                          class="row-menu"
                          type="button"
                          aria-label={`Actions for ${row.name}`}
                          data-row-action="menu"
                          data-row-id={row.path}
                        >
                          <IconEllipsisVertical />
                        </button>
                      ) : (
                        ""
                      )}
                    </td>
                  </tr>
                )).join("")}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </>
  );
}
