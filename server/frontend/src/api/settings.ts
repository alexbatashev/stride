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

export type GitHubSettings = {
	configured: boolean;
	connected: boolean;
	login: string | null;
	auth_method: "oauth" | "pat" | null;
};

export type GoogleSettings = {
	configured: boolean;
	connected: boolean;
	email: string | null;
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

export type McpServer = {
	id: string;
	name: string;
	url: string;
	enabled: boolean;
	created_at: number;
	header_names: string[];
	has_authorization: boolean;
};

export type NewMcpServer = {
	name: string;
	url: string;
	bearer_token: string;
	headers_json: string;
	enabled: boolean;
};

export type WritableDir = {
	id: string;
	path: string;
	created_at: number;
};

export type Skill = {
	id: string;
	name: string;
	title: string;
	description: string;
	content: string;
};

export type NewSkill = {
	name: string;
	title: string;
	description: string;
	content: string;
};

export type SkillUpdate = {
	title: string;
	description: string;
	content: string;
};

export type MemoryWing = {
	id: string;
	name: string;
	description: string;
	rooms: number;
	memories: number;
	created_at: number;
};

export type MemoryRoom = {
	id: string;
	wing: string;
	name: string;
	description: string;
	memories: number;
	created_at: number;
};

export type Memory = {
	id: string;
	wing: string;
	room: string;
	title: string;
	summary: string;
	content: string;
	source: string | null;
	keywords: string;
	created_at: number;
};

export type MemorySettings = {
	wings: MemoryWing[];
	rooms: MemoryRoom[];
	memories: Memory[];
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

export async function getGitHubSettings(): Promise<GitHubSettings> {
	return request("/api/settings/github");
}

export async function startGitHubAuthorize(): Promise<string> {
	const response = await request<{ url: string }>("/api/settings/github/authorize");
	return response.url;
}

export async function connectGitHubPat(token: string): Promise<void> {
	await request("/api/settings/github/pat", {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify({ token }),
	});
}

export async function disconnectGitHub(): Promise<void> {
	await request("/api/settings/github/disconnect", { method: "POST" });
}

export async function getGoogleSettings(): Promise<GoogleSettings> {
	return request("/api/settings/google");
}

export async function startGoogleAuthorize(): Promise<string> {
	const response = await request<{ url: string }>("/api/settings/google/authorize");
	return response.url;
}

export async function disconnectGoogle(): Promise<void> {
	await request("/api/settings/google/disconnect", { method: "POST" });
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

export async function listMcpServers(): Promise<McpServer[]> {
	return request("/api/settings/mcp");
}

export async function createMcpServer(data: NewMcpServer): Promise<McpServer> {
	return request("/api/settings/mcp", {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify(data),
	});
}

export async function deleteMcpServer(id: string): Promise<void> {
	await request(`/api/settings/mcp/${id}`, { method: "DELETE" });
}

export async function listSkills(): Promise<Skill[]> {
	return request("/api/settings/skills");
}

export async function createSkill(data: NewSkill): Promise<Skill> {
	return request("/api/settings/skills", {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify(data),
	});
}

export async function updateSkill(id: string, data: SkillUpdate): Promise<Skill> {
	return request(`/api/settings/skills/${id}`, {
		method: "PATCH",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify(data),
	});
}

export async function deleteSkill(id: string): Promise<void> {
	await request(`/api/settings/skills/${id}`, { method: "DELETE" });
}

export async function listWritableDirs(): Promise<WritableDir[]> {
	return request("/api/settings/writable-dirs");
}

export async function createWritableDir(path: string): Promise<WritableDir> {
	return request("/api/settings/writable-dirs", {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify({ path }),
	});
}

export async function deleteWritableDir(id: string): Promise<void> {
	await request(`/api/settings/writable-dirs/${id}`, { method: "DELETE" });
}

export type ThreadRetentionSettings = {
	archive_after_days: number | null;
	remove_after_days: number | null;
};

export async function getThreadRetention(): Promise<ThreadRetentionSettings> {
	return request("/api/settings/thread-retention");
}

export async function updateThreadRetention(
	data: ThreadRetentionSettings,
): Promise<ThreadRetentionSettings> {
	return request("/api/settings/thread-retention", {
		method: "PUT",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify(data),
	});
}

export async function listMemories(): Promise<MemorySettings> {
	return request("/api/settings/memories");
}

export async function deleteMemory(id: string): Promise<void> {
	await request(`/api/settings/memories/${id}`, { method: "DELETE" });
}

export type ModelSummary = {
	key: string;
	slug: string;
	display_name: string;
	description: string;
	source: "config" | "user";
	provider: string;
	vision: boolean;
	reasoning_effort: string | null;
};

export type ProviderSummary = {
	id: string;
	name: string;
	kind: string;
	url: string;
	created_at: number;
};

export type NewProvider = {
	name: string;
	kind: string;
	url: string;
	token: string;
};

export type UserModelSummary = {
	id: string;
	name: string;
	slug: string;
	display_name: string;
	description: string;
	provider_id: string;
	provider_name: string;
	vision: boolean;
	reasoning_effort: string | null;
	created_at: number;
};

export type NewUserModel = {
	name: string;
	slug: string;
	provider_id: string;
	display_name?: string | null;
	description?: string | null;
	reasoning_effort?: string | null;
	vision?: boolean;
};

export type AgentSettings = {
	subagent_allowed_models: string[];
	subagent_guidelines: string;
	using_server_defaults: boolean;
	server_default_guidelines: string;
};

export async function listModels(): Promise<ModelSummary[]> {
	return request("/api/models");
}

export async function listProviders(): Promise<ProviderSummary[]> {
	return request("/api/settings/providers");
}

export async function createProvider(data: NewProvider): Promise<ProviderSummary> {
	return request("/api/settings/providers", {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify(data),
	});
}

export async function deleteProvider(id: string): Promise<void> {
	await request(`/api/settings/providers/${id}`, { method: "DELETE" });
}

export async function listUserModels(): Promise<UserModelSummary[]> {
	return request("/api/settings/user-models");
}

export async function createUserModel(data: NewUserModel): Promise<UserModelSummary> {
	return request("/api/settings/user-models", {
		method: "POST",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify(data),
	});
}

export async function deleteUserModel(id: string): Promise<void> {
	await request(`/api/settings/user-models/${id}`, { method: "DELETE" });
}

export async function getAgentSettings(): Promise<AgentSettings> {
	return request("/api/settings/agent");
}

export async function updateAgentSettings(data: AgentSettings): Promise<AgentSettings> {
	return request("/api/settings/agent", {
		method: "PUT",
		headers: { "Content-Type": "application/json" },
		body: JSON.stringify(data),
	});
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
