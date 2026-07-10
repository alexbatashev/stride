import { Component, effect, emit, onMount } from "@frontiers-labs/argon";
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
import { bindMenuDismiss, browserLoad, closePreview, fileActions, fileName, handleFileDialogClose, handleFileMenuSelect, handleFileRowAction, handleSelectionChange, managerLoad, openMenu, restoreVersionAndReload, type FileItem, type FileMenuTarget, type FileVersionItem, type ManagerHost, type VersionHost } from "./app-files-support.js";
import { browserStyles, fileManagementStyles, managerStyles } from "./app-files-styles.js";
import { AppDialog } from "./app-dialog.js";
import { AppDropdownMenu } from "./app-dropdown-menu.js";
import { IconChevronLeft } from "./icons/chevron-left.js";
import { IconEllipsisVertical } from "./icons/ellipsis-vertical.js";
import { IconPlus } from "./icons/plus.js";
import { IconTrash2 } from "./icons/trash-2.js";
import { IconUpload } from "./icons/upload.js";
import { IconX } from "./icons/x.js";

// ── File browser (the /files page) ────────────────────────────────────────────

export function AppFileBrowser({
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
  onMount(() => {
    void browserLoad(this);
    return () => closePreview(this as VersionHost);
  });

  effect(() => {
    const root = this.shadowRoot;
    if (!root || !menuOpen) return;
    return bindMenuDismiss(this as VersionHost, root);
  });

  effect(() => {
    const root = this.shadowRoot!;
    root.querySelector('[data-tool="rename"]')?.toggleAttribute("disabled", files.selected.length !== 1);
    root.querySelector('[data-tool="remove"]')?.toggleAttribute("disabled", files.selected.length === 0);
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
            const pathToRename = files.selected[0];
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
            if (files.selected.length === 0) return;
            if (!window.confirm("Remove selected files?")) return;
            void (async () => {
              try {
                for (const target of files.selected) {
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
        rows={JSON.stringify(entries)}
        selected={files.selected}
        loading={loading}
        loadingText="Loading files..."
        emptyText="No files here yet. Upload to get started."
        on:selection-change={(event: Event) => handleSelectionChange(event)}
        on:row-action={(event: Event) =>
          handleFileRowAction(this as VersionHost, event, (entry) => {
            this.path = entry.path;
            void browserLoad(this);
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

// ── Workspace file manager (threads side panel) ───────────────────────────────

export function AppFileManager({
  threadId = "",
  open = false,
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
  open?: boolean;
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
  onMount(() => () => closePreview(this as VersionHost));

  effect(() => {
    const root = this.shadowRoot;
    if (!root || !menuOpen) return;
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
      files.selected = [];
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
    root.querySelector('[data-tool="remove"]')?.toggleAttribute("disabled", files.selected.length === 0);
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
              emit(this, "files-close");
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
              if (files.selected.length === 0) return;
              if (!window.confirm("Remove selected files?")) return;
              void (async () => {
                try {
                  for (const target of files.selected) {
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
          rows={JSON.stringify(entries)}
          selected={files.selected}
          loading={loading}
          loadingText="Loading files..."
          emptyText={threadId !== "" ? "No files here." : "Start a thread before managing files."}
          on:selection-change={(event: Event) => handleSelectionChange(event)}
          on:row-action={(event: Event) =>
            handleFileRowAction(this as VersionHost, event, (entry) => {
              this.path = entry.path;
              this._loadedKey = "";
              void managerLoad(this);
            })
          }
        />
      </section>
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
