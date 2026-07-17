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

export type FileVersion = {
	version: number;
	size: number;
	created_at: number;
	mime_type: string | null;
};

export type FileVersions = {
	path: string;
	versions: FileVersion[];
};

export type ThreadSummary = {
	id: string;
	title: string;
	project_id: string | null;
};

export type ArchivedThread = {
	id: string;
	title: string;
	project_id: string | null;
	archived_at: number;
	last_activity_at: number;
};

export type ThreadMessage = {
	id: string;
	seq: number;
	role: 'system' | 'agent' | 'user' | 'tool';
	format: 'markdown' | 'html';
	content: string;
	thinking: string | null;
	tool_call_name: string | null;
};

export type SendMessageResponse = {
	thread_id: string;
	run_id: string;
};

export type ThreadEvent = {
	id?: string;
	seq: number;
	thread_id: string;
	run_id: string | null;
	agent_path: string[];
	kind:
		| {
				type: 'snapshot';
				status: 'idle' | 'running';
				in_progress: {message_id: string; run_id: string; content: string; format: 'markdown' | 'html'; thinking: string | null} | null;
				pending_approvals: {approval_id: string; message: string}[];
				pending_quizzes: {quiz_id: string; questions: QuizQuestion[]}[];
		  }
		| {type: 'run_started'}
		| {type: 'message_started'; message_id: string; role: 'user' | 'assistant' | 'system' | 'tool'}
		| {type: 'text_delta'; message_id: string; delta: string}
		| {type: 'thinking_delta'; message_id: string; delta: string}
		| {type: 'message_committed'; message_id: string}
		| {type: 'tool_call_started'; tool_call_id: string; name: string; arguments: string}
		| {type: 'tool_call_progress'; tool_call_id: string; payload: unknown}
		| {type: 'tool_call_finished'; tool_call_id: string; name: string; result: string; is_error: boolean}
		| {type: 'agent_spawned'; agent_id: string; parent_tool_call_id: string; name: string; model: string}
		| {type: 'agent_finished'; agent_id: string; result: string}
		| {type: 'approval_requested'; approval_id: string; tool_call_id: string; message: string}
		| {type: 'approval_resolved'; approval_id: string; approved: boolean}
		| {type: 'quiz_requested'; quiz_id: string; questions: QuizQuestion[]}
		| {type: 'quiz_answered'; quiz_id: string}
		| {type: 'run_finished'}
		| {type: 'run_failed'; error: string}
		| {type: 'run_cancelled'};
};

export type QuizQuestion = {
	question: string;
	options: string[];
};

export async function listThreads(): Promise<ThreadSummary[]> {
	return request('/api/threads');
}

export async function createThread(
	content: string,
	projectId?: string,
	stagedUploads?: string[],
	model?: string,
): Promise<SendMessageResponse> {
	return request('/api/threads', {
		method: 'POST',
		body: JSON.stringify({
			content,
			project_id: projectId ?? null,
			staged_uploads: stagedUploads ?? [],
			model: model ?? null,
		})
	});
}

export async function listArchivedThreads(): Promise<ArchivedThread[]> {
	return request('/api/threads/archived');
}

export async function renameThread(threadId: string, title: string): Promise<void> {
	await request(`/api/threads/${threadId}`, {method: 'PATCH', body: JSON.stringify({title})});
}

export async function archiveThread(threadId: string): Promise<void> {
	await request(`/api/threads/${threadId}/archive`, {method: 'POST'});
}

export async function unarchiveThread(threadId: string): Promise<void> {
	await request(`/api/threads/${threadId}/unarchive`, {method: 'POST'});
}

export async function deleteThread(threadId: string): Promise<void> {
	await request(`/api/threads/${threadId}`, {method: 'DELETE'});
}

export async function updateThreadModel(threadId: string, model: string | null): Promise<void> {
	await request(`/api/threads/${threadId}/model`, {
		method: 'PATCH',
		body: JSON.stringify({model})
	});
}

export async function listMessages(threadId: string): Promise<ThreadMessage[]> {
	return request(`/api/threads/${threadId}/messages`);
}

export async function sendMessage(
	threadId: string,
	content: string,
	stagedUploads?: string[],
	model?: string,
): Promise<SendMessageResponse> {
	return request(`/api/threads/${threadId}/messages`, {
		method: 'POST',
		body: JSON.stringify({content, staged_uploads: stagedUploads ?? [], model: model ?? null})
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

export const AGENT_HOME = '/home/agent';

export async function listWorkspaceFiles(threadId: string, path = AGENT_HOME): Promise<WorkspaceList> {
	return request(`/api/threads/${threadId}/files?path=${encodeURIComponent(path)}`);
}

export async function listWorkspaceFileVersions(threadId: string, path: string): Promise<FileVersions> {
	return request(`/api/threads/${threadId}/file-versions?path=${encodeURIComponent(path)}`);
}

export async function restoreWorkspaceFileVersion(threadId: string, path: string, version: number): Promise<void> {
	await request(`/api/threads/${threadId}/file-versions`, {
		method: 'POST',
		body: JSON.stringify({path, version})
	});
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
	return downloadWorkspaceFileVersion(threadId, path);
}

export async function downloadWorkspaceFileVersion(threadId: string, path: string, version?: number): Promise<Blob> {
	const token = readToken();
	const headers = new Headers();
	if (token) headers.set('Authorization', `Bearer ${token}`);

	const query = version == null ? '' : `?version=${encodeURIComponent(String(version))}`;
	const response = await fetch(`/api/threads/${threadId}/files/${encodePath(path)}${query}`, {headers});
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

export async function uploadFiles(threadId: string, files: File[], path = AGENT_HOME): Promise<UploadedFile[]> {
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

export async function transcribeAudio(audio: Blob, fileName = 'voice.webm'): Promise<string> {
	const token = readToken();
	const headers = new Headers();
	headers.set('Accept', 'application/json');
	if (token) headers.set('Authorization', `Bearer ${token}`);

	const body = new FormData();
	body.append('file', audio, fileName);

	const response = await fetch('/api/transcribe', {method: 'POST', headers, body});
	if (!response.ok) {
		const detail = (await response.text()).trim();
		throw new Error(detail || `Transcription failed (${response.status})`);
	}
	const data = await response.json() as {text: string};
	return data.text;
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
