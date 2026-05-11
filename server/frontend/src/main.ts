import {LitElement, css, html} from 'lit';
import {AuthMode} from './api/auth.js';
import {readToken} from './api/auth.js';
import './pages/auth-page.js';
import './pages/sample-page.js';

class FridayApp extends LitElement {
	static properties = {
		path: {state: true}
	};

	private path = this.initialPath();

	static styles = css`
		:host {
			color: #172033;
			display: block;
			font-family:
				Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
		}

		* {
			box-sizing: border-box;
		}
	`;

	connectedCallback() {
		super.connectedCallback();
		window.addEventListener('popstate', this.onPopState);
	}

	disconnectedCallback() {
		window.removeEventListener('popstate', this.onPopState);
		super.disconnectedCallback();
	}

	render() {
		const route = this.route();

		if (route === 'home') {
			return html`<sample-page @navigate=${this.onNavigate}></sample-page>`;
		}

		return html`<auth-page .mode=${route} @navigate=${this.onNavigate}></auth-page>`;
	}

	private route(): 'home' | AuthMode {
		if (this.path === '/register') {
			return 'register';
		}

		if (this.path === '/' && readToken()) {
			return 'home';
		}

		return 'login';
	}

	private onNavigate(event: CustomEvent<{path: string}>) {
		this.navigate(event.detail.path);
	}

	private onPopState = () => {
		this.path = window.location.pathname;
	};

	private navigate(path: string) {
		window.history.pushState({}, '', path);
		this.path = path;
	}

	private initialPath() {
		const path = window.location.pathname;

		if (path === '/' && !readToken()) {
			window.history.replaceState({}, '', '/login');
			return '/login';
		}

		return path;
	}
}

customElements.define('friday-app', FridayApp);
