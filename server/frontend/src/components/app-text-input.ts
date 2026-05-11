import {LitElement, css, html} from 'lit';

export class AppTextInput extends LitElement {
	static properties = {
		autocomplete: {type: String},
		disabled: {type: Boolean},
		label: {type: String},
		name: {type: String},
		required: {type: Boolean},
		type: {type: String},
		value: {type: String}
	};

	autocomplete = '';
	disabled = false;
	label = '';
	name = '';
	required = false;
	type = 'text';
	value = '';

	static styles = css`
		:host {
			display: block;
		}

		label {
			color: #202a3d;
			display: grid;
			font-size: 14px;
			font-weight: 650;
			gap: 8px;
		}

		input {
			border: 1px solid #ccd2df;
			border-radius: 6px;
			box-sizing: border-box;
			color: #172033;
			font: inherit;
			min-height: 42px;
			padding: 0 12px;
			width: 100%;
		}

		input:focus {
			border-color: #1f5eff;
			box-shadow: 0 0 0 3px rgb(31 94 255 / 15%);
			outline: none;
		}
	`;

	render() {
		return html`
			<label>
				${this.label}
				<input
					.autocomplete=${this.autocomplete}
					.name=${this.name}
					.type=${this.type}
					.value=${this.value}
					?disabled=${this.disabled}
					?required=${this.required}
					@input=${this.onInput}
					@keydown=${this.onKeydown}
				/>
			</label>
		`;
	}

	private onInput(event: Event) {
		this.value = (event.target as HTMLInputElement).value;
		this.dispatchEvent(
			new CustomEvent('value-change', {
				bubbles: true,
				composed: true,
				detail: {value: this.value}
			})
		);
	}

	private onKeydown(event: KeyboardEvent) {
		if (event.key !== 'Enter') {
			return;
		}

		this.dispatchEvent(new CustomEvent('commit', {bubbles: true, composed: true}));
	}
}

customElements.define('app-text-input', AppTextInput);
