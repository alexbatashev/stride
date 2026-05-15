/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import {LitElement, css, html} from 'lit';

type ButtonSize = 'default' | 'xs' | 'sm' | 'lg' | 'icon' | 'icon-xs' | 'icon-sm' | 'icon-lg';
type ButtonVariant = 'default' | 'primary' | 'outline' | 'secondary' | 'ghost' | 'destructive' | 'link';

export class AppButton extends LitElement {
	static properties = {
		disabled: {type: Boolean},
		loading: {type: Boolean},
		size: {type: String},
		type: {type: String},
		variant: {type: String}
	};

	disabled = false;
	loading = false;
	size: ButtonSize = 'default';
	type: 'button' | 'submit' = 'button';
	variant: ButtonVariant = 'default';

	static styles = css`
		:host {
			display: inline-block;
		}

		button {
			align-items: center;
			background: var(--primary, #18181b);
			background-clip: padding-box;
			border: 1px solid transparent;
			border-radius: 8px;
			box-sizing: border-box;
			color: var(--primary-foreground, #fafafa);
			cursor: pointer;
			display: flex;
			font: inherit;
			font-size: 0.875rem;
			font-weight: 500;
			gap: 6px;
			height: 32px;
			justify-content: center;
			line-height: 1;
			outline: none;
			padding: 0 10px;
			position: relative;
			transition:
				background-color 140ms ease,
				border-color 140ms ease,
				box-shadow 140ms ease,
				color 140ms ease,
				opacity 140ms ease,
				transform 80ms ease;
			user-select: none;
			white-space: nowrap;
			width: 100%;
		}

		button:hover:not(:disabled) {
			background: var(--primary-hover, #27272a);
		}

		button:focus-visible {
			border-color: var(--ring, #18181b);
			box-shadow: 0 0 0 3px var(--ring-shadow, rgb(24 24 27 / 12%));
		}

		button:active:not(:disabled) {
			transform: translateY(1px);
		}

		button[data-variant='outline'] {
			background: var(--background, #ffffff);
			border-color: var(--border, #e4e4e7);
			color: var(--foreground, #18181b);
		}

		button[data-variant='outline']:hover:not(:disabled) {
			background: var(--muted, #f4f4f5);
		}

		button[data-variant='secondary'] {
			background: var(--secondary, #f4f4f5);
			color: var(--secondary-foreground, #18181b);
		}

		button[data-variant='secondary']:hover:not(:disabled) {
			background: var(--secondary-hover, #e4e4e7);
		}

		button[data-variant='ghost'] {
			background: transparent;
			color: var(--foreground, #18181b);
		}

		button[data-variant='ghost']:hover:not(:disabled) {
			background: var(--muted, #f4f4f5);
		}

		button[data-variant='destructive'] {
			background: var(--destructive-muted, rgb(220 38 38 / 10%));
			color: var(--destructive, #dc2626);
		}

		button[data-variant='destructive']:hover:not(:disabled) {
			background: var(--destructive-hover, rgb(220 38 38 / 20%));
		}

		button[data-variant='destructive']:focus-visible {
			border-color: var(--destructive-ring, rgb(220 38 38 / 40%));
			box-shadow: 0 0 0 3px var(--destructive-shadow, rgb(220 38 38 / 20%));
		}

		button[data-variant='link'] {
			background: transparent;
			color: var(--primary, #18181b);
			height: auto;
			padding: 0;
			text-underline-offset: 4px;
		}

		button[data-variant='link']:hover:not(:disabled) {
			background: transparent;
			text-decoration: underline;
		}

		button[data-size='xs'] {
			border-radius: 8px;
			font-size: 0.75rem;
			gap: 4px;
			height: 24px;
			padding: 0 8px;
		}

		button[data-size='sm'] {
			border-radius: 8px;
			font-size: 0.8rem;
			gap: 4px;
			height: 28px;
			padding: 0 10px;
		}

		button[data-size='lg'] {
			height: 36px;
			padding: 0 10px;
		}

		button[data-size='icon'] {
			height: 32px;
			padding: 0;
			width: 32px;
		}

		button[data-size='icon-xs'] {
			border-radius: 8px;
			height: 24px;
			padding: 0;
			width: 24px;
		}

		button[data-size='icon-sm'] {
			border-radius: 8px;
			height: 28px;
			padding: 0;
			width: 28px;
		}

		button[data-size='icon-lg'] {
			height: 36px;
			padding: 0;
			width: 36px;
		}

		button:disabled {
			cursor: default;
			opacity: 0.5;
			pointer-events: none;
		}

		.spinner {
			animation: spin 800ms linear infinite;
			border: 2px solid currentcolor;
			border-radius: 999px;
			border-right-color: transparent;
			display: inline-block;
			height: 14px;
			width: 14px;
		}

		@keyframes spin {
			to {
				transform: rotate(360deg);
			}
		}
	`;

	render() {
		const variant = this.variant === 'primary' ? 'default' : this.variant;

		return html`
			<button
				data-variant=${variant}
				data-size=${this.size}
				type=${this.type}
				?disabled=${this.disabled || this.loading}
				aria-busy=${this.loading ? 'true' : 'false'}
			>
				${this.loading ? html`<span class="spinner" aria-hidden="true"></span>` : null}
				<slot></slot>
			</button>
		`;
	}
}

customElements.define('app-button', AppButton);
