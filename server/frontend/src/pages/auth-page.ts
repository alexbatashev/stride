import {LitElement, css, html} from 'lit';
import {AuthMode} from '../api/auth.js';
import '../components/auth-form.js';

export class AuthPage extends LitElement {
	static properties = {
		mode: {type: String}
	};

	mode: AuthMode = 'login';

	static styles = css`
		:host {
			align-items: center;
			display: grid;
			min-height: 100vh;
			padding: 24px;
		}

		main {
			margin: 0 auto;
			max-width: 420px;
			width: 100%;
		}
	`;

	render() {
		return html`
			<main>
				<auth-form .mode=${this.mode} @auth-success=${this.onAuthSuccess} @auth-mode-change=${this.onModeChange}></auth-form>
			</main>
		`;
	}

	private onAuthSuccess() {
		this.dispatchEvent(new CustomEvent('navigate', {bubbles: true, composed: true, detail: {path: '/'}}));
	}

	private onModeChange(event: CustomEvent<{mode: AuthMode}>) {
		const path = event.detail.mode === 'login' ? '/login' : '/register';
		this.dispatchEvent(new CustomEvent('navigate', {bubbles: true, composed: true, detail: {path}}));
	}
}

customElements.define('auth-page', AuthPage);
