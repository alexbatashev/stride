/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import {LitElement, css, html} from 'lit';

export class AppTextInput extends LitElement {
	static properties = {
		autocomplete: {type: String},
		disabled: {type: Boolean},
		label: {type: String},
		name: {type: String},
		placeholder: {type: String},
		required: {type: Boolean},
		type: {type: String},
		value: {type: String}
	};

	autocomplete = '';
	disabled = false;
	label = '';
	name = '';
	placeholder = '';
	required = false;
	type = 'text';
	value = '';

	static styles = css`
		:host {
			display: block;
		}

		label {
			color: var(--foreground, #09090b);
			display: grid;
			font-size: 0.875rem;
			font-weight: 500;
			gap: 8px;
			line-height: 1.35;
		}

		input {
			background: var(--background, transparent);
			border: 1px solid var(--input, #e4e4e7);
			border-radius: 8px;
			box-sizing: border-box;
			color: var(--foreground, #09090b);
			font: inherit;
			font-size: 1rem;
			height: 32px;
			min-width: 0;
			outline: none;
			padding: 4px 10px;
			transition:
				border-color 140ms ease,
				box-shadow 140ms ease,
				background-color 140ms ease,
				opacity 140ms ease;
			width: 100%;
		}

		input:focus {
			border-color: var(--ring, #18181b);
			box-shadow: 0 0 0 3px var(--ring-shadow, rgb(24 24 27 / 12%));
		}

		input::placeholder {
			color: var(--muted-foreground, #71717a);
		}

		input:disabled {
			background: var(--input-disabled, rgb(244 244 245 / 50%));
			cursor: not-allowed;
			opacity: 0.5;
		}

		input[type='file'] {
			padding-block: 3px;
		}

		input[type='file']::file-selector-button {
			background: transparent;
			border: 0;
			color: var(--foreground, #09090b);
			font: inherit;
			font-size: 0.875rem;
			font-weight: 500;
			height: 24px;
			margin: 0 10px 0 0;
			padding: 0;
		}

		@media (min-width: 768px) {
			input {
				font-size: 0.875rem;
			}
		}
	`;

	render() {
		return html`
			<label>
				${this.label}
				<input
					.autocomplete=${this.autocomplete}
					.name=${this.name}
					.placeholder=${this.placeholder}
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
