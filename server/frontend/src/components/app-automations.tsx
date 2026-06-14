import { Component, css, onMount } from "@frontiers-labs/argon";
import {
	type Automation,
	type AutomationRun,
	createAutomation,
	deleteAutomation,
	listAutomationRuns,
	listAutomations,
	setAutomationEnabled,
} from "../api/automations.js";

type RunView = {
	id: string;
	status: string;
	startedLabel: string;
	output: string;
};

type AutomationsHost = HTMLElement & {
	items: Automation[];
	loading: boolean;
	error: string;
	creating: boolean;
	formError: string;
	detailOpen: boolean;
	detailName: string;
	runs: RunView[];
	runsLoading: boolean;
};

async function load(host: AutomationsHost): Promise<void> {
	host.loading = true;
	host.error = "";
	try {
		host.items = await listAutomations();
	} catch {
		host.error = "Failed to load automations.";
	} finally {
		host.loading = false;
	}
}

async function openDetail(host: AutomationsHost, item: Automation): Promise<void> {
	host.detailName = item.name;
	host.detailOpen = true;
	host.runsLoading = true;
	host.runs = [];
	try {
		const runs = await listAutomationRuns(item.id);
		host.runs = runs.map((run: AutomationRun): RunView => ({
			id: run.id,
			status: run.status,
			startedLabel: new Date(run.started_at * 1000).toLocaleString(),
			output: run.output || "(no output)",
		}));
	} catch {
		host.runs = [];
	} finally {
		host.runsLoading = false;
	}
}

const styles = css`
	:host {
		display: block;
		overflow: auto;
	}

	.content {
		box-sizing: border-box;
		margin: 0 auto;
		max-width: 760px;
		padding: 32px 24px;
		width: 100%;
	}

	.head-row {
		align-items: center;
		display: flex;
		gap: 12px;
		justify-content: space-between;
	}

	h1 {
		color: var(--foreground);
		font-size: 26px;
		margin: 0;
	}

	.muted {
		color: var(--muted-foreground);
		font-size: 14px;
		margin: 8px 0 0;
	}

	.list {
		display: flex;
		flex-direction: column;
		gap: 8px;
		margin-top: 24px;
	}

	.row {
		align-items: center;
		border: 1px solid var(--border);
		border-radius: 8px;
		display: flex;
		gap: 12px;
		justify-content: space-between;
		padding: 12px 16px;
	}

	.info {
		background: none;
		border: 0;
		cursor: pointer;
		display: flex;
		flex-direction: column;
		gap: 4px;
		min-width: 0;
		padding: 0;
		text-align: left;
	}

	.name {
		color: var(--foreground);
		font-weight: 600;
	}

	.meta {
		color: var(--muted-foreground);
		font: 13px/1.2 ui-monospace, SFMono-Regular, Menlo, monospace;
	}

	.controls {
		align-items: center;
		display: flex;
		gap: 8px;
	}

	button {
		background: var(--secondary);
		border: 1px solid var(--border);
		border-radius: 8px;
		color: var(--foreground);
		cursor: pointer;
		font: inherit;
		font-size: 14px;
		padding: 6px 12px;
	}

	button.danger {
		color: var(--destructive);
	}

	.modal {
		align-items: center;
		background: rgba(0, 0, 0, 0.4);
		display: flex;
		inset: 0;
		justify-content: center;
		padding: 24px;
		position: fixed;
		z-index: 50;
	}

	.card {
		background: var(--background);
		border: 1px solid var(--border);
		border-radius: 12px;
		box-sizing: border-box;
		max-height: 85vh;
		max-width: 560px;
		overflow: auto;
		padding: 24px;
		width: 100%;
	}

	.card h2 {
		margin: 0 0 16px;
	}

	.card label {
		color: var(--foreground);
		display: flex;
		flex-direction: column;
		font-size: 14px;
		gap: 6px;
		margin-bottom: 14px;
	}

	.card label.inline {
		align-items: center;
		flex-direction: row;
	}

	.card input,
	.card select,
	.card textarea {
		background: var(--background);
		border: 1px solid var(--border);
		border-radius: 8px;
		color: var(--foreground);
		font: inherit;
		padding: 8px 10px;
	}

	.card label.inline input {
		width: auto;
	}

	.card textarea {
		font: 13px/1.4 ui-monospace, SFMono-Regular, Menlo, monospace;
		resize: vertical;
	}

	.actions {
		display: flex;
		gap: 8px;
		justify-content: flex-end;
	}

	.runs {
		display: flex;
		flex-direction: column;
		gap: 8px;
	}

	.run summary {
		cursor: pointer;
	}

	.run pre {
		background: var(--muted);
		border-radius: 8px;
		margin: 8px 0 0;
		max-height: 320px;
		overflow: auto;
		padding: 12px;
		white-space: pre-wrap;
	}

	.status {
		font-size: 12px;
	}

	.status[data-status="success"] {
		color: #16a34a;
	}

	.status[data-status="failed"] {
		color: var(--destructive);
	}

	.time {
		color: var(--muted-foreground);
		font-size: 12px;
		margin-left: 8px;
	}

	.error {
		color: var(--destructive);
		font-size: 13px;
		margin-top: 12px;
	}

	.error:empty {
		display: none;
	}
`;

export function AppAutomations({
	items = [],
	loading = false,
	error = "",
	creating = false,
	formError = "",
	detailOpen = false,
	detailName = "",
	runs = [],
	runsLoading = false,
}: {
	items?: Automation[];
	loading?: boolean;
	error?: string;
	creating?: boolean;
	formError?: string;
	detailOpen?: boolean;
	detailName?: string;
	runs?: RunView[];
	runsLoading?: boolean;
}): Component {
	onMount(() => {
		void load(this);
	});

	return (
		<>
			<style>{styles}</style>
			<div
				class="root"
				onClick={(event: Event) => {
					const node = event.target as HTMLElement;
					if (node.dataset.backdrop === "create") {
						this.creating = false;
						return;
					}
					if (node.dataset.backdrop === "detail") {
						this.detailOpen = false;
						return;
					}
					const target = node.closest<HTMLElement>("[data-action]");
					if (!target) return;
					switch (target.dataset.action) {
						case "open-create":
							this.creating = true;
							return;
						case "close-create":
							this.creating = false;
							return;
						case "close-detail":
							this.detailOpen = false;
							return;
					}
					const item = (this.items as Automation[]).find((it) => it.id === target.dataset.id);
					if (!item) return;
					switch (target.dataset.action) {
						case "detail":
							void openDetail(this, item);
							break;
						case "toggle":
							void setAutomationEnabled(item.id, !item.enabled)
								.then(() => load(this))
								.catch(() => {
									this.error = "Failed to update.";
								});
							break;
						case "delete":
							if (!window.confirm(`Delete automation "${item.name}"?`)) return;
							void deleteAutomation(item.id)
								.then(() => load(this))
								.catch(() => {
									this.error = "Failed to delete.";
								});
							break;
					}
				}}
				onSubmit={(event: Event) => {
					event.preventDefault();
					const data = new FormData(event.target as HTMLFormElement);
					this.formError = "";
					void createAutomation({
						name: String(data.get("name") ?? ""),
						schedule: String(data.get("schedule") ?? ""),
						kind: data.get("kind") === "python" ? "python" : "agent",
						payload: String(data.get("payload") ?? ""),
						enabled: data.get("enabled") !== null,
					})
						.then(() => {
							this.creating = false;
							void load(this);
						})
						.catch((err: unknown) => {
							this.formError =
								err instanceof Error && err.message === "400"
									? "Check the name, cron schedule and task."
									: "Failed to create automation.";
						});
				}}
			>
				<div class="content">
					<div class="head-row">
						<h1>Automations</h1>
						<button type="button" data-action="open-create">
							New automation
						</button>
					</div>
					<p class="muted">Scheduled tasks Friday runs for you on a cron schedule.</p>
					<div class="list">
						{items.length === 0 ? (
							<p class="muted">{loading ? "Loading…" : "No automations yet."}</p>
						) : (
							items
								.map(
										(item) => (
											<div class="row" key={item.id}>
												<button class="info" type="button" data-action="detail" data-id={item.id}>
													<span class="name">{item.name}</span>
													<span class="meta">
														{item.kind} · {item.schedule}
													</span>
												</button>
												<div class="controls">
													<button type="button" data-action="toggle" data-id={item.id}>
														{item.enabled ? "On" : "Off"}
													</button>
													<button class="danger" type="button" data-action="delete" data-id={item.id}>
														Delete
													</button>
												</div>
											</div>
										),
									)
									.join("")
						)}
					</div>
					<div class="error">{error}</div>
				</div>

				{creating && (
					<div class="modal" data-backdrop="create">
						<form class="card">
							<h2>New automation</h2>
							<label>
								Name
								<input name="name" required />
							</label>
							<label>
								Schedule (cron)
								<input name="schedule" placeholder="*/30 * * * *" required />
							</label>
							<label>
								Type
								<select name="kind">
									<option value="agent">Agent prompt</option>
									<option value="python">Python script</option>
								</select>
							</label>
							<label>
								Task
								<textarea name="payload" rows="6" required></textarea>
							</label>
							<label class="inline">
								<input type="checkbox" name="enabled" checked /> Enabled
							</label>
							<div class="actions">
								<button type="button" data-action="close-create">
									Cancel
								</button>
								<button type="submit">Create</button>
							</div>
							<div class="error">{formError}</div>
						</form>
					</div>
				)}

				{detailOpen && (
					<div class="modal" data-backdrop="detail">
						<div class="card">
							<div class="actions">
								<h2>{detailName}</h2>
								<button type="button" data-action="close-detail">
									Close
								</button>
							</div>
							<div class="runs">
								{runs.length === 0 ? (
									<p class="muted">{runsLoading ? "Loading…" : "No executions yet."}</p>
								) : (
									runs
										.map(
											(run) => (
												<details class="run" key={run.id}>
													<summary>
														<span class="status" data-status={run.status}>{run.status}</span>
														<span class="time">{run.startedLabel}</span>
													</summary>
													<pre>{run.output}</pre>
												</details>
											),
										)
										.join("")
								)}
							</div>
						</div>
					</div>
				)}
			</div>
		</>
	);
}
