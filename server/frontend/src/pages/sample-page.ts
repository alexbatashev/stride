import {LitElement, css, html} from 'lit';
import {logout} from '../api/auth.js';

export class SamplePage extends LitElement {
	static styles = css`
		:host {
			display: block;
			min-height: 100vh;
		}

		header {
			align-items: center;
			border-bottom: 1px solid #e5e9f2;
			display: flex;
			justify-content: space-between;
			padding: 16px 24px;
		}

		main {
			margin: 0 auto;
			max-width: 840px;
			padding: 48px 24px;
		}

		h1 {
			margin: 0 0 12px;
		}

		p {
			color: #546179;
			line-height: 1.5;
			margin: 0;
		}
	`;

	render() {
		return html`
			<header>
				<strong>Friday</strong>
				<app-button variant="secondary" @click=${this.onLogout}>Log out</app-button>
			</header>
			<main>
				<h1>Authenticated page</h1>
				<p>This page is visible only after login or registration stores a session token.</p>
			</main>
		`;
	}

	private async onLogout() {
		await logout();
		this.dispatchEvent(new CustomEvent('navigate', {bubbles: true, composed: true, detail: {path: '/login'}}));
	}
}

customElements.define('sample-page', SamplePage);
