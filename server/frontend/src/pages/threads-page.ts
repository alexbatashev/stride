import {LitElement, css, html, nothing} from 'lit';
import {logout} from '../api/auth.js';
import {
	ThreadEvent,
	ThreadMessage,
	ThreadSummary,
	createThread,
	listMessages,
	listThreads,
	sendMessage
} from '../api/threads.js';
import '../components/app-button.js';
import '../components/app-sidebar.js';

type ViewMessage = ThreadMessage & {pending?: boolean};

export class ThreadsPage extends LitElement {
	static properties = {
		threadId: {type: String, attribute: 'thread-id'},
		threads: {state: true},
		messages: {state: true},
		draft: {state: true},
		running: {state: true},
		loading: {state: true},
		error: {state: true}
	};

	threadId = '';
	threads: ThreadSummary[] = [];
	messages: ViewMessage[] = [];
	draft = '';
	running = false;
	loading = true;
	error = '';

	private events: EventSource | null = null;
	private pendingAssistant = '';

	static styles = css`
		:host {
			background: #f6f7f9;
			color: #1f2933;
			display: block;
			font-family:
				Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
			min-height: 100vh;
		}

		app-sidebar-provider {
			--sidebar-width: 17.5rem;
			--sidebar-bg: #eef1f4;
			--sidebar-fg: #344150;
			--sidebar-accent: #dfe5ec;
			--sidebar-accent-fg: #111820;
			--sidebar-border: #d9dee5;
			background: #f6f7f9;
			min-height: 100vh;
		}

		.brand {
			align-items: center;
			display: flex;
			gap: 10px;
			padding: 4px;
		}

		.mark {
			align-items: center;
			background: #19232d;
			border-radius: 6px;
			color: white;
			display: inline-flex;
			font-size: 14px;
			font-weight: 750;
			height: 30px;
			justify-content: center;
			width: 30px;
		}

		.brand strong {
			font-size: 15px;
			font-weight: 750;
		}

		.thread-label {
			display: block;
			overflow: hidden;
			text-overflow: ellipsis;
			white-space: nowrap;
		}

		.main {
			display: grid;
			grid-template-rows: auto 1fr auto;
			min-height: 100vh;
		}

		.topbar {
			align-items: center;
			background: rgb(246 247 249 / 88%);
			border-bottom: 1px solid #e1e5eb;
			display: flex;
			gap: 12px;
			min-height: 56px;
			padding: 0 22px;
		}

		.topbar h1 {
			font-size: 15px;
			font-weight: 700;
			margin: 0;
			overflow: hidden;
			text-overflow: ellipsis;
			white-space: nowrap;
		}

		.messages {
			margin: 0 auto;
			max-width: 820px;
			overflow-y: auto;
			padding: 28px 22px 18px;
			width: 100%;
		}

		.empty {
			align-content: center;
			display: grid;
			min-height: 100%;
			padding-bottom: 80px;
			text-align: center;
		}

		.empty h2 {
			font-size: 28px;
			font-weight: 750;
			margin: 0 0 10px;
		}

		.empty p {
			color: #667481;
			font-size: 15px;
			margin: 0;
		}

		.message {
			display: grid;
			margin: 0 0 22px;
		}

		.message.user {
			justify-items: end;
		}

		.bubble {
			border-radius: 8px;
			font-size: 15px;
			line-height: 1.55;
			max-width: min(680px, 100%);
			overflow-wrap: anywhere;
			padding: 11px 14px;
			white-space: pre-wrap;
		}

		.user .bubble {
			background: #dce9f5;
			color: #142434;
		}

		.agent .bubble,
		.tool .bubble {
			background: white;
			border: 1px solid #e1e5eb;
			color: #202a33;
		}

		.thinking {
			border-left: 3px solid #aeb8c3;
			color: #667481;
			font-size: 13px;
			margin-bottom: 8px;
			padding-left: 10px;
			white-space: pre-wrap;
		}

		.composer-wrap {
			background: linear-gradient(rgb(246 247 249 / 0), #f6f7f9 24px);
			padding: 18px 22px 24px;
		}

		.composer {
			align-items: end;
			background: white;
			border: 1px solid #cfd6df;
			border-radius: 8px;
			box-shadow: 0 10px 30px rgb(31 41 51 / 8%);
			display: grid;
			gap: 10px;
			grid-template-columns: minmax(0, 1fr) 42px;
			margin: 0 auto;
			max-width: 820px;
			padding: 10px;
		}

		textarea {
			color: #1f2933;
			font: inherit;
			line-height: 1.45;
			max-height: 180px;
			min-height: 42px;
			overflow-y: auto;
			padding: 10px;
			resize: none;
			width: 100%;
		}

		textarea::placeholder {
			color: #7d8994;
		}

		textarea:focus {
			outline: none;
		}

		.send {
			align-items: center;
			background: #1f6f5b;
			border-radius: 7px;
			color: white;
			cursor: pointer;
			display: flex;
			font-size: 18px;
			font-weight: 750;
			height: 42px;
			justify-content: center;
			width: 42px;
		}

		.send:disabled {
			background: #a9b4bd;
			cursor: default;
		}

		app-button.sidebar-action {
			width: 100%;
		}

		.error {
			color: #a33a3a;
			font-size: 13px;
			margin: 8px auto 0;
			max-width: 820px;
		}

		@media (max-width: 760px) {
			.main {
				min-height: 100vh;
			}
		}
	`;

	connectedCallback() {
		super.connectedCallback();
		void this.load();
	}

	disconnectedCallback() {
		this.closeEvents();
		super.disconnectedCallback();
	}

	updated(changed: Map<string, unknown>) {
		if (changed.has('threadId')) {
			void this.loadThread();
		}
	}

	render() {
		return html`
			<app-sidebar-provider>
				<app-sidebar collapsible="offcanvas">
					<app-sidebar-header>
						<div class="brand"><span class="mark">F</span><strong>Friday</strong></div>
						<app-button class="sidebar-action" variant="secondary" @click=${this.startNew}>New thread</app-button>
					</app-sidebar-header>
					<app-sidebar-content>
						<app-sidebar-group>
							<app-sidebar-group-label>Threads</app-sidebar-group-label>
							<app-sidebar-group-content>
								<app-sidebar-menu>${this.threads.map((thread) => this.renderThread(thread))}</app-sidebar-menu>
							</app-sidebar-group-content>
						</app-sidebar-group>
					</app-sidebar-content>
					<app-sidebar-footer>
						<app-button class="sidebar-action" variant="secondary" @click=${this.onLogout}>Log out</app-button>
					</app-sidebar-footer>
					<app-sidebar-rail></app-sidebar-rail>
				</app-sidebar>
				<app-sidebar-inset>
					<section class="main">
						<header class="topbar">
							<app-sidebar-trigger></app-sidebar-trigger>
							<h1>${this.currentTitle()}</h1>
						</header>
						<main class="messages">${this.renderMessages()}</main>
						<footer class="composer-wrap">
							<form class="composer" @submit=${this.onSubmit}>
								<textarea
									.value=${this.draft}
									placeholder=${this.threadId ? 'Message Friday' : 'Ask Friday anything'}
									rows="1"
									@input=${this.onDraft}
									@keydown=${this.onKeydown}
								></textarea>
								<button class="send" type="submit" ?disabled=${!this.canSend()}>↑</button>
							</form>
							${this.error ? html`<div class="error">${this.error}</div>` : nothing}
						</footer>
					</section>
				</app-sidebar-inset>
			</app-sidebar-provider>
		`;
	}

	private renderThread(thread: ThreadSummary) {
		return html`
			<app-sidebar-menu-item>
				<app-sidebar-menu-button
					href=${`/threads/${thread.id}`}
					tooltip=${thread.title}
					?active=${thread.id === this.threadId}
					@click=${this.onThreadClick}
				>
					<span class="thread-label">${thread.title}</span>
				</app-sidebar-menu-button>
			</app-sidebar-menu-item>
		`;
	}

	private renderMessages() {
		if (this.loading) {
			return html`<div class="empty"><p>Loading thread...</p></div>`;
		}

		if (!this.threadId || this.messages.length === 0) {
			return html`<div class="empty"><h2>What are we working on?</h2><p>Start a thread and Friday will keep the context here.</p></div>`;
		}

		return this.messages.map((message) => this.renderMessage(message));
	}

	private renderMessage(message: ViewMessage) {
		const role = message.role === 'agent' ? 'agent' : message.role;
		return html`
			<article class=${`message ${role}`}>
				<div class="bubble">
					${message.thinking ? html`<div class="thinking">${message.thinking}</div>` : nothing}
					${message.content || (message.pending ? 'Thinking...' : '')}
				</div>
			</article>
		`;
	}

	private async load() {
		try {
			this.threads = await listThreads();
			await this.loadThread();
		} catch (error) {
			this.handleError(error);
		}
	}

	private async loadThread() {
		this.closeEvents();
		this.pendingAssistant = '';
		this.error = '';

		if (!this.threadId) {
			this.messages = [];
			this.loading = false;
			this.running = false;
			return;
		}

		this.loading = true;
		try {
			this.messages = await listMessages(this.threadId);
			this.loading = false;
			this.openEvents(this.threadId);
		} catch (error) {
			this.loading = false;
			this.handleError(error);
		}
	}

	private openEvents(threadId: string) {
		this.events = new EventSource(`/api/threads/${threadId}/events`);
		this.events.onmessage = (event) => this.applyEvent(JSON.parse(event.data) as ThreadEvent);
		this.events.onerror = () => {
			this.error = 'Live updates disconnected.';
		};
	}

	private closeEvents() {
		this.events?.close();
		this.events = null;
	}

	private applyEvent(event: ThreadEvent) {
		if (event.thread_id !== this.threadId) {
			return;
		}

		if (event.kind.type === 'Snapshot') {
			this.running = event.kind.status === 'running';
			if (event.kind.in_progress?.content) {
				const last = this.messages[this.messages.length - 1];
				if (last?.role !== 'agent' || last.content !== event.kind.in_progress.content) {
					this.pendingAssistant = event.kind.in_progress.content;
					this.upsertPendingAssistant();
				}
			}
		}

		if (event.kind.type === 'RunStarted') {
			this.running = true;
		}

		if (event.kind.type === 'AgentDelta') {
			this.pendingAssistant += event.kind.content;
			this.upsertPendingAssistant();
		}

		if (event.kind.type === 'ThinkingDelta') {
			this.upsertPendingAssistant(event.kind.thinking);
		}

		if (event.kind.type === 'RunFinished' || event.kind.type === 'AgentMessageCommitted') {
			this.running = false;
			void this.refreshAfterRun();
		}

		if (event.kind.type === 'RunFailed') {
			this.running = false;
			this.error = event.kind.error;
		}
	}

	private upsertPendingAssistant(thinking?: string) {
		const messages = [...this.messages];
		const last = messages[messages.length - 1];

		if (last?.pending && last.role === 'agent') {
			messages[messages.length - 1] = {
				...last,
				content: this.pendingAssistant,
				thinking: thinking ? `${last.thinking ?? ''}${thinking}` : last.thinking
			};
		} else {
			messages.push({
				id: 'pending-agent',
				seq: Number.MAX_SAFE_INTEGER,
				role: 'agent',
				content: this.pendingAssistant,
				thinking: thinking ?? null,
				pending: true
			});
		}

		this.messages = messages;
	}

	private async refreshAfterRun() {
		if (!this.threadId) {
			return;
		}

		this.pendingAssistant = '';
		this.messages = await listMessages(this.threadId);
		this.threads = await listThreads();
	}

	private currentTitle() {
		return this.threads.find((thread) => thread.id === this.threadId)?.title ?? 'New thread';
	}

	private canSend() {
		return this.draft.trim().length > 0 && !this.running;
	}

	private onDraft(event: Event) {
		this.draft = (event.target as HTMLTextAreaElement).value;
	}

	private onKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter' && !event.shiftKey) {
			event.preventDefault();
			void this.submitDraft();
		}
	}

	private onSubmit(event: SubmitEvent) {
		event.preventDefault();
		void this.submitDraft();
	}

	private async submitDraft() {
		if (!this.canSend()) {
			return;
		}

		const content = this.draft.trim();
		this.draft = '';
		this.error = '';
		this.running = true;
		this.messages = [
			...this.messages,
			{id: `pending-user-${Date.now()}`, seq: Number.MAX_SAFE_INTEGER, role: 'user', content, thinking: null, pending: true}
		];

		try {
			if (this.threadId) {
				await sendMessage(this.threadId, content);
			} else {
				const response = await createThread(content);
				this.threadId = response.thread_id;
				history.pushState(null, '', `/threads/${response.thread_id}`);
				this.threads = await listThreads();
				await this.loadThread();
			}
		} catch (error) {
			this.running = false;
			this.handleError(error);
		}
	}

	private startNew() {
		this.threadId = '';
		this.messages = [];
		this.draft = '';
		this.running = false;
		history.pushState(null, '', '/threads');
	}

	private onThreadClick(event: MouseEvent) {
		event.preventDefault();
		const href = (event.currentTarget as HTMLElement).getAttribute('href') ?? '';
		const id = href.split('/').pop() ?? '';
		if (!id || id === this.threadId) {
			return;
		}
		this.threadId = id;
		history.pushState(null, '', `/threads/${id}`);
	}

	private async onLogout() {
		await logout();
		this.dispatchEvent(new CustomEvent('navigate', {bubbles: true, composed: true, detail: {path: '/login'}}));
	}

	private handleError(error: unknown) {
		if (error instanceof Error && error.message === '401') {
			this.dispatchEvent(new CustomEvent('navigate', {bubbles: true, composed: true, detail: {path: '/login'}}));
			return;
		}

		this.error = 'Request failed.';
	}
}

customElements.define('threads-page', ThreadsPage);
