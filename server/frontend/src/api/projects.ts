import {readToken} from './auth.js';

export type ProjectSummary = {
	id: string;
	title: string;
};

export async function listProjects(): Promise<ProjectSummary[]> {
	return request('/api/projects');
}

export async function createProject(title: string): Promise<ProjectSummary> {
	return request('/api/projects', {
		method: 'POST',
		body: JSON.stringify({title})
	});
}

export async function renameProject(id: string, title: string): Promise<ProjectSummary> {
	return request(`/api/projects/${id}`, {
		method: 'PATCH',
		body: JSON.stringify({title})
	});
}

export async function deleteProject(id: string): Promise<void> {
	await request(`/api/projects/${id}`, {method: 'DELETE'});
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

	return (await response.json()) as T;
}
