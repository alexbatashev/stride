import {LitElement, css, html, nothing, type TemplateResult} from "lit";

export type DataTableColumn<T> = {
	key: string;
	header: string;
	width?: string;
	mobileHidden?: boolean;
	render: (row: T) => string | number | TemplateResult | typeof nothing;
};

export class AppDataTable<T> extends LitElement {
	static properties = {
		columns: {attribute: false},
		emptyText: {type: String, attribute: "empty-text"},
		getRowId: {attribute: false},
		loading: {type: Boolean},
		loadingText: {type: String, attribute: "loading-text"},
		rows: {attribute: false},
		selectable: {type: Boolean},
		selectedIds: {attribute: false},
	};

	columns: DataTableColumn<T>[] = [];
	emptyText = "No results.";
	getRowId: (row: T) => string = (row) => String((row as {id?: string; path?: string}).id ?? (row as {path?: string}).path ?? "");
	loading = false;
	loadingText = "Loading...";
	rows: T[] = [];
	selectable = true;
	selectedIds = new Set<string>();

	static styles = css`
		:host {
			display: block;
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

		.cell-icon svg {
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

	render() {
		if (this.loading && this.rows.length === 0) {
			return html`<div class="empty">${this.loadingText}</div>`;
		}

		if (this.rows.length === 0) {
			return html`<div class="empty">${this.emptyText}</div>`;
		}

		return html`
			<div class="table-wrap">
				<table>
					<thead>
						<tr>
							${this.selectable ? html`<th class="select">${this.renderSelectAll()}</th>` : nothing}
							${this.columns.map((column) => this.renderHead(column))}
						</tr>
					</thead>
					<tbody>
						${this.rows.map((row) => this.renderRow(row))}
					</tbody>
				</table>
			</div>
		`;
	}

	private renderHead(column: DataTableColumn<T>) {
		return html`<th style=${this.columnStyle(column)} data-mobile-hidden=${column.mobileHidden ? "true" : "false"}>
			${column.header}
		</th>`;
	}

	private renderRow(row: T) {
		const rowId = this.getRowId(row);
		return html`
			<tr>
				${this.selectable ? html`<td class="select">${this.renderRowSelect(rowId)}</td>` : nothing}
				${this.columns.map(
					(column) => html`
						<td style=${this.columnStyle(column)} data-mobile-hidden=${column.mobileHidden ? "true" : "false"}>
							${column.render(row)}
						</td>
					`,
				)}
			</tr>
		`;
	}

	private renderSelectAll() {
		const allSelected = this.rows.length > 0 && this.rows.every((row) => this.selectedIds.has(this.getRowId(row)));
		return html`<input type="checkbox" aria-label="Select all rows" .checked=${allSelected} @change=${this.toggleAll} />`;
	}

	private renderRowSelect(rowId: string) {
		return html`<input
			type="checkbox"
			aria-label="Select row"
			.checked=${this.selectedIds.has(rowId)}
			@change=${(event: Event) => this.toggleRow(rowId, event)}
		/>`;
	}

	private toggleAll(event: Event) {
		const checked = (event.target as HTMLInputElement).checked;
		const selectedIds = checked ? new Set(this.rows.map((row) => this.getRowId(row))) : new Set<string>();
		this.emitSelection(selectedIds);
	}

	private toggleRow(rowId: string, event: Event) {
		const selectedIds = new Set(this.selectedIds);
		if ((event.target as HTMLInputElement).checked) {
			selectedIds.add(rowId);
		} else {
			selectedIds.delete(rowId);
		}
		this.emitSelection(selectedIds);
	}

	private emitSelection(selectedIds: Set<string>) {
		this.dispatchEvent(
			new CustomEvent("selection-change", {
				bubbles: true,
				composed: true,
				detail: {selectedIds},
			}),
		);
	}

	private columnStyle(column: DataTableColumn<T>) {
		return column.width ? `width: ${column.width}` : "";
	}
}

customElements.define("app-data-table", AppDataTable);
