import { readToken } from "./auth.js";

export type TelegramSettings = {
	bot_configured: boolean;
	bot_username: string | null;
	connected: boolean;
	username: string | null;
	first_name: string | null;
	last_name: string | null;
};

export type TelegramAuthData = {
	id: number;
	first_name?: string;
	last_name?: string;
	username?: string;
	photo_url?: string;
	auth_date: number;
	hash: string;
};

export type EmailAccount = {
	id: string;
	name: string;
	email: string;
	host: string;
	port: number;
	username: string;
	inbox_mailbox: string;
	sent_mailbox: string;
	drafts_mailbox: string;
	created_at: number;
};

export type NewEmailAccount = {
	name: string;
	email: string;
	host: string;
	port: number;
	username: string;
	password: string;
	inbox_mailbox: string;
	sent_mailbox: string;
	drafts_mailbox: string;
};

export async function getTelegramSettings(): Promise<TelegramSettings> {
	return request("/api/settings/telegram");
}

export async function loginTelegram(data: TelegramAuthData): Promise<void> {
	await request("/api/settings/telegram/login", {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify(data),
	});
}

export async function disconnectTelegram(): Promise<void> {
	await request("/api/settings/telegram/disconnect", { method: "POST" });
}

export async function listEmailAccounts(): Promise<EmailAccount[]> {
	return request("/api/settings/email");
}

export async function createEmailAccount(data: NewEmailAccount): Promise<EmailAccount> {
	return request("/api/settings/email", {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify(data),
	});
}

export async function deleteEmailAccount(id: string): Promise<void> {
	await request(`/api/settings/email/${id}`, { method: "DELETE" });
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
	const token = readToken();
	const headers = new Headers(init.headers);
	headers.set("Accept", "application/json");
	if (token) {
		headers.set("Authorization", `Bearer ${token}`);
	}

	const response = await fetch(path, { ...init, headers });
	if (!response.ok) {
		const body = await response.json().catch(() => null) as { error?: string } | null;
		throw new Error(body?.error || `${response.status}`);
	}
	if (response.status === 204) {
		return undefined as T;
	}

	const text = await response.text();
	return (text ? JSON.parse(text) : undefined) as T;
}
