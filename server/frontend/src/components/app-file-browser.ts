import {LitElement, css, html} from "lit";
import {
	FileEntry,
	createDirectory,
	deleteEntry,
	downloadFile,
	listFiles,
	renameEntry,
	uploadFiles,
} from "../api/files.js";
import type {DataTableColumn} from "./app-data-table.js";

export class AppFileBrowser extends LitElement {
	static properties = {
		entries: {state: true},
		error: {state: true},
		loading: {state: true},
		path: {state: true},
		selected: {state: true},
	};

	private path = "";
	private entries: FileEntry[] = [];
	private selected = new Set<string>();
	private loading = false;
	private error = "";
	private loaded = false;
	private readonly columns: DataTableColumn<FileEntry>[] = [
		{
			key: "name",
			header: "Name",
			html: true,
			render: (entry) => this.renderNameCell(entry),
		},
		{
			key: "size",
			header: "Size",
			width: "90px",
			mobileHidden: true,
			render: (entry) => (entry.kind === "directory" ? "" : this.formatSize(entry.size)),
		},
		{
			key: "updated_at",
			header: "Updated",
			width: "120px",
			mobileHidden: true,
			render: (entry) => this.formatDate(entry.updated_at),
		},
	];

	static styles = css`
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

		.action-button > :first-child,
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

	connectedCallback() {
		super.connectedCallback();
		void this.load();
	}

	render() {
		return html`
			<header>
				<h1>Files</h1>
			</header>
			<div class="toolbar">
				<button class="action-button" type="button" @click=${this.createFolder}>
					<icon-plus></icon-plus><span>New folder</span>
				</button>
				<button class="action-button" type="button" @click=${this.selectFiles}>
					<icon-upload></icon-upload><span>Upload</span>
				</button>
				<button class="action-button" type="button" @click=${this.renameSelected} ?disabled=${this.selected.size !== 1}>
					<span>Rename</span>
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
			<app-data-table
				.columns=${this.columns}
				.getRowId=${(entry: FileEntry) => entry.path}
				.loading=${this.loading}
				.rows=${this.entries}
				.selectedIds=${this.selected}
				data-empty-text="No files here yet. Upload to get started."
				data-loading-text="Loading files..."
				@row-action=${this.onRowAction}
				@selection-change=${this.onSelectionChange}
			></app-data-table>
		`;
	}

	private async load() {
		this.loading = true;
		this.error = "";
		try {
			const listing = await listFiles(this.path);
			this.path = listing.path;
			this.entries = listing.entries;
			this.selected = new Set();
			this.loaded = true;
		} catch {
			this.error = "Failed to load files.";
		} finally {
			this.loading = false;
		}
	}

	private async createFolder() {
		const name = window.prompt("Folder name:")?.trim();
		if (!name) return;
		try {
			await createDirectory(this.joinPath(name));
			await this.load();
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
			await uploadFiles(files, this.path);
			await this.load();
		} catch {
			this.error = "Upload failed.";
		}
	}

	private async renameSelected() {
		if (this.selected.size !== 1) return;
		const path = [...this.selected][0];
		const entry = this.entries.find((item) => item.path === path);
		if (!entry) return;

		const name = window.prompt("New name:", entry.name)?.trim();
		if (!name || name === entry.name) return;
		try {
			await renameEntry(path, name);
			await this.load();
		} catch {
			this.error = "Failed to rename.";
		}
	}

	private async removeSelected() {
		if (this.selected.size === 0) return;
		if (!window.confirm("Remove selected files?")) return;
		try {
			for (const path of this.selected) {
				await deleteEntry(path);
			}
			await this.load();
		} catch {
			this.error = "Failed to remove selected files.";
		}
	}

	private async openEntry(entry: FileEntry) {
		if (entry.kind === "directory") {
			this.path = entry.path;
			await this.load();
			return;
		}

		try {
			const blob = await downloadFile(entry.path);
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
		void this.load();
	}

	private onSelectionChange(event: CustomEvent<{selectedIds: Set<string>}>) {
		this.selected = event.detail.selectedIds;
	}

	private onRowAction(event: CustomEvent<{action: string; rowId: string}>) {
		if (event.detail.action !== "open") return;
		const entry = this.entries.find((item) => item.path === event.detail.rowId);
		if (entry) void this.openEntry(entry);
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

	private renderNameCell(entry: FileEntry) {
		const icon = entry.kind === "directory" ? "icon-folder" : "icon-file";
		const path = this.escapeHtml(entry.path);
		const name = this.escapeHtml(entry.name);
		return `<button class="cell-action" type="button" data-row-action="open" data-row-id="${path}"><span class="cell-icon"><${icon}></${icon}></span><span>${name}</span></button>`;
	}

	private escapeHtml(value: string) {
		return value
			.replace(/&/g, "&amp;")
			.replace(/</g, "&lt;")
			.replace(/>/g, "&gt;")
			.replace(/"/g, "&quot;")
			.replace(/'/g, "&#39;");
	}
}

customElements.define("app-file-browser", AppFileBrowser);
