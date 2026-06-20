import { readToken } from "./auth.js";

export type AutomationKind = "python" | "agent";
export type TriggerKind = "cron" | "email" | "webhook" | "manual" | "vfs_change";
export type NotifyKind = "none" | "telegram";

export type Automation = {
	id: string;
	name: string;
	schedule: string;
	kind: AutomationKind;
	payload: string;
	enabled: boolean;
	created_at: number;
	last_run: number | null;
	trigger_kind: TriggerKind;
	trigger_config: Record<string, unknown> | null;
	notify_kind: NotifyKind;
	// Returned only when a webhook automation is created.
	webhook_secret?: string | null;
};

export type AutomationRun = {
	id: string;
	started_at: number;
	finished_at: number | null;
	status: "running" | "success" | "failed";
	output: string;
};

export type NewAutomation = {
	name: string;
	schedule: string;
	kind: AutomationKind;
	payload: string;
	enabled: boolean;
	trigger_kind: TriggerKind;
	notify_kind: NotifyKind;
	// For the vfs_change trigger: { path } (empty path = all global files).
	trigger_config?: Record<string, unknown>;
};

export async function listAutomations(): Promise<Automation[]> {
	return request("/api/automations");
}

export async function createAutomation(input: NewAutomation): Promise<Automation> {
	return request("/api/automations", { method: "POST", body: JSON.stringify(input) });
}

export async function runAutomation(id: string): Promise<void> {
	await request(`/api/automations/${id}/run`, { method: "POST" });
}

export async function setAutomationEnabled(id: string, enabled: boolean): Promise<void> {
	await request(`/api/automations/${id}`, { method: "PATCH", body: JSON.stringify({ enabled }) });
}

export async function deleteAutomation(id: string): Promise<void> {
	await request(`/api/automations/${id}`, { method: "DELETE" });
}

export async function listAutomationRuns(id: string): Promise<AutomationRun[]> {
	return request(`/api/automations/${id}/runs`);
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
	const token = readToken();
	const headers = new Headers(init.headers);
	headers.set("Accept", "application/json");
	if (init.body) {
		headers.set("Content-Type", "application/json");
	}
	if (token) {
		headers.set("Authorization", `Bearer ${token}`);
	}

	const response = await fetch(path, { ...init, headers });
	if (!response.ok) {
		throw new Error(`${response.status}`);
	}
	if (response.status === 204) {
		return undefined as T;
	}

	const text = await response.text();
	return (text ? JSON.parse(text) : undefined) as T;
}
