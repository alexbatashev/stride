import {
	type Automation,
	type AutomationRun,
	createAutomation,
	deleteAutomation,
	listAutomationRuns,
	listAutomations,
	setAutomationEnabled,
} from "../api/automations.js";
import { bindSidebar } from "./sidebar.js";

const root = document.querySelector<HTMLElement>("#automations-page");

class AutomationsPage {
	private readonly listEl: HTMLElement;
	private readonly detailEl: HTMLElement;
	private readonly createEl: HTMLElement;
	private readonly errorEl: HTMLElement;

	constructor(private readonly root: HTMLElement) {
		this.listEl = this.mustQuery("[data-list]");
		this.detailEl = this.mustQuery("[data-detail]");
		this.createEl = this.mustQuery("[data-create]");
		this.errorEl = this.mustQuery("[data-error]");
		this.bindEvents();
		void this.refresh();
	}

	private mustQuery<T extends Element>(selector: string): T {
		const element = this.root.querySelector<T>(selector);
		if (!element) throw new Error(`Missing ${selector}`);
		return element;
	}

	private bindEvents() {
		this.root.querySelector('[data-action="new"]')?.addEventListener("click", () => {
			this.openCreate();
		});
		const sidebar = this.root.querySelector<HTMLElement>("app-sidebar");
		if (sidebar) bindSidebar(sidebar);
	}

	private async refresh() {
		try {
			this.renderList(await listAutomations());
			this.setError("");
		} catch (error) {
			this.setError(error instanceof Error ? error.message : "Failed to load automations.");
		}
	}

	private renderList(items: Automation[]) {
		this.listEl.textContent = "";
		if (items.length === 0) {
			const empty = document.createElement("p");
			empty.className = "muted";
			empty.textContent = "No automations yet.";
			this.listEl.append(empty);
			return;
		}
		for (const item of items) {
			this.listEl.append(this.renderRow(item));
		}
	}

	private renderRow(item: Automation): HTMLElement {
		const row = document.createElement("div");
		row.className = "row";
		row.addEventListener("click", (event) => {
			if ((event.target as Element).closest("[data-control]")) return;
			void this.openDetail(item);
		});

		const info = document.createElement("div");
		info.className = "info";
		const name = document.createElement("span");
		name.className = "name";
		name.textContent = item.name;
		const meta = document.createElement("span");
		meta.className = "meta";
		meta.textContent = `${item.kind} · ${item.schedule}`;
		info.append(name, meta);

		const controls = document.createElement("div");
		controls.className = "controls";

		const toggle = document.createElement("input");
		toggle.type = "checkbox";
		toggle.checked = item.enabled;
		toggle.title = "Enabled";
		toggle.setAttribute("data-control", "");
		toggle.addEventListener("change", () => {
			void setAutomationEnabled(item.id, toggle.checked).catch((error) => {
				toggle.checked = !toggle.checked;
				this.setError(error instanceof Error ? error.message : "Failed to update.");
			});
		});

		const remove = document.createElement("button");
		remove.type = "button";
		remove.className = "danger";
		remove.textContent = "Delete";
		remove.setAttribute("data-control", "");
		remove.addEventListener("click", () => {
			if (!window.confirm(`Delete automation "${item.name}"?`)) return;
			void deleteAutomation(item.id).then(() => this.refresh());
		});

		controls.append(toggle, remove);
		row.append(info, controls);
		return row;
	}

	private openCreate() {
		this.createEl.textContent = "";
		const form = document.createElement("form");
		form.className = "card";
		form.innerHTML = `
			<h2>New automation</h2>
			<label>Name<input name="name" required /></label>
			<label>Schedule (cron)<input name="schedule" placeholder="*/30 * * * *" required /></label>
			<label>Type
				<select name="kind">
					<option value="agent">Agent prompt</option>
					<option value="python">Python script</option>
				</select>
			</label>
			<label>Task<textarea name="payload" rows="6" required></textarea></label>
			<label class="inline"><input type="checkbox" name="enabled" checked /> Enabled</label>
			<div class="actions">
				<button type="button" data-cancel>Cancel</button>
				<button type="submit">Create</button>
			</div>
			<div class="error" data-form-error></div>`;
		form.querySelector("[data-cancel]")?.addEventListener("click", () => this.closeModal(this.createEl));
		form.addEventListener("submit", (event) => {
			event.preventDefault();
			void this.submitCreate(form);
		});
		this.createEl.append(form);
		this.showModal(this.createEl);
	}

	private async submitCreate(form: HTMLFormElement) {
		const data = new FormData(form);
		const errorEl = form.querySelector<HTMLElement>("[data-form-error]");
		try {
			await createAutomation({
				name: String(data.get("name") ?? ""),
				schedule: String(data.get("schedule") ?? ""),
				kind: data.get("kind") === "python" ? "python" : "agent",
				payload: String(data.get("payload") ?? ""),
				enabled: data.get("enabled") !== null,
			});
			this.closeModal(this.createEl);
			await this.refresh();
		} catch (error) {
			const message =
				error instanceof Error && error.message === "400"
					? "Check the name, cron schedule and task."
					: "Failed to create automation.";
			if (errorEl) errorEl.textContent = message;
		}
	}

	private async openDetail(item: Automation) {
		this.detailEl.textContent = "";
		const card = document.createElement("div");
		card.className = "card";
		const header = document.createElement("div");
		header.className = "actions";
		const title = document.createElement("h2");
		title.textContent = item.name;
		const close = document.createElement("button");
		close.type = "button";
		close.textContent = "Close";
		close.addEventListener("click", () => this.closeModal(this.detailEl));
		header.append(title, close);
		const runsEl = document.createElement("div");
		runsEl.className = "runs";
		runsEl.textContent = "Loading…";
		card.append(header, runsEl);
		this.detailEl.append(card);
		this.showModal(this.detailEl);

		try {
			this.renderRuns(runsEl, await listAutomationRuns(item.id));
		} catch {
			runsEl.textContent = "Failed to load executions.";
		}
	}

	private renderRuns(container: HTMLElement, runs: AutomationRun[]) {
		container.textContent = "";
		if (runs.length === 0) {
			container.textContent = "No executions yet.";
			return;
		}
		for (const run of runs) {
			const entry = document.createElement("details");
			entry.className = "run";
			const summary = document.createElement("summary");
			const when = new Date(run.started_at * 1000).toLocaleString();
			summary.innerHTML = `<span class="status ${run.status}">${run.status}</span> ${when}`;
			const output = document.createElement("pre");
			output.textContent = run.output || "(no output)";
			entry.append(summary, output);
			container.append(entry);
		}
	}

	private showModal(modal: HTMLElement) {
		modal.hidden = false;
		modal.addEventListener("click", this.backdropClose, { once: false });
	}

	private backdropClose = (event: Event) => {
		if (event.target === event.currentTarget) {
			this.closeModal(event.currentTarget as HTMLElement);
		}
	};

	private closeModal(modal: HTMLElement) {
		modal.hidden = true;
		modal.textContent = "";
		modal.removeEventListener("click", this.backdropClose);
	}

	private setError(message: string) {
		this.errorEl.textContent = message;
	}
}

if (root) {
	new AutomationsPage(root);
}
