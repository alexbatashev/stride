import { Component, css, effect, onMount } from "@frontiers-labs/argon";
import {
  createDirectory,
  deleteEntry,
  downloadFileVersion,
  listFiles,
  listFileVersions,
  renameEntry,
  restoreFileVersion,
  uploadFiles,
} from "../api/files.js";
import {
  createWorkspaceDirectory,
  deleteWorkspaceEntry,
  downloadWorkspaceFileVersion,
  listWorkspaceFileVersions,
  listWorkspaceFiles,
  restoreWorkspaceFileVersion,
  uploadFiles as uploadWorkspaceFiles,
} from "../api/threads.js";
import { AppDialog } from "./app-dialog.js";
import { AppDropdownMenu } from "./app-dropdown-menu.js";
import { IconChevronLeft } from "./icons/chevron-left.js";
import { IconEllipsisVertical } from "./icons/ellipsis-vertical.js";
import { IconFile } from "./icons/file.js";
import { IconFolder } from "./icons/folder.js";
import { IconPlus } from "./icons/plus.js";
import { IconTrash2 } from "./icons/trash-2.js";
import { IconUpload } from "./icons/upload.js";
import { IconX } from "./icons/x.js";

interface FileItem {
  name: string;
  path: string;
  kind: string;
  sizeLabel: string;
  updatedLabel: string;
  mimeType: string;
}

interface FileVersionItem {
  version: number;
  sizeLabel: string;
  createdLabel: string;
  mimeType: string;
  latest: boolean;
}

type FileMenuTarget =
  | { kind: "file"; path: string }
  | { kind: "version"; path: string; version: number };

type FileActions = {
  download(path: string, version?: number): Promise<Blob>;
  listVersions(path: string): Promise<{
    versions: { version: number; size: number; created_at: number; mime_type: string | null }[];
  }>;
  restoreVersion(path: string, version: number): Promise<void>;
  reload(): Promise<void>;
};

type VersionHost = FilesHost & {
  activeFile: FileItem | null;
  versions: FileVersionItem[];
  versionsOpen: boolean;
  versionsLoading: boolean;
  previewOpen: boolean;
  previewUrl: string;
  previewTitle: string;
  menuOpen: boolean;
  menuTop: number;
  menuLeft: number;
  menuTarget: FileMenuTarget | null;
  actionItems: string[];
  _loadedThread?: string;
  _loadedKey?: string;
};

function fileActions(host: VersionHost): FileActions {
  if ("threadId" in host) {
    return {
      download: (targetPath, version) => downloadWorkspaceFileVersion(host.threadId, targetPath, version),
      listVersions: (targetPath) => listWorkspaceFileVersions(host.threadId, targetPath),
      restoreVersion: (targetPath, version) => restoreWorkspaceFileVersion(host.threadId, targetPath, version),
      reload: () => {
        host._loadedKey = "";
        return managerLoad(host as ManagerHost);
      },
    };
  }

  return {
    download: (targetPath, version) => downloadFileVersion(targetPath, version),
    listVersions: (targetPath) => listFileVersions(targetPath),
    restoreVersion: (targetPath, version) => restoreFileVersion(targetPath, version),
    reload: () => browserLoad(host),
  };
}

// Text bindings insert markup verbatim, so the displayed name is escaped here.
function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

function toFileItem(entry: {
  name: string;
  path: string;
  kind: string;
  size: number | null;
  updated_at: number;
  mime_type?: string | null;
}): FileItem {
  return {
    name: escapeHtml(entry.name),
    path: entry.path,
    kind: entry.kind,
    sizeLabel: entry.kind === "directory" ? "" : formatSize(entry.size),
    updatedLabel: formatDate(entry.updated_at),
    mimeType: entry.mime_type ?? "",
  };
}

function formatSize(size: number | null): string {
  if (size == null) return "";
  if (size < 1024) return `${size} B`;
  const units = ["KB", "MB", "GB"];
  let value = size / 1024;
  let unit = units[0];
  for (const next of units.slice(1)) {
    if (value < 1024) break;
    value /= 1024;
    unit = next;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${unit}`;
}

function formatDate(ms: number): string {
  if (!ms) return "";
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
  }).format(new Date(ms));
}

function formatDateTime(ms: number): string {
  if (!ms) return "";
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
    hour: "numeric",
    minute: "2-digit",
  }).format(new Date(ms));
}

function fileName(path: string): string {
  return path.split("/").filter(Boolean).pop() ?? "download";
}

function isPreviewable(mimeType: string, path: string): boolean {
  const lower = path.toLowerCase();
  return (
    mimeType === "application/pdf" ||
    mimeType?.startsWith("image/") === true ||
    lower.endsWith(".pdf") ||
    /\.(png|jpe?g|gif|webp|svg|bmp|avif)$/.test(lower)
  );
}

function fileByPath(host: VersionHost, path: string): FileItem | undefined {
  return (host.entries as FileItem[]).find((entry) => entry.path === path);
}

function versionByNumber(host: VersionHost, version: number): FileVersionItem | undefined {
  return (host.versions as FileVersionItem[]).find((item) => item.version === version);
}

function buildMenuItems(host: VersionHost, target: FileMenuTarget): string[] {
  const mimeType =
    target.kind === "file" ? fileByPath(host, target.path)?.mimeType ?? "" : versionByNumber(host, target.version)?.mimeType ?? "";
  const items = ["Download"];
  if (isPreviewable(mimeType, target.path)) {
    items.push("Preview");
  }
  return items;
}

function toVersionItem(
  version: { version: number; size: number; created_at: number; mime_type: string | null },
  latest: boolean,
): FileVersionItem {
  return {
    version: version.version,
    sizeLabel: formatSize(version.size),
    createdLabel: formatDateTime(version.created_at),
    mimeType: version.mime_type ?? "",
    latest,
  };
}

async function downloadBlob(actions: FileActions, path: string, version?: number): Promise<void> {
  const blob = await actions.download(path, version);
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName(path);
  link.click();
  URL.revokeObjectURL(url);
}

async function previewBlob(host: VersionHost, actions: FileActions, path: string, version?: number): Promise<void> {
  const blob = await actions.download(path, version);
  if (host.previewUrl) URL.revokeObjectURL(host.previewUrl);
  host.previewUrl = URL.createObjectURL(blob);
  host.previewTitle = version == null ? fileName(path) : `${fileName(path)} · version ${version}`;
  host.previewOpen = true;
}

async function openVersionDialog(host: VersionHost, actions: FileActions, file: FileItem): Promise<void> {
  host.activeFile = file;
  host.versionsOpen = true;
  host.versionsLoading = true;
  host.versions = [];
  host.error = "";
  try {
    const response = await actions.listVersions(file.path);
    host.versions = response.versions.map((version, index) => toVersionItem(version, index === 0));
  } catch {
    host.error = "Failed to load versions.";
  } finally {
    host.versionsLoading = false;
  }
}

async function restoreVersionAndReload(host: VersionHost, actions: FileActions, version: number): Promise<void> {
  const file = host.activeFile;
  if (!file) return;
  if (!window.confirm(`Restore version ${version} of ${fileName(file.path)}?`)) return;
  host.error = "";
  try {
    await actions.restoreVersion(file.path, version);
    await actions.reload();
    await openVersionDialog(host, actions, file);
  } catch {
    host.error = "Restore failed.";
  }
}

function closePreview(host: VersionHost): void {
  host.previewOpen = false;
  if (host.previewUrl) URL.revokeObjectURL(host.previewUrl);
  host.previewUrl = "";
  host.previewTitle = "";
}

function openMenu(host: VersionHost, event: MouseEvent, target: FileMenuTarget): void {
  const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
  host.menuTarget = target;
  host.actionItems = buildMenuItems(host, target);
  host.menuLeft = Math.max(8, rect.right - 176);
  host.menuTop = rect.bottom + 4;
  host.menuOpen = true;
}

function closeMenu(host: VersionHost): void {
  host.menuOpen = false;
  host.menuTarget = null;
  host.actionItems = [];
}

function dialogIdFromEvent(event: Event): string {
  for (const node of event.composedPath()) {
    if (!(node instanceof HTMLElement)) continue;
    if (node.dataset.dialog) return node.dataset.dialog;
    const attr = node.getAttribute("data-dialog");
    if (attr) return attr;
  }
  return "";
}

function bindMenuDismiss(host: VersionHost, root: ShadowRoot): () => void {
  let dismissClick: ((event: Event) => void) | null = null;
  let dismissKey: ((event: KeyboardEvent) => void) | null = null;

  const clearDismiss = () => {
    if (dismissClick) document.removeEventListener("click", dismissClick, true);
    if (dismissKey) document.removeEventListener("keydown", dismissKey, true);
    dismissClick = null;
    dismissKey = null;
  };

  clearDismiss();
  if (!host.menuOpen) return clearDismiss;

  const menu = root.querySelector("app-dropdown-menu");
  requestAnimationFrame(() => {
    dismissClick = (event: Event) => {
      if (!host.menuOpen) return;
      const path = event.composedPath();
      if (menu && path.includes(menu)) return;
      const openedFromRowMenu = path.some(
        (node) => node instanceof HTMLElement && node.dataset.rowAction === "menu",
      );
      if (openedFromRowMenu) return;
      closeMenu(host);
    };
    dismissKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") closeMenu(host);
    };
    document.addEventListener("click", dismissClick, true);
    document.addEventListener("keydown", dismissKey, true);
  });

  return clearDismiss;
}

async function handleMenuSelect(host: VersionHost, actions: FileActions, action: string): Promise<void> {
  const target = host.menuTarget;
  closeMenu(host);
  if (!target) return;
  const version = target.kind === "version" ? target.version : undefined;
  try {
    if (action === "download") {
      await downloadBlob(actions, target.path, version);
    } else if (action === "preview") {
      await previewBlob(host, actions, target.path, version);
    }
  } catch {
    host.error = action === "preview" ? "Preview failed." : "Download failed.";
  }
}

// ── Data table ────────────────────────────────────────────────────────────────

const tableStyles = css`
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

export function AppDataTable({
  rows = [],
  selected = [],
  selectable = true,
  loading = false,
  loadingText = "Loading...",
  emptyText = "No results.",
}: {
  rows?: FileItem[];
  selected?: string[];
  selectable?: boolean;
  loading?: boolean;
  loadingText?: string;
  emptyText?: string;
}): Component {
  // Checkbox checked state is a DOM property, not an attribute, so it is
  // synced imperatively after every rows/selected update.
  effect(() => {
    const picked = new Set(selected);
    const root = this.shadowRoot!;
    for (const box of root.querySelectorAll<HTMLInputElement>("input[data-row-id]")) {
      box.checked = picked.has(box.dataset.rowId!);
    }
    const all = root.querySelector<HTMLInputElement>('input[data-select="all"]');
    if (all) all.checked = rows.length > 0 && rows.every((row) => picked.has(row.path));
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
            next = box.checked ? rows.map((row) => row.path) : [];
          } else {
            next = selected.filter((id) => id !== box.dataset.rowId);
            if (box.checked) next.push(box.dataset.rowId!);
          }
          this.dispatchEvent(
            new CustomEvent("selection-change", {
              bubbles: true,
              composed: true,
              detail: { selectedIds: next },
            }),
          );
        }}
        onClick={(event: Event) => {
          const action = (event.target as Element).closest<HTMLElement>("[data-row-action]");
          if (!action) return;
          this.dispatchEvent(
            new CustomEvent("row-action", {
              bubbles: true,
              composed: true,
              detail: {
                action: action.dataset.rowAction ?? "",
                rowId: action.dataset.rowId ?? "",
                left: action.getBoundingClientRect().right,
                top: action.getBoundingClientRect().bottom,
              },
            }),
          );
        }}
      >
        {rows.length === 0 ? (
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
                {rows.map((row) => (
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

// ── File browser (the /files page) ────────────────────────────────────────────

type FilesHost = HTMLElement & {
  path: string;
  entries: FileItem[];
  selected: string[];
  loading: boolean;
  error: string;
};

async function browserLoad(host: FilesHost): Promise<void> {
  host.loading = true;
  host.error = "";
  try {
    const listing = await listFiles(host.path);
    host.path = listing.path;
    host.entries = listing.entries.map(toFileItem);
    host.selected = [];
  } catch {
    host.error = "Failed to load files.";
  } finally {
    host.loading = false;
  }
}

const browserStyles = css`
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
`;

const fileManagementStyles = css`
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

export function AppFileBrowser({
  path = "",
  entries = [],
  selected = [],
  loading = false,
  error = "",
  activeFile = null,
  versions = [],
  versionsOpen = false,
  versionsLoading = false,
  previewOpen = false,
  previewUrl = "",
  previewTitle = "",
  menuOpen = false,
  menuTop = 0,
  menuLeft = 0,
  menuTarget = null,
  actionItems = [],
}: {
  path?: string;
  entries?: FileItem[];
  selected?: string[];
  loading?: boolean;
  error?: string;
  activeFile?: FileItem | null;
  versions?: FileVersionItem[];
  versionsOpen?: boolean;
  versionsLoading?: boolean;
  previewOpen?: boolean;
  previewUrl?: string;
  previewTitle?: string;
  menuOpen?: boolean;
  menuTop?: number;
  menuLeft?: number;
  menuTarget?: FileMenuTarget | null;
  actionItems?: string[];
}): Component {
  onMount(() => {
    void browserLoad(this);
    const root = this.shadowRoot!;
    const onSelection = (event: Event) => {
      this.selected = (event as CustomEvent<{ selectedIds: string[] }>).detail.selectedIds;
    };
    const onRowAction = (event: Event) => {
      const detail = (event as CustomEvent<{ action: string; rowId: string; left: number; top: number }>).detail;
      const entry = (this.entries as FileItem[]).find((item) => item.path === detail.rowId);
      if (!entry) return;
      if (detail.action === "menu") {
        this.menuTarget = { kind: "file", path: entry.path };
        this.actionItems = buildMenuItems(this as VersionHost, this.menuTarget);
        this.menuLeft = Math.max(8, detail.left - 176);
        this.menuTop = detail.top + 4;
        this.menuOpen = true;
        return;
      }
      if (detail.action !== "open") return;
      if (entry.kind === "directory") {
        this.path = entry.path;
        void browserLoad(this);
        return;
      }
      void openVersionDialog(this as VersionHost, fileActions(this as VersionHost), entry);
    };
    const onMenuSelect = (event: Event) => {
      void handleMenuSelect(this as VersionHost, fileActions(this as VersionHost), (event as CustomEvent<{ action: string }>).detail.action);
    };
    const onDialogClose = (event: Event) => {
      const id = dialogIdFromEvent(event);
      if (id === "versions") {
        this.versionsOpen = false;
        closeMenu(this as VersionHost);
      } else if (id === "preview") {
        closePreview(this as VersionHost);
      }
    };
    root.addEventListener("selection-change", onSelection);
    root.addEventListener("row-action", onRowAction);
    root.addEventListener("select", onMenuSelect);
    root.addEventListener("close", onDialogClose);
    return () => {
      root.removeEventListener("selection-change", onSelection);
      root.removeEventListener("row-action", onRowAction);
      root.removeEventListener("select", onMenuSelect);
      root.removeEventListener("close", onDialogClose);
      closePreview(this as VersionHost);
    };
  });

  effect(() => {
    const root = this.shadowRoot;
    if (!root) return;
    return bindMenuDismiss(this as VersionHost, root);
  });

  effect(() => {
    const root = this.shadowRoot!;
    root.querySelector('[data-tool="rename"]')?.toggleAttribute("disabled", selected.length !== 1);
    root.querySelector('[data-tool="remove"]')?.toggleAttribute("disabled", selected.length === 0);
    root.querySelector('[data-tool="up"]')?.toggleAttribute("disabled", !path);
  });

  return (
    <>
      <style>{browserStyles}</style>
      <style>{fileManagementStyles}</style>
      <header>
        <h1>Files</h1>
      </header>
      <div class="toolbar">
        <button
          class="action-button"
          type="button"
          onClick={() => {
            const name = window.prompt("Folder name:")?.trim();
            if (!name) return;
            void createDirectory([this.path, name].filter(Boolean).join("/"))
              .then(() => browserLoad(this))
              .catch(() => {
                this.error = "Failed to create folder.";
              });
          }}
        >
          <IconPlus /><span>New folder</span>
        </button>
        <button
          class="action-button"
          type="button"
          onClick={() => root.querySelector<HTMLInputElement>('input[type="file"]')!.click()}
        >
          <IconUpload /><span>Upload</span>
        </button>
        <button
          class="action-button"
          type="button"
          data-tool="rename"
          onClick={() => {
            const pathToRename = (this.selected as string[])[0];
            const entry = (this.entries as FileItem[]).find((item) => item.path === pathToRename);
            if (!entry) return;
            const name = window.prompt("New name:", entry.name)?.trim();
            if (!name || name === entry.name) return;
            void renameEntry(pathToRename, name)
              .then(() => browserLoad(this))
              .catch(() => {
                this.error = "Failed to rename.";
              });
          }}
        >
          <span>Rename</span>
        </button>
        <button
          class="action-button"
          type="button"
          data-tool="remove"
          onClick={() => {
            if (this.selected.length === 0) return;
            if (!window.confirm("Remove selected files?")) return;
            void (async () => {
              try {
                for (const target of this.selected as string[]) {
                  await deleteEntry(target);
                }
                await browserLoad(this);
              } catch {
                this.error = "Failed to remove selected files.";
              }
            })();
          }}
        >
          <IconTrash2 /><span>Remove</span>
        </button>
        <input
          type="file"
          multiple
          onChange={(event: Event) => {
            const picker = event.target as HTMLInputElement;
            const files = Array.from(picker.files ?? []);
            picker.value = "";
            if (files.length === 0) return;
            this.error = "";
            void uploadFiles(files, this.path)
              .then(() => browserLoad(this))
              .catch(() => {
                this.error = "Upload failed.";
              });
          }}
        />
      </div>
      <div class="path">
        <button
          class="icon-button"
          type="button"
          aria-label="Up one level"
          data-tool="up"
          onClick={() => {
            this.path = (this.path as string).split("/").slice(0, -1).join("/");
            void browserLoad(this);
          }}
        >
          <IconChevronLeft />
        </button>
        <span>/{path}</span>
      </div>
      <div class="error">{error}</div>
      <AppDataTable
        rows={entries}
        selected={selected}
        loading={loading}
        loadingText="Loading files..."
        emptyText="No files here yet. Upload to get started."
      />
      <AppDropdownMenu
        open={menuOpen}
        items={actionItems}
        position={`left:${menuLeft}px;top:${menuTop}px`}
      />
      <AppDialog
        dialogId="versions"
        open={versionsOpen}
        title={activeFile ? fileName(activeFile.path) : "File versions"}
        description="Restore, download, or preview a saved version."
      >
        <div
          class="versions"
          onClick={(event: MouseEvent) => {
            const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-version-action]");
            if (!button) return;
            const row = button.closest(".version-row");
            const index = row ? Array.from(row.parentElement?.children ?? []).indexOf(row) : -1;
            const version = (this.versions as FileVersionItem[])[index]?.version;
            if (version == null) return;
            if (button.dataset.versionAction === "restore") {
              void restoreVersionAndReload(this as VersionHost, fileActions(this as VersionHost), version);
            } else if (button.dataset.versionAction === "menu" && activeFile) {
              openMenu(this as VersionHost, event, { kind: "version", path: activeFile.path, version });
            }
          }}
        >
          {versionsLoading ? (
            <div class="dialog-empty">Loading versions...</div>
          ) : versions.length === 0 ? (
            <div class="dialog-empty">No versions found.</div>
          ) : (
            versions.map((version) => (
              <div class="version-row">
                <div class="version-main">
                  <div class="version-title">
                    <span>Version {version.version}</span>
                    {version.latest ? <span class="badge">Current</span> : ""}
                  </div>
                  <div class="version-meta">
                    {version.createdLabel} · {version.sizeLabel}
                  </div>
                </div>
                <button
                  class="text-button"
                  type="button"
                  data-version-action="restore"
                >
                  Restore
                </button>
                <button
                  class="version-menu"
                  type="button"
                  aria-label="Version actions"
                  data-version-action="menu"
                >
                  <IconEllipsisVertical />
                </button>
              </div>
            )).join("")
          )}
        </div>
      </AppDialog>
      <AppDialog dialogId="preview" open={previewOpen} title={previewTitle} size="wide">
        <iframe class="preview-frame" src={previewUrl} title={previewTitle}></iframe>
      </AppDialog>
    </>
  );
}

// ── Workspace file manager (threads side panel) ───────────────────────────────

type ManagerHost = FilesHost & { threadId: string; open: boolean };

async function managerLoad(host: ManagerHost): Promise<void> {
  if (!host.threadId) return;
  host.loading = true;
  host.error = "";
  try {
    const listing = await listWorkspaceFiles(host.threadId, host.path);
    host.path = listing.path;
    host.entries = listing.entries.map(toFileItem);
    host.selected = [];
  } catch {
    host.error = "Failed to load files.";
  } finally {
    host.loading = false;
  }
}

const managerStyles = css`
  :host {
    background: var(--background);
    border-left: 1px solid var(--border);
    box-sizing: border-box;
    display: none;
    flex: 0 0 380px;
    height: 100%;
    max-width: 380px;
    min-width: 380px;
  }

  :host([open]) {
    display: block;
  }

  .panel {
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
  }

  header {
    align-items: center;
    border-bottom: 1px solid var(--border);
    box-sizing: border-box;
    display: flex;
    flex: 0 0 auto;
    gap: 8px;
    height: 56px;
    padding: 10px 12px;
  }

  h2 {
    color: var(--foreground);
    flex: 1;
    font-size: 15px;
    font-weight: 650;
    line-height: 1;
    margin: 0;
    min-width: 0;
  }

  .icon-button,
  .action-button {
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
    height: 32px;
    justify-content: center;
    outline: none;
    padding: 0 9px;
    white-space: nowrap;
  }

  .icon-button {
    padding: 0;
    width: 32px;
  }

  .icon-button:hover,
  .action-button:hover {
    background: var(--accent);
    color: var(--accent-foreground);
  }

  .icon-button:focus-visible,
  .action-button:focus-visible {
    box-shadow: 0 0 0 3px var(--ring-shadow);
  }

  .action-button:disabled,
  .icon-button:disabled {
    cursor: default;
    opacity: 0.5;
  }

  .icon-button > * {
    height: 16px;
    width: 16px;
  }

  .action-button > :not(span) {
    height: 16px;
    width: 16px;
  }

  .toolbar {
    align-items: center;
    border-bottom: 1px solid var(--border);
    display: flex;
    flex: 0 0 auto;
    gap: 6px;
    padding: 8px 12px;
  }

  .path {
    align-items: center;
    border-bottom: 1px solid var(--border);
    color: var(--muted-foreground);
    display: flex;
    flex: 0 0 auto;
    font-size: 12px;
    gap: 4px;
    min-height: 36px;
    padding: 0 12px;
  }

  .path span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .error {
    color: var(--destructive);
    font-size: 12px;
    padding: 8px 12px 0;
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
    --table-size-width: 70px;
    --table-updated-width: 96px;
  }

  @media (max-width: 767px) {
    :host {
      border-left: 0;
      display: none;
      flex: none;
      inset: 0;
      max-width: none;
      min-width: 0;
      position: fixed;
      width: 100%;
      z-index: 80;
    }

    :host([open]) {
      display: block;
    }

    .panel {
      background: var(--background);
    }

    header {
      height: 52px;
    }

    .toolbar {
      overflow-x: auto;
    }
  }
`;

export function AppFileManager({
  threadId = "",
  open = false,
  path = "",
  entries = [],
  selected = [],
  loading = false,
  error = "",
  activeFile = null,
  versions = [],
  versionsOpen = false,
  versionsLoading = false,
  previewOpen = false,
  previewUrl = "",
  previewTitle = "",
  menuOpen = false,
  menuTop = 0,
  menuLeft = 0,
  menuTarget = null,
  actionItems = [],
}: {
  threadId?: string;
  open?: boolean;
  path?: string;
  entries?: FileItem[];
  selected?: string[];
  loading?: boolean;
  error?: string;
  activeFile?: FileItem | null;
  versions?: FileVersionItem[];
  versionsOpen?: boolean;
  versionsLoading?: boolean;
  previewOpen?: boolean;
  previewUrl?: string;
  previewTitle?: string;
  menuOpen?: boolean;
  menuTop?: number;
  menuLeft?: number;
  menuTarget?: FileMenuTarget | null;
  actionItems?: string[];
}): Component {
  onMount(() => {
    const root = this.shadowRoot!;
    const onSelection = (event: Event) => {
      this.selected = (event as CustomEvent<{ selectedIds: string[] }>).detail.selectedIds;
    };
    const onRowAction = (event: Event) => {
      const detail = (event as CustomEvent<{ action: string; rowId: string; left: number; top: number }>).detail;
      const entry = (this.entries as FileItem[]).find((item) => item.path === detail.rowId);
      if (!entry) return;
      if (detail.action === "menu") {
        this.menuTarget = { kind: "file", path: entry.path };
        this.actionItems = buildMenuItems(this as VersionHost, this.menuTarget);
        this.menuLeft = Math.max(8, detail.left - 176);
        this.menuTop = detail.top + 4;
        this.menuOpen = true;
        return;
      }
      if (detail.action !== "open") return;
      if (entry.kind === "directory") {
        this.path = entry.path;
        this._loadedKey = "";
        void managerLoad(this);
        return;
      }
      void openVersionDialog(this as VersionHost, fileActions(this as VersionHost), entry);
    };
    const onMenuSelect = (event: Event) => {
      void handleMenuSelect(this as VersionHost, fileActions(this as VersionHost), (event as CustomEvent<{ action: string }>).detail.action);
    };
    const onDialogClose = (event: Event) => {
      const id = dialogIdFromEvent(event);
      if (id === "versions") {
        this.versionsOpen = false;
        closeMenu(this as VersionHost);
      } else if (id === "preview") {
        closePreview(this as VersionHost);
      }
    };
    root.addEventListener("selection-change", onSelection);
    root.addEventListener("row-action", onRowAction);
    root.addEventListener("select", onMenuSelect);
    root.addEventListener("close", onDialogClose);
    return () => {
      root.removeEventListener("selection-change", onSelection);
      root.removeEventListener("row-action", onRowAction);
      root.removeEventListener("select", onMenuSelect);
      root.removeEventListener("close", onDialogClose);
      closePreview(this as VersionHost);
    };
  });

  effect(() => {
    const root = this.shadowRoot;
    if (!root) return;
    return bindMenuDismiss(this as VersionHost, root);
  });

  // The "open" attribute drives :host([open]) visibility, and a thread
  // switch resets the listing; both flow through this effect.
  effect(() => {
    this.toggleAttribute("open", open);
    if (this._loadedThread !== threadId) {
      this._loadedThread = threadId;
      this._loadedKey = "";
      this.path = "";
      this.entries = [];
      this.selected = [];
    }
    if (!open || !threadId) return;
    const key = `${threadId}:${this.path}`;
    if (this._loadedKey === key) return;
    this._loadedKey = key;
    void managerLoad(this);
  });

  effect(() => {
    const root = this.shadowRoot!;
    root.querySelector('[data-tool="folder"]')?.toggleAttribute("disabled", !threadId);
    root.querySelector('[data-tool="upload"]')?.toggleAttribute("disabled", !threadId);
    root.querySelector('[data-tool="remove"]')?.toggleAttribute("disabled", selected.length === 0);
    root.querySelector('[data-tool="up"]')?.toggleAttribute("disabled", !path);
  });

  return (
    <>
      <style>{managerStyles}</style>
      <style>{fileManagementStyles}</style>
      <section class="panel" aria-label="Workspace files">
        <header>
          <h2>Files</h2>
          <button
            class="icon-button"
            type="button"
            aria-label="Close files"
            onClick={() => {
              this.open = false;
              this.dispatchEvent(new CustomEvent("files-close", { bubbles: true, composed: true }));
            }}
          >
            <IconX />
          </button>
        </header>
        <div class="toolbar">
          <button
            class="action-button"
            type="button"
            data-tool="folder"
            onClick={() => {
              const name = window.prompt("Folder name:")?.trim();
              if (!name) return;
              void createWorkspaceDirectory(this.threadId, [this.path, name].filter(Boolean).join("/"))
                .then(() => {
                  this._loadedKey = "";
                  return managerLoad(this);
                })
                .catch(() => {
                  this.error = "Failed to create folder.";
                });
            }}
          >
            <IconPlus /><span>Folder</span>
          </button>
          <button
            class="action-button"
            type="button"
            data-tool="upload"
            onClick={() => root.querySelector<HTMLInputElement>('input[type="file"]')!.click()}
          >
            <IconUpload /><span>Upload</span>
          </button>
          <button
            class="action-button"
            type="button"
            data-tool="remove"
            onClick={() => {
              if (this.selected.length === 0) return;
              if (!window.confirm("Remove selected files?")) return;
              void (async () => {
                try {
                  for (const target of this.selected as string[]) {
                    await deleteWorkspaceEntry(this.threadId, target);
                  }
                  this._loadedKey = "";
                  await managerLoad(this);
                } catch {
                  this.error = "Failed to remove selected files.";
                }
              })();
            }}
          >
            <IconTrash2 /><span>Remove</span>
          </button>
          <input
            type="file"
            multiple
            onChange={(event: Event) => {
              const picker = event.target as HTMLInputElement;
              const files = Array.from(picker.files ?? []);
              picker.value = "";
              if (files.length === 0) return;
              this.error = "";
              void uploadWorkspaceFiles(this.threadId, files, this.path)
                .then(() => {
                  this._loadedKey = "";
                  return managerLoad(this);
                })
                .catch(() => {
                  this.error = "Upload failed.";
                });
            }}
          />
        </div>
        <div class="path">
          <button
            class="icon-button"
            type="button"
            aria-label="Up one level"
            data-tool="up"
            onClick={() => {
              this.path = (this.path as string).split("/").slice(0, -1).join("/");
              this._loadedKey = "";
              void managerLoad(this);
            }}
          >
            <IconChevronLeft />
          </button>
          <span>/{path}</span>
        </div>
        <div class="error">{error}</div>
        <AppDataTable
          rows={entries}
          selected={selected}
          loading={loading}
          loadingText="Loading files..."
          emptyText={threadId !== "" ? "No files here." : "Start a thread before managing files."}
        />
      </section>
      <AppDropdownMenu
        open={menuOpen}
        items={actionItems}
        position={`left:${menuLeft}px;top:${menuTop}px`}
      />
      <AppDialog
        dialogId="versions"
        open={versionsOpen}
        title={activeFile ? fileName(activeFile.path) : "File versions"}
        description="Restore, download, or preview a saved version."
      >
        <div
          class="versions"
          onClick={(event: MouseEvent) => {
            const button = (event.target as HTMLElement).closest<HTMLButtonElement>("[data-version-action]");
            if (!button) return;
            const row = button.closest(".version-row");
            const index = row ? Array.from(row.parentElement?.children ?? []).indexOf(row) : -1;
            const version = (this.versions as FileVersionItem[])[index]?.version;
            if (version == null) return;
            if (button.dataset.versionAction === "restore") {
              void restoreVersionAndReload(this as VersionHost, fileActions(this as VersionHost), version);
            } else if (button.dataset.versionAction === "menu" && activeFile) {
              openMenu(this as VersionHost, event, { kind: "version", path: activeFile.path, version });
            }
          }}
        >
          {versionsLoading ? (
            <div class="dialog-empty">Loading versions...</div>
          ) : versions.length === 0 ? (
            <div class="dialog-empty">No versions found.</div>
          ) : (
            versions.map((version) => (
              <div class="version-row">
                <div class="version-main">
                  <div class="version-title">
                    <span>Version {version.version}</span>
                    {version.latest ? <span class="badge">Current</span> : ""}
                  </div>
                  <div class="version-meta">
                    {version.createdLabel} · {version.sizeLabel}
                  </div>
                </div>
                <button
                  class="text-button"
                  type="button"
                  data-version-action="restore"
                >
                  Restore
                </button>
                <button
                  class="version-menu"
                  type="button"
                  aria-label="Version actions"
                  data-version-action="menu"
                >
                  <IconEllipsisVertical />
                </button>
              </div>
            )).join("")
          )}
        </div>
      </AppDialog>
      <AppDialog dialogId="preview" open={previewOpen} title={previewTitle} size="wide">
        <iframe class="preview-frame" src={previewUrl} title={previewTitle}></iframe>
      </AppDialog>
    </>
  );
}
