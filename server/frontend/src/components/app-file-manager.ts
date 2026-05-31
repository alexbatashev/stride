import {LitElement, css, html} from "lit";
import {
	WorkspaceEntry,
	createWorkspaceDirectory,
	deleteWorkspaceEntry,
	downloadWorkspaceFile,
	listWorkspaceFiles,
	uploadFiles,
} from "../api/threads.js";
import "./app-data-table.js";
import type {DataTableColumn} from "./app-data-table.js";

export class AppFileManager extends LitElement {
	static properties = {
		entries: {state: true},
		error: {state: true},
		loading: {state: true},
		open: {type: Boolean, reflect: true},
		path: {state: true},
		selected: {state: true},
		threadId: {type: String, attribute: "thread-id"},
	};

	open = false;
	threadId = "";

	private path = "";
	private entries: WorkspaceEntry[] = [];
	private selected = new Set<string>();
	private loading = false;
	private error = "";
	private loadedKey = "";
	private readonly columns: DataTableColumn<WorkspaceEntry>[] = [
		{
			key: "name",
			header: "Name",
			render: (entry) => html`
				<button class="cell-action" type="button" @click=${() => this.openEntry(entry)}>
					<span class="cell-icon">${entry.kind === "directory" ? html`<icon-folder></icon-folder>` : html`<icon-file></icon-file>`}</span>
					<span>${entry.name}</span>
				</button>
			`,
		},
		{
			key: "size",
			header: "Size",
			width: "70px",
			mobileHidden: true,
			render: (entry) => (entry.kind === "directory" ? "" : this.formatSize(entry.size)),
		},
		{
			key: "updated_at",
			header: "Updated",
			width: "96px",
			mobileHidden: true,
			render: (entry) => this.formatDate(entry.updated_at),
		},
	];

	static styles = css`
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

		.icon-button > * {
			height: 16px;
			width: 16px;
		}

		.action-button > :first-child {
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

	protected updated(changed: Map<string, unknown>) {
		if (changed.has("threadId")) {
			this.path = "";
			this.entries = [];
			this.selected = new Set();
			this.loadedKey = "";
		}

		if (this.open) {
			void this.load();
		}
	}

	render() {
		return html`
			<section class="panel" aria-label="Workspace files">
				<header>
					<h2>Files</h2>
					<button class="icon-button" type="button" aria-label="Close files" @click=${this.close}>
						<icon-x></icon-x>
					</button>
				</header>
				<div class="toolbar">
					<button class="action-button" type="button" @click=${this.createFolder} ?disabled=${!this.threadId}>
						<icon-plus></icon-plus><span>Folder</span>
					</button>
					<button class="action-button" type="button" @click=${this.selectFiles} ?disabled=${!this.threadId}>
						<icon-upload></icon-upload><span>Upload</span>
					</button>
					<button class="action-button" type="button" @click=${this.removeSelected} ?disabled=${this.selected.size === 0}>
						<icon-trash-2></icon-trash-2><span>Remove</span>
					</button>
					<input type="file" multiple @change=${this.onFilesSelected} />
				</div>
				<div class="path">
					<button class="icon-button" type="button" aria-label="Up one level" @click=${this.goUp} ?disabled=${!this.path}>
						<icon-chevron-left></icon-chevron-left>
					</button>
					<span>/${this.path}</span>
				</div>
				<div class="error">${this.error}</div>
				${this.renderTable()}
			</section>
		`;
	}

	private renderTable() {
		if (!this.threadId) {
			return html`<app-data-table empty-text="Start a thread before managing files."></app-data-table>`;
		}

		return html`<app-data-table
			.columns=${this.columns}
			.getRowId=${(entry: WorkspaceEntry) => entry.path}
			.loading=${this.loading}
			.rows=${this.entries}
			.selectedIds=${this.selected}
			empty-text="No files here."
			loading-text="Loading files..."
			@selection-change=${this.onSelectionChange}
		></app-data-table>`;
	}

	private async load(force = false) {
		if (!this.threadId) {
			return;
		}

		const key = `${this.threadId}:${this.path}`;
		if (!force && this.loadedKey === key) {
			return;
		}

		this.loading = true;
		this.error = "";
		try {
			const listing = await listWorkspaceFiles(this.threadId, this.path);
			this.path = listing.path;
			this.entries = listing.entries;
			this.selected = new Set();
			this.loadedKey = key;
		} catch {
			this.error = "Failed to load files.";
		} finally {
			this.loading = false;
		}
	}

	private close() {
		this.open = false;
		this.dispatchEvent(new CustomEvent("files-close", {bubbles: true, composed: true}));
	}

	private async createFolder() {
		const name = window.prompt("Folder name:");
		const cleanName = name?.trim();
		if (!cleanName) return;

		try {
			await createWorkspaceDirectory(this.threadId, this.joinPath(cleanName));
			await this.load(true);
		} catch {
			this.error = "Failed to create folder.";
		}
	}

	private selectFiles() {
		this.shadowRoot!.querySelector<HTMLInputElement>('input[type="file"]')?.click();
	}

	private async onFilesSelected(event: Event) {
		const input = event.target as HTMLInputElement;
		const files = Array.from(input.files ?? []);
		input.value = "";
		if (files.length === 0) return;

		this.error = "";
		try {
			await uploadFiles(this.threadId, files, this.path);
			await this.load(true);
		} catch {
			this.error = "Upload failed.";
		}
	}

	private async removeSelected() {
		if (this.selected.size === 0) return;
		if (!window.confirm("Remove selected files?")) return;

		try {
			for (const path of this.selected) {
				await deleteWorkspaceEntry(this.threadId, path);
			}
			await this.load(true);
		} catch {
			this.error = "Failed to remove selected files.";
		}
	}

	private async openEntry(entry: WorkspaceEntry) {
		if (entry.kind === "directory") {
			this.path = entry.path;
			this.loadedKey = "";
			await this.load(true);
			return;
		}

		try {
			const blob = await downloadWorkspaceFile(this.threadId, entry.path);
			const url = URL.createObjectURL(blob);
			const link = document.createElement("a");
			link.href = url;
			link.download = entry.name;
			link.click();
			URL.revokeObjectURL(url);
		} catch {
			this.error = "Download failed.";
		}
	}

	private goUp() {
		this.path = this.path.split("/").slice(0, -1).join("/");
		this.loadedKey = "";
		void this.load(true);
	}

	private onSelectionChange(event: CustomEvent<{selectedIds: Set<string>}>) {
		this.selected = event.detail.selectedIds;
	}

	private joinPath(name: string) {
		return [this.path, name].filter(Boolean).join("/");
	}

	private formatSize(size: number | null) {
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

	private formatDate(ms: number) {
		if (!ms) return "";
		return new Intl.DateTimeFormat(undefined, {
			month: "short",
			day: "numeric",
			year: "numeric",
		}).format(new Date(ms));
	}
}

customElements.define("app-file-manager", AppFileManager);
