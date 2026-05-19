/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import {LitElement, css, html} from 'lit';

const STOP_ICON = html`
	<svg
		xmlns="http://www.w3.org/2000/svg"
		width="16"
		height="16"
		viewBox="0 0 24 24"
		fill="currentColor"
		aria-hidden="true"
	>
		<rect x="4" y="4" width="16" height="16" rx="2"/>
	</svg>
`;

const ARROW_UP_ICON = html`
	<svg
		xmlns="http://www.w3.org/2000/svg"
		width="18"
		height="18"
		viewBox="0 0 24 24"
		fill="none"
		stroke="currentColor"
		stroke-width="2.5"
		stroke-linecap="round"
		stroke-linejoin="round"
		aria-hidden="true"
	>
		<path d="m5 12 7-7 7 7" />
		<path d="M12 19V5" />
	</svg>
`;

const PLUS_ICON = html`
	<svg
		xmlns="http://www.w3.org/2000/svg"
		width="22"
		height="22"
		viewBox="0 0 24 24"
		fill="none"
		stroke="currentColor"
		stroke-width="2"
		stroke-linecap="round"
		stroke-linejoin="round"
		aria-hidden="true"
	>
		<path d="M5 12h14" />
		<path d="M12 5v14" />
	</svg>
`;

const TOOLS_ICON = html`
	<svg
		xmlns="http://www.w3.org/2000/svg"
		width="21"
		height="21"
		viewBox="0 0 24 24"
		fill="none"
		stroke="currentColor"
		stroke-width="2"
		stroke-linecap="round"
		stroke-linejoin="round"
		aria-hidden="true"
	>
		<path d="M4 4v6" />
		<path d="M4 14v6" />
		<path d="M8 7H4" />
		<path d="M8 17H4" />
		<path d="M14 5h6" />
		<path d="M14 12h6" />
		<path d="M14 19h6" />
		<path d="M11 5v14" />
	</svg>
`;

const MONITOR_ICON = html`
	<svg
		xmlns="http://www.w3.org/2000/svg"
		width="18"
		height="18"
		viewBox="0 0 24 24"
		fill="none"
		stroke="currentColor"
		stroke-width="2"
		stroke-linecap="round"
		stroke-linejoin="round"
		aria-hidden="true"
	>
		<rect width="18" height="12" x="3" y="4" rx="2" />
		<path d="M8 20h8" />
		<path d="M12 16v4" />
	</svg>
`;

const MIC_ICON = html`
	<svg
		xmlns="http://www.w3.org/2000/svg"
		width="22"
		height="22"
		viewBox="0 0 24 24"
		fill="none"
		stroke="currentColor"
		stroke-width="2"
		stroke-linecap="round"
		stroke-linejoin="round"
		aria-hidden="true"
	>
		<path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z" />
		<path d="M19 10v2a7 7 0 0 1-14 0v-2" />
		<path d="M12 19v3" />
	</svg>
`;

export class AppPromptInput extends LitElement {
	static properties = {
		disabled: {type: Boolean},
		running: {type: Boolean},
		placeholder: {type: String},
		value: {type: String}
	};

	disabled = false;
	running = false;
	placeholder = 'Send a message';
	value = '';

	static styles = css`
		:host {
			display: block;
		}

		form {
			background: var(--prompt-bg, #212121);
			border: 1px solid var(--prompt-border, #333333);
			border-radius: 28px;
			box-shadow: var(--prompt-shadow, none);
			box-sizing: border-box;
			display: grid;
			gap: 34px;
			padding: 32px 24px 26px;
			transition:
				border-color 140ms ease,
				box-shadow 140ms ease;
		}

		form:focus-within {
			border-color: var(--prompt-focus-border, #3c3c3c);
			box-shadow: 0 0 0 3px var(--prompt-ring, rgb(255 255 255 / 7%));
		}

		textarea {
			background: transparent;
			border: 0;
			color: var(--prompt-fg, #d4d4d4);
			font: inherit;
			font-size: 1.25rem;
			line-height: 1.5;
			max-height: 220px;
			min-height: 48px;
			min-width: 0;
			outline: none;
			overflow-y: auto;
			padding: 0;
			resize: none;
			width: 100%;
		}

		textarea::placeholder {
			color: var(--prompt-muted, #747474);
		}

		textarea:disabled {
			cursor: not-allowed;
			opacity: 0.5;
		}

		.toolbar {
			align-items: center;
			display: flex;
			gap: 16px;
			justify-content: space-between;
			min-height: 42px;
		}

		.actions {
			align-items: center;
			display: flex;
			gap: 14px;
			min-width: 0;
		}

		.right-actions {
			align-items: center;
			display: flex;
			gap: 18px;
		}

		.tool-button,
		.send {
			align-items: center;
			border-radius: 999px;
			display: inline-flex;
			flex: 0 0 auto;
			justify-content: center;
			outline: none;
			user-select: none;
			transition:
				background-color 140ms ease,
				border-color 140ms ease,
				box-shadow 140ms ease,
				color 140ms ease,
				opacity 140ms ease;
			white-space: nowrap;
		}

		.tool-button {
			background: transparent;
			border: 1px solid var(--prompt-control-border, #343434);
			color: var(--prompt-control-fg, #bdbdbd);
			cursor: pointer;
			font: inherit;
			font-size: 0.95rem;
			font-weight: 500;
			gap: 8px;
			height: 42px;
			padding: 0 14px;
		}

		.tool-button.icon {
			font-size: 1.5rem;
			padding: 0;
			width: 42px;
		}

		.tool-button:hover {
			background: var(--prompt-control-hover-bg, #2d2d2d);
			color: var(--prompt-control-hover-fg, #e4e4e7);
		}

		.badge {
			background: var(--prompt-badge-bg, #10233d);
			border-radius: 8px;
			color: var(--prompt-badge-fg, #4da3ff);
			font-weight: 600;
			margin-left: 2px;
			padding: 2px 6px;
		}

		.mic {
			background: transparent;
			border: 0;
			color: var(--prompt-control-fg, #bdbdbd);
			cursor: pointer;
			height: 42px;
			padding: 0;
			width: 42px;
		}

		.mic:hover {
			background: transparent;
			color: var(--prompt-control-hover-fg, #e4e4e7);
		}

		.send {
			background: var(--prompt-send-bg, #333333);
			border: 1px solid var(--prompt-send-bg, #333333);
			color: var(--prompt-send-fg, #777777);
			cursor: pointer;
			height: 42px;
			width: 42px;
		}

		.send:not(:disabled) {
			background: var(--prompt-send-ready-bg, #f4f4f5);
			border-color: var(--prompt-send-ready-bg, #f4f4f5);
			color: var(--prompt-send-ready-fg, #18181b);
		}

		.send:hover:not(:disabled) {
			opacity: 0.92;
		}

		.tool-button:focus-visible,
		.send:focus-visible {
			box-shadow: 0 0 0 3px var(--prompt-ring, rgb(255 255 255 / 7%));
		}

		.send:disabled {
			cursor: not-allowed;
			opacity: 0.5;
			pointer-events: none;
		}

		.send.stop {
			background: var(--prompt-send-ready-bg, #f4f4f5);
			border-color: var(--prompt-send-ready-bg, #f4f4f5);
			color: var(--prompt-send-ready-fg, #18181b);
		}

		.send.stop:hover {
			opacity: 0.92;
		}

		.sr-only {
			border: 0;
			clip: rect(0, 0, 0, 0);
			height: 1px;
			margin: -1px;
			overflow: hidden;
			padding: 0;
			position: absolute;
			white-space: nowrap;
			width: 1px;
		}

		@media (min-width: 768px) {
			textarea {
				font-size: 1.25rem;
			}
		}

		@media (max-width: 640px) {
			:host {
				max-width: 100%;
			}

			form {
				border-radius: 24px;
				gap: 24px;
				padding: 22px 18px 18px;
				width: 100%;
			}

			.toolbar {
				align-items: flex-end;
				gap: 10px;
			}

			.actions {
				gap: 8px;
				min-width: 0;
			}

			.tool-button.text {
				display: none;
			}

			.right-actions {
				gap: 8px;
			}
		}
	`;

	render() {
		return html`
			<form @submit=${this.onSubmit}>
				<textarea
					.value=${this.value}
					placeholder=${this.placeholder}
					rows="2"
					?disabled=${this.disabled || this.running}
					@input=${this.onInput}
					@keydown=${this.onKeydown}
				></textarea>
				<div class="toolbar">
					<div class="actions">
						<slot name="actions">
							<button class="tool-button icon" type="button" aria-label="Add attachment">${PLUS_ICON}</button>
							<button class="tool-button icon" type="button" aria-label="Tools">${TOOLS_ICON}</button>
							<button class="tool-button text" type="button">
								${MONITOR_ICON}
								<span>Cloud computers</span>
								<span class="badge">New</span>
							</button>
						</slot>
					</div>
					<div class="right-actions">
						<slot name="right-actions">
							<button class="tool-button icon mic" type="button" aria-label="Voice input">${MIC_ICON}</button>
						</slot>
						${this.running
							? html`<button class="send stop" type="button" @click=${this.onStop}>
									${STOP_ICON}
									<span class="sr-only">Stop</span>
								</button>`
							: html`<button class="send" type="submit" ?disabled=${this.disabled || !this.value.trim()}>
									${ARROW_UP_ICON}
									<span class="sr-only">Send message</span>
								</button>`
						}
					</div>
				</div>
			</form>
		`;
	}

	private onInput(event: Event) {
		this.value = (event.target as HTMLTextAreaElement).value;
		this.dispatchEvent(
			new CustomEvent('value-change', {
				bubbles: true,
				composed: true,
				detail: {value: this.value}
			})
		);
	}

	private onKeydown(event: KeyboardEvent) {
		if (event.key !== 'Enter' || event.shiftKey || event.isComposing) {
			return;
		}

		event.preventDefault();
		this.submit();
	}

	private onStop() {
		this.dispatchEvent(new CustomEvent('prompt-stop', {bubbles: true, composed: true}));
	}

	private onSubmit(event: SubmitEvent) {
		event.preventDefault();
		this.submit();
	}

	private submit() {
		const value = this.value.trim();
		if (!value || this.disabled) {
			return;
		}

		this.dispatchEvent(
			new CustomEvent('prompt-submit', {
				bubbles: true,
				composed: true,
				detail: {value}
			})
		);
	}
}

customElements.define('app-prompt-input', AppPromptInput);
