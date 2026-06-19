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

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
	const token = readToken();
	const headers = new Headers(init.headers);
	headers.set("Accept", "application/json");
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
