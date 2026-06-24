import {readToken} from './auth.js';

export type UploadedFile = {
	name: string;
	path: string;
	size: number;
};

export type StagedUpload = {
	id: string;
	name: string;
	size: number;
};

export type WorkspaceEntry = {
	name: string;
	path: string;
	kind: 'directory' | 'file';
	size: number | null;
	updated_at: number;
	mime_type: string | null;
};

export type WorkspaceList = {
	path: string;
	entries: WorkspaceEntry[];
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
				pending_approval: {approval_id: string; message: string} | null;
				pending_quiz: {quiz_id: string; questions: QuizQuestion[]} | null;
		  }
		| {type: 'RunStarted'}
		| {type: 'UserMessageCommitted'; message_id: string; seq: number}
		| {type: 'AgentDelta'; content: string}
		| {type: 'ThinkingDelta'; thinking: string}
		| {type: 'AgentMessageCommitted'; message_id: string; seq: number}
		| {type: 'ToolStarted'; name: string}
		| {type: 'ToolFinished'; name: string}
		| {type: 'WaitingForApproval'; approval_id: string; message: string}
		| {type: 'ApprovalResolved'; approval_id: string; approved: boolean}
		| {type: 'WaitingForQuiz'; quiz_id: string; questions: QuizQuestion[]}
		| {type: 'QuizAnswered'; quiz_id: string}
		| {type: 'RunFinished'}
		| {type: 'RunFailed'; error: string}
		| {type: 'RunCancelled'};
};

export type QuizQuestion = {
	question: string;
	options: string[];
};

export async function listThreads(): Promise<ThreadSummary[]> {
	return request('/api/threads');
}

export async function createThread(content: string, projectId?: string, stagedUploads?: string[]): Promise<SendMessageResponse> {
	return request('/api/threads', {
		method: 'POST',
		body: JSON.stringify({content, project_id: projectId ?? null, staged_uploads: stagedUploads ?? []})
	});
}

export async function listMessages(threadId: string): Promise<ThreadMessage[]> {
	return request(`/api/threads/${threadId}/messages`);
}

export async function sendMessage(threadId: string, content: string, stagedUploads?: string[]): Promise<SendMessageResponse> {
	return request(`/api/threads/${threadId}/messages`, {
		method: 'POST',
		body: JSON.stringify({content, staged_uploads: stagedUploads ?? []})
	});
}

export async function cancelRun(threadId: string): Promise<void> {
	await request(`/api/threads/${threadId}/cancel`, {method: 'POST'});
}

export async function resolveApproval(threadId: string, approvalId: string, approved: boolean): Promise<void> {
	await request(`/api/threads/${threadId}/approvals/${approvalId}`, {
		method: 'POST',
		body: JSON.stringify({approved})
	});
}

export async function answerQuiz(threadId: string, quizId: string, answers: string[]): Promise<void> {
	await request(`/api/threads/${threadId}/quizzes/${quizId}`, {
		method: 'POST',
		body: JSON.stringify({answers})
	});
}

export async function listWorkspaceFiles(threadId: string, path = ''): Promise<WorkspaceList> {
	return request(`/api/threads/${threadId}/files?path=${encodeURIComponent(path)}`);
}

export async function createWorkspaceDirectory(threadId: string, path: string): Promise<void> {
	await request(`/api/threads/${threadId}/directories`, {
		method: 'POST',
		body: JSON.stringify({path})
	});
}

export async function deleteWorkspaceEntry(threadId: string, path: string): Promise<void> {
	await request(`/api/threads/${threadId}/files/${encodePath(path)}`, {method: 'DELETE'});
}

export async function downloadWorkspaceFile(threadId: string, path: string): Promise<Blob> {
	const token = readToken();
	const headers = new Headers();
	if (token) headers.set('Authorization', `Bearer ${token}`);

	const response = await fetch(`/api/threads/${threadId}/files/${encodePath(path)}`, {headers});
	if (!response.ok) throw new Error(`${response.status}`);
	return response.blob();
}

export async function stageUploads(files: File[]): Promise<StagedUpload[]> {
	const token = readToken();
	const headers = new Headers();
	headers.set('Accept', 'application/json');
	if (token) headers.set('Authorization', `Bearer ${token}`);

	const body = new FormData();
	for (const file of files) {
		body.append('file', file, file.name);
	}

	const response = await fetch('/api/uploads', {method: 'POST', headers, body});
	if (!response.ok) throw new Error(`${response.status}`);
	const data = await response.json() as {files: StagedUpload[]};
	return data.files;
}

export async function uploadFiles(threadId: string, files: File[], path = ''): Promise<UploadedFile[]> {
	const token = readToken();
	const headers = new Headers();
	headers.set('Accept', 'application/json');
	if (token) headers.set('Authorization', `Bearer ${token}`);

	const body = new FormData();
	for (const file of files) {
		body.append('file', file, file.name);
	}

	const response = await fetch(`/api/threads/${threadId}/files?path=${encodeURIComponent(path)}`, {method: 'POST', headers, body});
	if (!response.ok) throw new Error(`${response.status}`);
	const data = await response.json() as {files: UploadedFile[]};
	return data.files;
}

function encodePath(path: string): string {
	return path.split('/').filter(Boolean).map(encodeURIComponent).join('/');
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

	if (response.status === 204) {
		return undefined as T;
	}

	const text = await response.text();
	return (text ? JSON.parse(text) : undefined) as T;
}
