import {LitElement, css, html, nothing} from "lit";
import {
	WorkspaceEntry,
	createWorkspaceDirectory,
	deleteWorkspaceEntry,
	downloadWorkspaceFile,
	listWorkspaceFiles,
	uploadFiles,
} from "../api/threads.js";
import {
	CHEVRON_LEFT,
	FILE,
	FOLDER,
	PLUS,
	TRASH_2,
	UPLOAD,
	X,
} from "./icons.js";

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

		.icon-button svg,
		.action-button svg,
		.entry-icon svg {
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

		.table-wrap {
			flex: 1;
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

		th:first-child,
		td:first-child {
			padding-left: 12px;
			width: 34px;
		}

		th:nth-child(3),
		td:nth-child(3) {
			width: 70px;
		}

		th:nth-child(4),
		td:nth-child(4) {
			width: 96px;
		}

		tr:hover td {
			background: var(--accent);
		}

		.name-button {
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

		.name-button span:last-child {
			overflow: hidden;
			text-overflow: ellipsis;
			white-space: nowrap;
		}

		.entry-icon {
			align-items: center;
			color: var(--muted-foreground);
			display: inline-flex;
			flex: 0 0 16px;
			height: 16px;
			justify-content: center;
			width: 16px;
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

		input[type="checkbox"] {
			accent-color: var(--primary);
			height: 14px;
			margin: 0;
			width: 14px;
		}

		input[type="file"] {
			display: none;
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

			th:nth-child(3),
			td:nth-child(3),
			th:nth-child(4),
			td:nth-child(4) {
				display: none;
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
						${X}
					</button>
				</header>
				<div class="toolbar">
					<button class="action-button" type="button" @click=${this.createFolder} ?disabled=${!this.threadId}>
						${PLUS}<span>Folder</span>
					</button>
					<button class="action-button" type="button" @click=${this.selectFiles} ?disabled=${!this.threadId}>
						${UPLOAD}<span>Upload</span>
					</button>
					<button class="action-button" type="button" @click=${this.removeSelected} ?disabled=${this.selected.size === 0}>
						${TRASH_2}<span>Remove</span>
					</button>
					<input type="file" multiple @change=${this.onFilesSelected} />
				</div>
				<div class="path">
					<button class="icon-button" type="button" aria-label="Up one level" @click=${this.goUp} ?disabled=${!this.path}>
						${CHEVRON_LEFT}
					</button>
					<span>/${this.path}</span>
				</div>
				<div class="error">${this.error}</div>
				<div class="table-wrap">
					${this.renderBody()}
				</div>
			</section>
		`;
	}

	private renderBody() {
		if (!this.threadId) {
			return html`<div class="empty">Start a thread before managing files.</div>`;
		}

		if (this.loading && this.entries.length === 0) {
			return html`<div class="empty">Loading files...</div>`;
		}

		if (this.entries.length === 0) {
			return html`<div class="empty">No files here.</div>`;
		}

		const allSelected = this.entries.length > 0 && this.entries.every((entry) => this.selected.has(entry.path));

		return html`
			<table>
				<thead>
					<tr>
						<th>
							<input type="checkbox" aria-label="Select all files" .checked=${allSelected} @change=${this.toggleAll} />
						</th>
						<th>Name</th>
						<th>Size</th>
						<th>Updated</th>
					</tr>
				</thead>
				<tbody>
					${this.entries.map((entry) => this.renderRow(entry))}
				</tbody>
			</table>
		`;
	}

	private renderRow(entry: WorkspaceEntry) {
		return html`
			<tr>
				<td>
					<input
						type="checkbox"
						aria-label=${`Select ${entry.name}`}
						.checked=${this.selected.has(entry.path)}
						@change=${(event: Event) => this.toggleEntry(entry, event)}
					/>
				</td>
				<td>
					<button class="name-button" type="button" @click=${() => this.openEntry(entry)}>
						<span class="entry-icon">${entry.kind === "directory" ? FOLDER : FILE}</span>
						<span>${entry.name}</span>
					</button>
				</td>
				<td>${entry.kind === "directory" ? nothing : this.formatSize(entry.size)}</td>
				<td>${this.formatDate(entry.updated_at)}</td>
			</tr>
		`;
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

	private toggleAll(event: Event) {
		const checked = (event.target as HTMLInputElement).checked;
		this.selected = checked ? new Set(this.entries.map((entry) => entry.path)) : new Set();
		this.requestUpdate();
	}

	private toggleEntry(entry: WorkspaceEntry, event: Event) {
		const selected = new Set(this.selected);
		if ((event.target as HTMLInputElement).checked) {
			selected.add(entry.path);
		} else {
			selected.delete(entry.path);
		}
		this.selected = selected;
		this.requestUpdate();
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
