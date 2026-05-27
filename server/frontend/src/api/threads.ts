import {readToken} from './auth.js';

export type UploadedFile = {
	name: string;
	path: string;
	size: number;
};

export type ThreadSummary = {
	id: string;
	title: string;
	project_id: string | null;
};

export type ThreadMessage = {
	id: string;
	seq: number;
	role: 'system' | 'agent' | 'user' | 'tool';
	content: string;
	thinking: string | null;
	tool_call_name: string | null;
};

export type SendMessageResponse = {
	thread_id: string;
	run_id: string;
};

export type ThreadEvent = {
	seq: number;
	thread_id: string;
	run_id: string | null;
	kind:
		| {
				type: 'Snapshot';
				status: 'idle' | 'running';
				in_progress: {run_id: string; content: string; thinking: string | null} | null;
		  }
		| {type: 'RunStarted'}
		| {type: 'UserMessageCommitted'; message_id: string; seq: number}
		| {type: 'AgentDelta'; content: string}
		| {type: 'ThinkingDelta'; thinking: string}
		| {type: 'AgentMessageCommitted'; message_id: string; seq: number}
		| {type: 'ToolStarted'; name: string}
		| {type: 'ToolFinished'; name: string}
		| {type: 'WaitingForApproval'; approval_id: string; message: string}
		| {type: 'RunFinished'}
		| {type: 'RunFailed'; error: string}
		| {type: 'RunCancelled'};
};

export async function listThreads(): Promise<ThreadSummary[]> {
	return request('/api/threads');
}

export async function createThread(content: string, projectId?: string, filePaths?: string[]): Promise<SendMessageResponse> {
	return request('/api/threads', {
		method: 'POST',
		body: JSON.stringify({content, project_id: projectId ?? null, file_paths: filePaths ?? []})
	});
}

export async function listMessages(threadId: string): Promise<ThreadMessage[]> {
	return request(`/api/threads/${threadId}/messages`);
}

export async function sendMessage(threadId: string, content: string, filePaths?: string[]): Promise<SendMessageResponse> {
	return request(`/api/threads/${threadId}/messages`, {
		method: 'POST',
		body: JSON.stringify({content, file_paths: filePaths ?? []})
	});
}

export async function cancelRun(threadId: string): Promise<void> {
	await request(`/api/threads/${threadId}/cancel`, {method: 'POST'});
}

export async function uploadFiles(threadId: string, files: File[]): Promise<UploadedFile[]> {
	const token = readToken();
	const headers = new Headers();
	headers.set('Accept', 'application/json');
	if (token) headers.set('Authorization', `Bearer ${token}`);

	const body = new FormData();
	for (const file of files) {
		body.append('file', file, file.name);
	}

	const response = await fetch(`/api/threads/${threadId}/files`, {method: 'POST', headers, body});
	if (!response.ok) throw new Error(`${response.status}`);
	const data = await response.json() as {files: UploadedFile[]};
	return data.files;
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
	const token = readToken();
	const headers = new Headers(init.headers);
	headers.set('Accept', 'application/json');

	if (init.body) {
		headers.set('Content-Type', 'application/json');
	}

	if (token) {
		headers.set('Authorization', `Bearer ${token}`);
	}

	const response = await fetch(path, {...init, headers});

	if (!response.ok) {
		throw new Error(`${response.status}`);
	}

	return (await response.json()) as T;
}
