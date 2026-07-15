import { Component, effect, onMount } from "@frontiers-labs/argon";
import {
  createDirectory,
  deleteEntry,
  renameEntry,
  uploadFiles,
} from "../api/files.js";
import {
  createWorkspaceDirectory,
  deleteWorkspaceEntry,
  uploadFiles as uploadWorkspaceFiles,
} from "../api/threads.js";
import { files } from "../stores/file-state.js";
import { AppDataTable } from "./app-data-table.js";
import {
  bindMenuDismiss,
  browserLoad,
  closePreview,
  fileActions,
  fileName,
  handleFileDialogClose,
  handleFileMenuSelect,
  handleFileRowAction,
  handleSelectionChange,
  managerLoad,
  openMenu,
  restoreVersionAndReload,
  type FileItem,
  type FileMenuTarget,
  type FileVersionItem,
  type VersionHost,
} from "./app-files-support.js";
import { explorerStyles, fileManagementStyles } from "./app-files-styles.js";
import { AppDialog } from "./app-dialog.js";
import { AppDropdownMenu } from "./app-dropdown-menu.js";
import { IconChevronLeft } from "./icons/chevron-left.js";
import { IconEllipsisVertical } from "./icons/ellipsis-vertical.js";
import { IconPlus } from "./icons/plus.js";
import { IconTrash2 } from "./icons/trash-2.js";
import { IconUpload } from "./icons/upload.js";

// Shared file-explorer body used by the /files page (global workspace, no
// `threadId`) and the thread side panel / mobile dialog (`threadId` set). The
// `fileActions()` adapter and the create/upload/remove calls branch on
// `threadId`; the render tree, menus, version and preview dialogs are shared.

export function AppFileExplorer({
  threadId = "",
  paneActive = true,
  path = "",
  entries = [],
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
  paneActive?: boolean;
  path?: string;
  entries?: FileItem[];
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
  const reload = () => {
    if (this.threadId) {
      this._loadedKey = "";
      return managerLoad(this as unknown as ManagerHost);
    }
    return browserLoad(this as VersionHost);
  };

  onMount(() => {
    if (!this.threadId) void browserLoad(this);
    return () => closePreview(this as VersionHost);
  });

  effect(() => {
    const root = this.shadowRoot;
    if (!root || !menuOpen) return;
    return bindMenuDismiss(this as VersionHost, root);
  });

  // Thread mode: reset on thread switch and (lazily) load when active.
  effect(() => {
    if (!threadId) return;
    this.toggleAttribute("data-compact", true);
    if (this._loadedThread !== threadId) {
      this._loadedThread = threadId;
      this._loadedKey = "";
      this.path = "";
      this.entries = [];
      files.selected = [];
    }
    if (!paneActive) return;
    const key = `${threadId}:${this.path}`;
    if (this._loadedKey === key) return;
    this._loadedKey = key;
    void managerLoad(this as unknown as ManagerHost);
  });

  effect(() => {
    const root = this.shadowRoot!;
    root.querySelector('[data-tool="rename"]')?.toggleAttribute("disabled", files.selected.length !== 1);
    root.querySelector('[data-tool="remove"]')?.toggleAttribute("disabled", files.selected.length === 0);
    root.querySelector('[data-tool="up"]')?.toggleAttribute("disabled", !path);
  });

  const createFolder = () => {
    const name = window.prompt("Folder name:")?.trim();
    if (!name) return;
    const target = [this.path, name].filter(Boolean).join("/");
    const request = this.threadId
      ? createWorkspaceDirectory(this.threadId, target)
      : createDirectory(target);
    void request.then(() => reload()).catch(() => {
      this.error = "Failed to create folder.";
    });
  };

  const removeSelected = () => {
    if (files.selected.length === 0) return;
    if (!window.confirm("Remove selected files?")) return;
    void (async () => {
      try {
        for (const target of files.selected) {
          await (this.threadId ? deleteWorkspaceEntry(this.threadId, target) : deleteEntry(target));
        }
        await reload();
      } catch {
        this.error = "Failed to remove selected files.";
      }
    })();
  };

  const uploadPicked = (event: Event) => {
    const picker = event.target as HTMLInputElement;
    const picked = Array.from(picker.files ?? []);
    picker.value = "";
    if (picked.length === 0) return;
    this.error = "";
    const request = this.threadId
      ? uploadWorkspaceFiles(this.threadId, picked, this.path)
      : uploadFiles(picked, this.path);
    void request.then(() => reload()).catch(() => {
      this.error = "Upload failed.";
    });
  };

  const renameSelected = () => {
    const pathToRename = files.selected[0];
    const entry = (this.entries as FileItem[]).find((item) => item.path === pathToRename);
    if (!entry) return;
    const name = window.prompt("New name:", entry.name)?.trim();
    if (!name || name === entry.name) return;
    void renameEntry(pathToRename, name).then(() => reload()).catch(() => {
      this.error = "Failed to rename.";
    });
  };

  const emptyText = threadId
    ? "No files here."
    : "No files here yet. Upload to get started.";

  return (
    <>
      <style>{explorerStyles}</style>
      <style>{fileManagementStyles}</style>
      <div class="toolbar">
        <button class="action-button" type="button" data-tool="folder" onClick={createFolder}>
          <IconPlus /><span>{threadId ? "Folder" : "New folder"}</span>
        </button>
        <button
          class="action-button"
          type="button"
          data-tool="upload"
          onClick={() => this.shadowRoot!.querySelector<HTMLInputElement>('input[type="file"]')!.click()}
        >
          <IconUpload /><span>Upload</span>
        </button>
        {threadId ? "" : (
          <button class="action-button" type="button" data-tool="rename" onClick={renameSelected}>
            <span>Rename</span>
          </button>
        )}
        <button class="action-button" type="button" data-tool="remove" onClick={removeSelected}>
          <IconTrash2 /><span>Remove</span>
        </button>
        <input type="file" multiple onChange={uploadPicked} />
      </div>
      <div class="path">
        <button
          class="icon-button"
          type="button"
          aria-label="Up one level"
          data-tool="up"
          onClick={() => {
            this.path = (this.path as string).split("/").slice(0, -1).join("/");
            void reload();
          }}
        >
          <IconChevronLeft />
        </button>
        <span>/{path}</span>
      </div>
      <div class="error">{error}</div>
      <AppDataTable
        rows={JSON.stringify(entries)}
        selected={files.selected}
        loading={loading}
        loadingText="Loading files..."
        emptyText={emptyText}
        on:selection-change={(event: Event) => handleSelectionChange(event)}
        on:row-action={(event: Event) =>
          handleFileRowAction(this as VersionHost, event, (entry) => {
            this.path = entry.path;
            void reload();
          })
        }
      />
      <AppDropdownMenu
        open={menuOpen}
        items={actionItems}
        position={`left:${menuLeft}px;top:${menuTop}px`}
        on:select={(event: Event) => handleFileMenuSelect(this as VersionHost, event)}
      />
      <AppDialog
        dialogId="versions"
        open={versionsOpen}
        title={activeFile ? fileName(activeFile.path) : "File versions"}
        description="Restore, download, or preview a saved version."
        on:close={(event: Event) => handleFileDialogClose(this as VersionHost, event)}
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
                <button class="text-button" type="button" data-version-action="restore">
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
      <AppDialog
        dialogId="preview"
        open={previewOpen}
        title={previewTitle}
        size="wide"
        on:close={(event: Event) => handleFileDialogClose(this as VersionHost, event)}
      >
        <iframe class="preview-frame" src={previewUrl} title={previewTitle}></iframe>
      </AppDialog>
    </>
  );
}
