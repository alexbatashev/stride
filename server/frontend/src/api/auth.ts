export type AuthMode = 'login' | 'register';

type AuthResponse = {
	token: string;
};

const TOKEN_KEY = 'stride.authToken';

export function readToken(): string | null {
	return localStorage.getItem(TOKEN_KEY);
}

export function saveToken(token: string) {
	localStorage.setItem(TOKEN_KEY, token);
}

export function clearToken() {
	localStorage.removeItem(TOKEN_KEY);
}

export async function authenticate(mode: AuthMode, username: string, password: string): Promise<string> {
	const response = await fetch(`/api/${mode}`, {
		method: 'POST',
		headers: {
			'Content-Type': 'application/json'
		},
		body: JSON.stringify({username, password})
	});

	if (!response.ok) {
		throw new Error(authErrorMessage(mode, response.status));
	}

	const body = (await response.json()) as AuthResponse;
	saveToken(body.token);
	return body.token;
}

export async function logout() {
	const token = readToken();
	clearToken();

	if (!token) {
		return;
	}

	await fetch('/api/logout', {
		method: 'POST',
		headers: {
			Authorization: `Bearer ${token}`
		}
	});
}

function authErrorMessage(mode: AuthMode, status: number): string {
	if (status === 400) {
		return 'Enter username and password.';
	}

	if (status === 401) {
		return 'Username or password is incorrect.';
	}

	if (status === 403) {
		return mode === 'register' ? 'Registration is disabled.' : 'Could not sign in.';
	}

	if (status === 409) {
		return mode === 'register' ? 'Username is already taken.' : 'Could not sign in.';
	}

	return 'Auth request failed.';
}
