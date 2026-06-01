import {LitElement, css, html} from 'lit';
import {AuthMode, authenticate} from '../api/auth.js';
import './app-text-input.js';

export class AuthForm extends LitElement {
	static properties = {
		error: {state: true},
		loading: {state: true},
		mode: {type: String}
	};

	mode: AuthMode = 'login';
	private error = '';
	private loading = false;
	private password = '';
	private username = '';

	static styles = css`
		:host {
			display: block;
		}

		form {
			display: grid;
			gap: 16px;
		}

		.actions {
			display: grid;
			gap: 10px;
			grid-template-columns: 1fr 1fr;
			margin-top: 4px;
		}

		.error {
			background: #fff1f0;
			border: 1px solid #ffccc7;
			border-radius: 6px;
			color: #9f1d16;
			font-size: 14px;
			margin: 0;
			padding: 10px 12px;
		}
	`;

	render() {
		const isLogin = this.mode === 'login';
		const title = isLogin ? 'Log in' : 'Create account';
		const submitLabel = isLogin ? 'Log in' : 'Register';
		const switchLabel = isLogin ? 'Register' : 'Log in';

		return html`
			<form @submit=${this.onSubmit}>
				<h1>${title}</h1>
				${this.error ? html`<p class="error">${this.error}</p>` : null}
				<app-text-input
					label="Username"
					name="username"
					autocomplete="username"
					.value=${this.username}
					?disabled=${this.loading}
					required
					@commit=${this.onSubmit}
					@value-change=${this.onUsernameChange}
				></app-text-input>
				<app-text-input
					label="Password"
					name="password"
					type="password"
					autocomplete=${isLogin ? 'current-password' : 'new-password'}
					.value=${this.password}
					?disabled=${this.loading}
					required
					@commit=${this.onSubmit}
					@value-change=${this.onPasswordChange}
				></app-text-input>
				<div class="actions">
					<app-button ?loading=${this.loading} @click=${this.onSubmit}>${submitLabel}</app-button>
					<app-button variant="secondary" ?disabled=${this.loading} @click=${this.switchMode}>
						${switchLabel}
					</app-button>
				</div>
			</form>
		`;
	}

	private onUsernameChange(event: CustomEvent<{value: string}>) {
		this.username = event.detail.value;
	}

	private onPasswordChange(event: CustomEvent<{value: string}>) {
		this.password = event.detail.value;
	}

	private switchMode() {
		const mode: AuthMode = this.mode === 'login' ? 'register' : 'login';
		this.dispatchEvent(new CustomEvent('auth-mode-change', {bubbles: true, composed: true, detail: {mode}}));
	}

	private async onSubmit(event: Event) {
		event.preventDefault();
		this.loading = true;
		this.error = '';

		try {
			await authenticate(this.mode, this.username, this.password);
			this.dispatchEvent(new CustomEvent('auth-success', {bubbles: true, composed: true}));
		} catch (error) {
			this.error = error instanceof Error ? error.message : 'Auth request failed.';
		} finally {
			this.loading = false;
		}
	}
}

customElements.define('auth-form', AuthForm);
