import {LitElement, css, html} from 'lit';
import './components/app-button.js';
import './components/app-text-input.js';
import './components/app-sidebar.js';
import './components/auth-form.js';

class FridayShowcase extends LitElement {
	static styles = css`
		:host {
			display: block;
			font-family: system-ui, sans-serif;
			padding: 2rem;
			background: #f5f5f5;
			min-height: 100vh;
		}

		h1 {
			margin: 0 0 2rem;
			font-size: 1.5rem;
			color: #172033;
		}

		h2 {
			margin: 0 0 1rem;
			font-size: 0.75rem;
			color: #888;
			text-transform: uppercase;
			letter-spacing: 0.06em;
			font-weight: 600;
		}

		.section {
			background: white;
			border-radius: 8px;
			padding: 1.5rem;
			margin-bottom: 1.5rem;
			border: 1px solid #e5e7eb;
		}

		.row {
			display: flex;
			gap: 1rem;
			align-items: flex-start;
			flex-wrap: wrap;
			margin-bottom: 1rem;
		}

		.row:last-child {
			margin-bottom: 0;
		}

		.sidebar-demo {
			height: 360px;
			border: 1px solid #e5e7eb;
			border-radius: 8px;
			overflow: hidden;
		}
	`;

	render() {
		return html`
			<h1>Component Showcase</h1>

			<div class="section">
				<h2>Button</h2>
				<div class="row">
					<app-button style="width:120px">Primary</app-button>
					<app-button variant="secondary" style="width:120px">Secondary</app-button>
				</div>
				<div class="row">
					<app-button disabled style="width:120px">Disabled</app-button>
					<app-button loading style="width:120px">Loading</app-button>
				</div>
			</div>

			<div class="section">
				<h2>Text Input</h2>
				<div class="row">
					<app-text-input label="Username" style="width:280px"></app-text-input>
					<app-text-input label="Password" type="password" style="width:280px"></app-text-input>
				</div>
				<div class="row">
					<app-text-input label="Disabled" disabled style="width:280px"></app-text-input>
				</div>
			</div>

			<div class="section">
				<h2>Auth Form</h2>
				<div class="row">
					<div style="width:320px">
						<auth-form mode="login"></auth-form>
					</div>
					<div style="width:320px">
						<auth-form mode="register"></auth-form>
					</div>
				</div>
			</div>

			<div class="section">
				<h2>Sidebar</h2>
				<div class="sidebar-demo">
					<app-sidebar-provider>
						<app-sidebar collapsible="icon">
							<app-sidebar-header>
								<app-sidebar-menu>
									<app-sidebar-menu-item>
										<app-sidebar-trigger></app-sidebar-trigger>
									</app-sidebar-menu-item>
								</app-sidebar-menu>
							</app-sidebar-header>
							<app-sidebar-content>
								<app-sidebar-group>
									<app-sidebar-group-label>Navigation</app-sidebar-group-label>
									<app-sidebar-group-content>
										<app-sidebar-menu>
											<app-sidebar-menu-item>
												<app-sidebar-menu-button active tooltip="Home">Home</app-sidebar-menu-button>
											</app-sidebar-menu-item>
											<app-sidebar-menu-item>
												<app-sidebar-menu-button tooltip="Threads">Threads</app-sidebar-menu-button>
											</app-sidebar-menu-item>
											<app-sidebar-menu-item>
												<app-sidebar-menu-button tooltip="Settings">Settings</app-sidebar-menu-button>
											</app-sidebar-menu-item>
										</app-sidebar-menu>
									</app-sidebar-group-content>
								</app-sidebar-group>
							</app-sidebar-content>
							<app-sidebar-footer>
								<app-sidebar-menu>
									<app-sidebar-menu-item>
										<app-sidebar-menu-button tooltip="Account">Account</app-sidebar-menu-button>
									</app-sidebar-menu-item>
								</app-sidebar-menu>
							</app-sidebar-footer>
							<app-sidebar-rail></app-sidebar-rail>
						</app-sidebar>
						<app-sidebar-inset>
							<p style="padding:1rem;color:#888;font-size:0.875rem">Main content area</p>
						</app-sidebar-inset>
					</app-sidebar-provider>
				</div>
			</div>
		`;
	}
}

customElements.define('friday-showcase', FridayShowcase);
