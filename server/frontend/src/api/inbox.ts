import { readToken } from "./auth.js";

export type InboxKind = "follow_up" | "automation" | "skill";
export type InboxStatus = "pending" | "approved" | "declined" | "failed";

export type InboxItem = {
	id: string;
	kind: InboxKind;
	title: string;
	rationale: string;
	status: InboxStatus;
	created_at: number;
	resolved_at: number | null;
	thread_id: string | null;
	details: Record<string, unknown> | null;
	outcome: string | null;
	outcome_href: string | null;
};

export type InboxList = {
	items: InboxItem[];
	pending: number;
};

export async function listInbox(): Promise<InboxList> {
	return request("/api/inbox");
}

export async function approveInboxItem(id: string): Promise<InboxItem> {
	return request(`/api/inbox/${id}/approve`, { method: "POST" });
}

export async function declineInboxItem(id: string): Promise<InboxItem> {
	return request(`/api/inbox/${id}/decline`, { method: "POST" });
}

export async function deleteInboxItem(id: string): Promise<void> {
	await request(`/api/inbox/${id}`, { method: "DELETE" });
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
		const detail = await response
			.clone()
			.json()
			.then((body: { error?: string }) => body.error)
			.catch(() => undefined);
		throw new Error(detail || `${response.status}`);
	}
	if (response.status === 204) {
		return undefined as T;
	}

	const text = await response.text();
	return (text ? JSON.parse(text) : undefined) as T;
}
