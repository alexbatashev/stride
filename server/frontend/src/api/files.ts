import {readToken} from './auth.js';

export type FileEntry = {
	name: string;
	path: string;
	kind: 'directory' | 'file';
	size: number | null;
	updated_at: number;
	mime_type: string | null;
};

export type FileList = {
	path: string;
	entries: FileEntry[];
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

export type UploadedFile = {
	name: string;
	path: string;
	size: number;
};

export async function listFiles(path = ''): Promise<FileList> {
	return request(`/api/files?path=${encodeURIComponent(path)}`);
}

export async function listFileVersions(path: string): Promise<FileVersions> {
	return request(`/api/files/versions?path=${encodeURIComponent(path)}`);
}

export async function restoreFileVersion(path: string, version: number): Promise<void> {
	await request('/api/files/versions', {
		method: 'POST',
		body: JSON.stringify({path, version})
	});
}

export async function createDirectory(path: string): Promise<void> {
	await request('/api/files/directories', {
		method: 'POST',
		body: JSON.stringify({path}),
	});
}

export async function renameEntry(path: string, name: string): Promise<void> {
	await request('/api/files/rename', {
		method: 'PATCH',
		body: JSON.stringify({path, name}),
	});
}

export async function deleteEntry(path: string): Promise<void> {
	await request(`/api/files/${encodePath(path)}`, {method: 'DELETE'});
}

export async function downloadFile(path: string): Promise<Blob> {
	return downloadFileVersion(path);
}

export async function downloadFileVersion(path: string, version?: number): Promise<Blob> {
	const token = readToken();
	const headers = new Headers();
	if (token) headers.set('Authorization', `Bearer ${token}`);

	const query = version == null ? '' : `?version=${encodeURIComponent(String(version))}`;
	const response = await fetch(`/api/files/${encodePath(path)}${query}`, {headers});
	if (!response.ok) throw new Error(`${response.status}`);
	return response.blob();
}

export async function uploadFiles(files: File[], path = ''): Promise<UploadedFile[]> {
	const token = readToken();
	const headers = new Headers();
	headers.set('Accept', 'application/json');
	if (token) headers.set('Authorization', `Bearer ${token}`);

	const body = new FormData();
	for (const file of files) {
		body.append('file', file, file.name);
	}

	const response = await fetch(`/api/files?path=${encodeURIComponent(path)}`, {method: 'POST', headers, body});
	if (!response.ok) throw new Error(`${response.status}`);
	const data = (await response.json()) as {files: UploadedFile[]};
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
