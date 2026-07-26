import {
  downloadFileVersion,
  listFiles,
  listFileVersions,
  restoreFileVersion,
} from "../api/files.js";
import {
  downloadWorkspaceFileVersion,
  listWorkspaceFileVersions,
  listWorkspaceFiles,
  restoreWorkspaceFileVersion,
} from "../api/threads.js";
import { files } from "../stores/file-state.js";

export interface FileItem {
  name: string;
  path: string;
  kind: string;
  sizeLabel: string;
  updatedLabel: string;
  mimeType: string;
}

export interface FileVersionItem {
  version: number;
  sizeLabel: string;
  createdLabel: string;
  mimeType: string;
  latest: boolean;
}

export type FileMenuTarget =
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

export type VersionHost = FilesHost & {
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

export function fileActions(host: VersionHost): FileActions {
  const threadId = "threadId" in host ? (host as unknown as ManagerHost).threadId : "";
  if (threadId) {
    const manager = host as unknown as ManagerHost;
    return {
      download: (targetPath, version) => downloadWorkspaceFileVersion(manager.threadId, targetPath, version),
      listVersions: (targetPath) => listWorkspaceFileVersions(manager.threadId, targetPath),
      restoreVersion: (targetPath, version) => restoreWorkspaceFileVersion(manager.threadId, targetPath, version),
      reload: () => {
        host._loadedKey = "";
        return managerLoad(manager);
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

function toFileItem(entry: {
  name: string;
  path: string;
  kind: string;
  size: number | null;
  updated_at: number;
  mime_type?: string | null;
}): FileItem {
  return {
    name: entry.name,
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

export function fileName(path: string): string {
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

export async function restoreVersionAndReload(host: VersionHost, actions: FileActions, version: number): Promise<void> {
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

export function closePreview(host: VersionHost): void {
  host.previewOpen = false;
  if (host.previewUrl) URL.revokeObjectURL(host.previewUrl);
  host.previewUrl = "";
  host.previewTitle = "";
}

function sameMenuTarget(current: FileMenuTarget | null, next: FileMenuTarget): boolean {
  if (!current || current.kind !== next.kind || current.path !== next.path) return false;
  return current.kind === "file" || (next.kind === "version" && current.version === next.version);
}

function openFileRowMenu(
  host: VersionHost,
  entry: FileItem,
  left: number,
  top: number,
): void {
  const target: FileMenuTarget = { kind: "file", path: entry.path };
  if (host.menuOpen && sameMenuTarget(host.menuTarget, target)) {
    closeMenu(host);
    return;
  }
  host.menuTarget = target;
  host.actionItems = buildMenuItems(host, target);
  host.menuLeft = Math.max(8, left - 176);
  host.menuTop = top + 4;
  host.menuOpen = true;
}

export function openMenu(host: VersionHost, event: MouseEvent, target: FileMenuTarget): void {
  if (host.menuOpen && sameMenuTarget(host.menuTarget, target)) {
    closeMenu(host);
    return;
  }
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

export function handleSelectionChange(event: Event): void {
  files.selected = (event as CustomEvent<{ selectedIds: string[] }>).detail.selectedIds;
}

export function handleFileRowAction(
  host: VersionHost,
  event: Event,
  openDirectory: (entry: FileItem) => void,
): void {
  const detail = (event as CustomEvent<{ action: string; rowId: string; left: number; top: number }>).detail;
  const entry = (host.entries as FileItem[]).find((item) => item.path === detail.rowId);
  if (!entry) return;
  if (detail.action === "menu") {
    openFileRowMenu(host, entry, detail.left, detail.top);
    return;
  }
  if (detail.action !== "open") return;
  if (entry.kind === "directory") {
    openDirectory(entry);
    return;
  }
  void openVersionDialog(host, fileActions(host), entry);
}

export function handleFileMenuSelect(host: VersionHost, event: Event): void {
  void handleMenuSelect(host, fileActions(host), (event as CustomEvent<{ action: string }>).detail.action);
}

export function handleFileDialogClose(host: VersionHost, event: Event): void {
  const id = dialogIdFromEvent(event);
  if (id === "versions") {
    host.versionsOpen = false;
    closeMenu(host);
  } else if (id === "preview") {
    closePreview(host);
  }
}

export function bindMenuDismiss(host: VersionHost, root: ShadowRoot): () => void {
  let dismissClick: ((event: Event) => void) | null = null;
  let dismissKey: ((event: KeyboardEvent) => void) | null = null;

  const clearDismiss = () => {
    if (dismissClick) document.removeEventListener("click", dismissClick, true);
    if (dismissKey) document.removeEventListener("keydown", dismissKey, true);
    dismissClick = null;
    dismissKey = null;
  };

  const menu = root.querySelector("app-dropdown-menu");
  requestAnimationFrame(() => {
    requestAnimationFrame(() => {
      if (!host.menuOpen) return;
      dismissClick = (event: Event) => {
        if (!host.menuOpen) return;
        const path = event.composedPath();
        if (menu && path.includes(menu)) return;
        if (
          path.some(
            (node) =>
              node instanceof HTMLElement &&
              (node.dataset.rowAction === "menu" || node.dataset.versionAction === "menu"),
          )
        ) {
          return;
        }
        closeMenu(host);
      };
      dismissKey = (event: KeyboardEvent) => {
        if (event.key === "Escape") closeMenu(host);
      };
      document.addEventListener("click", dismissClick, true);
      document.addEventListener("keydown", dismissKey, true);
    });
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


export type FilesHost = HTMLElement & {
  path: string;
  entries: FileItem[];
  loading: boolean;
  error: string;
};

export async function browserLoad(host: FilesHost): Promise<void> {
  host.loading = true;
  host.error = "";
  try {
    const listing = await listFiles(host.path);
    host.path = listing.path;
    host.entries = listing.entries.map(toFileItem);
    files.selected = [];
  } catch {
    host.error = "Failed to load files.";
  } finally {
    host.loading = false;
  }
}


export type ManagerHost = FilesHost & { threadId: string; open: boolean };

export async function managerLoad(host: ManagerHost): Promise<void> {
  if (!host.threadId) return;
  host.loading = true;
  host.error = "";
  try {
    const listing = await listWorkspaceFiles(host.threadId, host.path);
    host.path = listing.path;
    host.entries = listing.entries.map(toFileItem);
    files.selected = [];
  } catch {
    host.error = "Failed to load files.";
  } finally {
    host.loading = false;
  }
}
