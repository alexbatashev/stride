import {LitElement, css, html} from 'lit';

export class AppButton extends LitElement {
	static properties = {
		disabled: {type: Boolean},
		loading: {type: Boolean},
		type: {type: String},
		variant: {type: String}
	};

	disabled = false;
	loading = false;
	type: 'button' | 'submit' = 'button';
	variant: 'primary' | 'secondary' = 'primary';

	static styles = css`
		:host {
			display: inline-block;
		}

		button {
			align-items: center;
			background: #1f5eff;
			border: 1px solid #1f5eff;
			border-radius: 6px;
			color: white;
			cursor: pointer;
			display: inline-flex;
			font: inherit;
			font-weight: 650;
			gap: 8px;
			justify-content: center;
			min-height: 42px;
			padding: 0 16px;
			width: 100%;
		}

		button.secondary {
			background: white;
			border-color: #ccd2df;
			color: #172033;
		}

		button:disabled {
			cursor: default;
			opacity: 0.65;
		}
	`;

	render() {
		return html`
			<button
				class=${this.variant}
				type=${this.type}
				?disabled=${this.disabled || this.loading}
				aria-busy=${this.loading ? 'true' : 'false'}
			>
				<slot></slot>
			</button>
		`;
	}
}

customElements.define('app-button', AppButton);
