import { ProjectSummary, listProjects } from "../api/projects.js";
import { listModels } from "../api/settings.js";
import {
	QuizQuestion,
	ThreadEvent,
	ThreadMessage,
	ThreadSummary,
	answerQuiz,
	cancelRun,
	createThread,
	listMessages,
	listThreads,
	resolveApproval,
	sendMessage,
	stageUploads,
	updateThreadModel,
} from "../api/threads.js";
import { sidebar } from "../stores/ui.js";
import {resetThreadStream, threadStream} from '../stores/thread-stream.js';
import { bindSidebar } from "./sidebar.js";
import { openThreadMenu, type ThreadMutation } from "./thread-actions.js";

type ViewMessage = ThreadMessage & { pending?: boolean; liveToolName?: string };
type PendingQuiz = {
	id: string;
	questions: QuizQuestion[];
	index: number;
	answers: string[];
};

type SidebarEl = HTMLElement & {
	projects: { id: string; title: string; threads: { id: string; title: string }[] }[];
	threads: { id: string; title: string }[];
};

type MessageEl = HTMLElement & {
	messageId: string;
	seq: number;
	role: string;
	kind: string;
	format: string;
	text: string;
	thinking: string;
	toolName: string;
};

type PromptEl = HTMLElement & {
	disabled: boolean;
	running: boolean;
	placeholder: string;
	models: { value: string; label: string }[];
	selectedModel: string;
};

type ApprovalEl = HTMLElement & { message: string };
type QuizEl = HTMLElement & { question: string; options: string[] };
type FileManagerEl = HTMLElement & { threadId: string; open: boolean };

const root = document.querySelector<HTMLElement>("#threads-page");

class ThreadsPageHydrator {
	private threadId: string;
	private threads: ThreadSummary[];
	private projects: ProjectSummary[];
	private currentProjectId: string | null = null;
	private messages: ViewMessage[] = [];
	private attachedFiles: { name: string; id: string }[] = [];
	private modelOptions: { value: string; label: string }[] = [];
	private selectedModel = "";
	private modelPersistSeq = 0;
	private running: boolean;
	private error = "";
	private events: WebSocket | null = null;
	private pendingApproval: { id: string; message: string } | null = null;
	private pendingQuiz: PendingQuiz | null = null;
	private refreshSeq = 0;
	private lastEventSeq = 0;
	private reconnectAttempts = 0;
	private readonly messagesEl: HTMLElement;
	private readonly scrollEl: HTMLElement;
	private readonly titleEl: HTMLElement;
	private readonly promptEl: PromptEl;
	private readonly approvalEl: ApprovalEl;
	private readonly quizEl: QuizEl;
	private readonly errorEl: HTMLElement;
	private readonly sidebarEl: SidebarEl;
	private readonly fileManagerEl: FileManagerEl;
	private menuButtonEl: HTMLElement | null = null;

	constructor(private readonly root: HTMLElement) {
		this.threadId = root.dataset.threadId ?? "";
		resetThreadStream(this.threadId);
		this.selectedModel = root.dataset.selectedModel ?? "";
		this.running = root.dataset.running === "true";
		this.messagesEl = this.mustQuery("[data-messages]");
		this.scrollEl = this.messagesEl.closest<HTMLElement>(".content") ?? this.messagesEl;
		this.titleEl = this.mustQuery("[data-current-title]");
		this.promptEl = this.mustQuery("[data-prompt]");
		this.errorEl = this.mustQuery("[data-error]");
		this.approvalEl = this.mustQuery("[data-approval]");
		this.quizEl = this.mustQuery("[data-quiz]");
		this.sidebarEl = this.mustQuery("app-sidebar");
		this.fileManagerEl = this.mustQuery("[data-file-manager]");
		this.threads = this.readThreads();
		this.projects = this.sidebarEl.projects.map(({ id, title }) => ({ id, title }));
		this.currentProjectId = this.threadId
			? (this.threads.find((t) => t.id === this.threadId)?.project_id ?? null)
			: this.readProjectFromQuery();

		this.bindEvents();
		void this.loadModels();
		this.syncComposer();

		if (this.threadId) {
			void this.hydrateMessages(this.threadId);
		}
	}

	// The server renders messages into the DOM, but the hydrator owns the
	// `messages` array that every re-render reads from. Load it before opening
	// the live stream so an incoming event never replaces the DOM with a list
	// that is missing the server-rendered history.
	private async hydrateMessages(threadId: string) {
		try {
			this.messages = await listMessages(threadId);
			if (this.threadId !== threadId) {
				return;
			}
			this.renderMessages();
			this.scrollInitial();
		} catch (error) {
			this.handleError(error);
		}

		if (this.threadId === threadId) {
			this.openEvents(threadId);
		}
	}

	private mustQuery<T extends Element>(selector: string): T {
		const element = this.root.querySelector<T>(selector);
		if (!element) {
			throw new Error(`Missing ${selector}`);
		}

		return element;
	}

	// The server hydrates the sidebar with grouped threads; flatten them back
	// into API-shaped summaries.
	private readThreads(): ThreadSummary[] {
		const grouped = this.sidebarEl.projects.flatMap((project) =>
			project.threads.map((thread) => ({ ...thread, project_id: project.id })),
		);
		const ungrouped = this.sidebarEl.threads.map((thread) => ({ ...thread, project_id: null }));
		return [...grouped, ...ungrouped];
	}

	// A new thread can be opened pre-bound to a project via `/threads?project=<id>`
	// (the sidebar's per-project "+" action). Ignore unknown ids.
	private readProjectFromQuery(): string | null {
		const id = new URLSearchParams(window.location.search).get("project");
		if (!id) return null;
		return this.projects.some((p) => p.id === id) ? id : null;
	}

	private bindEvents() {
		bindSidebar(this.sidebarEl);
		this.root
			.querySelectorAll<HTMLElement>('[data-action="files"]')
			.forEach((button) => button.addEventListener("click", () => this.toggleFiles()));
		this.menuButtonEl = this.root.querySelector<HTMLElement>('[data-action="thread-menu"]');
		this.menuButtonEl?.addEventListener("click", () => this.openThreadActions());
		this.syncMenuButton();
		this.fileManagerEl.addEventListener("files-close", () => {
			this.fileManagerEl.open = false;
		});
		this.promptEl.addEventListener("prompt-submit", (event) =>
			this.onPromptSubmit(event as CustomEvent<{ value: string; model: string | null }>),
		);
		this.promptEl.addEventListener("model-change", (event) => {
			const model = (event as CustomEvent<{ value: string }>).detail.value;
			if (model === this.selectedModel) {
				return;
			}
			this.selectedModel = model;
			this.syncComposer();
			void this.persistSelectedModel();
		});
		this.promptEl.addEventListener("prompt-stop", () => void this.onStop());
		this.promptEl.addEventListener("prompt-error", (event) =>
			this.setError((event as CustomEvent<{ message: string }>).detail.message),
		);
		this.promptEl.addEventListener("files-attach", (event) =>
			void this.onFilesAttach(event as CustomEvent<{ files: File[] }>),
		);
		this.approvalEl.addEventListener("approval-response", (event) =>
			void this.onApprovalResponse(event as CustomEvent<{ approved: boolean }>),
		);
		this.quizEl.addEventListener("quiz-response", (event) =>
			void this.onQuizResponse(event as CustomEvent<{ answer: string }>),
		);
		window.addEventListener("popstate", () => {
			window.location.href = window.location.pathname;
		});
	}

	private openEvents(threadId: string, after?: number) {
		this.closeEvents();
		const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
		const suffix = after != null ? `?after=${after}` : '';
		const socket = new WebSocket(`${protocol}//${location.host}/api/threads/${threadId}/events${suffix}`);
		this.events = socket;
		socket.onopen = () => {
			if (this.events !== socket) return;
			this.reconnectAttempts = 0;
		};
		socket.onmessage = (event) => {
			if (this.events !== socket) return;
			this.applyEvent(JSON.parse(event.data as string) as ThreadEvent);
		};
		socket.onerror = () => {
			if (this.events !== socket) return;
			this.setError("Live updates disconnected.");
		};
		socket.onclose = () => {
			if (this.events !== socket) return;
			this.events = null;
			const delay = this.reconnectDelay();
			this.reconnectAttempts += 1;
			setTimeout(() => {
				if (this.threadId === threadId) {
					this.openEvents(threadId, this.lastEventSeq > 0 ? this.lastEventSeq : undefined);
				}
			}, delay);
		};
	}

	// Exponential backoff with full jitter so many clients reconnecting after a
	// server blip do not stampede: base 1s, doubling per attempt, capped at 30s.
	private reconnectDelay(): number {
		const base = 1000;
		const cap = 30000;
		const backoff = Math.min(cap, base * 2 ** this.reconnectAttempts);
		return Math.random() * backoff;
	}

	private closeEvents() {
		const socket = this.events;
		this.events = null;
		socket?.close();
	}

	private applyEvent(event: ThreadEvent) {
		if (event.thread_id !== this.threadId) {
			return;
		}

		if (event.kind.type === "snapshot") {
			this.lastEventSeq = event.seq;
		} else {
			if (event.seq <= this.lastEventSeq) {
				return;
			}
			this.lastEventSeq = event.seq;
		}

		if (event.kind.type === "snapshot") {
			this.running = event.kind.status === "running";
			threadStream.running = this.running;
			const firstApproval = event.kind.pending_approvals[0];
			const firstQuiz = event.kind.pending_quizzes[0];
			this.pendingApproval = firstApproval
				? {
						id: firstApproval.approval_id,
						message: firstApproval.message,
					}
				: null;
			this.pendingQuiz = firstQuiz
				? this.createPendingQuiz(
						firstQuiz.quiz_id,
						firstQuiz.questions,
					)
				: null;
			threadStream.pendingApprovals = event.kind.pending_approvals.map((approval) => ({id: approval.approval_id, toolCallId: '', message: approval.message}));
			threadStream.pendingQuizzes = event.kind.pending_quizzes.map((quiz) => ({id: quiz.quiz_id, questions: quiz.questions}));
			this.syncComposer();
			if (event.kind.in_progress?.content) {
				const partial = event.kind.in_progress;
				threadStream.messages = [...threadStream.messages.filter((message) => message.id !== partial.message_id), {
					id: partial.message_id,
					content: partial.content,
					thinking: partial.thinking ?? '',
					agentPath: [],
					committed: false,
				}];
				const existing = this.messages.find((message) => message.id === partial.message_id);
				if (existing) {
					existing.content = partial.content;
					existing.thinking = partial.thinking;
					existing.pending = true;
					this.updateMessageElement(existing);
				} else {
					const message: ViewMessage = {id: partial.message_id, seq: Number.MAX_SAFE_INTEGER, role: 'agent', format: partial.format, content: partial.content, thinking: partial.thinking, tool_call_name: null, pending: true};
					this.messages.push(message);
					this.appendMessage(message);
				}
			}
			return;
		}

		if (event.kind.type === 'run_started') {
			this.running = true;
			threadStream.running = true;
			this.syncComposer();
			return;
		}

		if (event.kind.type === 'message_started') {
			if (event.kind.role === 'user') {
				const pending = this.messages.find((message) => message.pending && message.role === 'user');
				if (pending) {
					pending.id = event.kind.message_id;
					pending.seq = event.seq;
					pending.pending = false;
					this.renderMessages();
				}
				} else if (event.kind.role === 'assistant') {
					const messageId = event.kind.message_id;
					threadStream.messages = [
						...threadStream.messages.filter((message) => message.id !== messageId),
						{
							id: messageId,
						content: '',
						thinking: '',
						agentPath: event.agent_path,
						committed: false,
						},
					];
					if (event.agent_path.length === 0 && !this.messages.some((message) => message.id === messageId)) {
						const message: ViewMessage = {
							id: messageId,
						seq: event.seq,
						role: 'agent',
						format: 'markdown',
						content: '',
						thinking: null,
						tool_call_name: null,
						pending: true,
					};
					this.messages.push(message);
					this.appendMessage(message);
				}
			}
			return;
		}

		if (event.kind.type === 'text_delta' || event.kind.type === 'thinking_delta') {
			const messageId = event.kind.message_id;
			const live = threadStream.messages.find((message) => message.id === messageId);
			if (!live) return;
			if (event.kind.type === 'text_delta') live.content += event.kind.delta;
			else live.thinking += event.kind.delta;
			threadStream.messages = threadStream.messages.map((message) => message.id === live.id ? {...live} : message);
			if (event.agent_path.length === 0) {
				const message = this.messages.find((candidate) => candidate.id === live.id);
				if (message) {
					message.content = live.content;
					message.thinking = live.thinking || null;
					this.updateMessageElement(message);
				}
			} else {
				this.updateSubagentToolCard(event.agent_path[event.agent_path.length - 1]);
			}
			return;
		}

		if (event.kind.type === 'message_committed') {
			const messageId = event.kind.message_id;
			const live = threadStream.messages.find((message) => message.id === messageId);
			if (live) {
				live.committed = true;
				threadStream.messages = threadStream.messages.map((message) => message.id === live.id ? {...live} : message);
			}
			const message = this.messages.find((candidate) => candidate.id === messageId);
			if (message) message.pending = false;
			return;
		}

		if (event.kind.type === 'tool_call_started') {
			const toolCallId = event.kind.tool_call_id;
			this.running = true;
			threadStream.toolCalls = [
				...threadStream.toolCalls.filter((tool) => tool.id !== toolCallId),
				{
					id: toolCallId,
					name: event.kind.name,
					arguments: event.kind.arguments,
					result: '',
					isError: false,
					status: 'running',
					agentPath: event.agent_path,
				},
			];
			if (event.agent_path.length === 0) this.upsertToolCard(toolCallId);
			this.syncComposer();
			return;
		}

		if (event.kind.type === 'tool_call_progress') {
			const toolCallId = event.kind.tool_call_id;
			const tool = threadStream.toolCalls.find((candidate) => candidate.id === toolCallId);
			if (tool) {
				tool.result = typeof event.kind.payload === 'string' ? event.kind.payload : JSON.stringify(event.kind.payload);
				threadStream.toolCalls = threadStream.toolCalls.map((candidate) => candidate.id === tool.id ? {...tool} : candidate);
				this.upsertToolCard(tool.id);
			}
			return;
		}

		if (event.kind.type === 'tool_call_finished') {
			const toolCallId = event.kind.tool_call_id;
			const tool = threadStream.toolCalls.find((candidate) => candidate.id === toolCallId);
			if (tool) {
				tool.result = event.kind.result;
				tool.isError = event.kind.is_error;
				tool.status = 'finished';
				threadStream.toolCalls = threadStream.toolCalls.map((candidate) => candidate.id === tool.id ? {...tool} : candidate);
				this.upsertToolCard(tool.id);
			}
			this.syncComposer();
			return;
		}

		if (event.kind.type === 'agent_spawned') {
			const agentId = event.kind.agent_id;
			threadStream.subagents = [
				...threadStream.subagents.filter((child) => child.id !== agentId),
				{
					id: agentId,
					name: event.kind.name,
					model: event.kind.model,
					result: '',
					finished: false,
					parentToolCallId: event.kind.parent_tool_call_id,
				},
			];
			this.updateSubagentToolCard(agentId);
			return;
		}

		if (event.kind.type === 'agent_finished') {
			const agentId = event.kind.agent_id;
			const child = threadStream.subagents.find((candidate) => candidate.id === agentId);
			if (child) {
				child.result = event.kind.result;
				child.finished = true;
				threadStream.subagents = threadStream.subagents.map((candidate) => candidate.id === child.id ? {...child} : candidate);
				this.updateSubagentToolCard(agentId);
			}
			return;
		}

		if (event.kind.type === 'approval_requested') {
			const approvalId = event.kind.approval_id;
			this.running = true;
			threadStream.pendingApprovals = [...threadStream.pendingApprovals.filter((approval) => approval.id !== approvalId), {id: approvalId, toolCallId: event.kind.tool_call_id, message: event.kind.message}];
			this.pendingApproval = {id: approvalId, message: event.kind.message};
			this.syncComposer();
			return;
		}

		if (event.kind.type === 'approval_resolved') {
			const approvalId = event.kind.approval_id;
			threadStream.pendingApprovals = threadStream.pendingApprovals.filter((approval) => approval.id !== approvalId);
			if (this.pendingApproval?.id === approvalId) {
				const next = threadStream.pendingApprovals[0];
				this.pendingApproval = next ? {id: next.id, message: next.message} : null;
			}
			this.syncComposer();
			return;
		}

		if (event.kind.type === 'quiz_requested') {
			const quizId = event.kind.quiz_id;
			threadStream.pendingQuizzes = [...threadStream.pendingQuizzes.filter((quiz) => quiz.id !== quizId), {id: quizId, questions: event.kind.questions}];
			this.pendingQuiz = this.createPendingQuiz(quizId, event.kind.questions);
			this.syncComposer();
			return;
		}

		if (event.kind.type === 'quiz_answered') {
			const quizId = event.kind.quiz_id;
			threadStream.pendingQuizzes = threadStream.pendingQuizzes.filter((quiz) => quiz.id !== quizId);
			if (this.pendingQuiz?.id === quizId) {
				const next = threadStream.pendingQuizzes[0];
				this.pendingQuiz = next ? this.createPendingQuiz(next.id, next.questions) : null;
			}
			this.syncComposer();
			return;
		}

		if (event.kind.type === 'run_finished') {
			this.running = false;
			threadStream.running = false;
			this.pendingApproval = null;
			this.pendingQuiz = null;
			this.syncComposer();
			void this.refreshAfterRun();
		}

		if (event.kind.type === 'run_failed') {
			this.running = false;
			threadStream.running = false;
			this.pendingApproval = null;
			this.pendingQuiz = null;
			this.syncComposer();
			this.setError(event.kind.error);
			void this.refreshAfterRun();
			return;
		}

		if (event.kind.type === 'run_cancelled') {
			this.running = false;
			threadStream.running = false;
			this.pendingApproval = null;
			this.pendingQuiz = null;
			this.syncComposer();
			void this.refreshAfterRun();
		}
	}

	private upsertToolCard(toolCallId: string) {
		const tool = threadStream.toolCalls.find((candidate) => candidate.id === toolCallId);
		if (!tool) return;
		const id = `tool:${tool.id}`;
		const content = tool.status === 'running' && !tool.result ? `Running ${tool.name}…` : tool.result;
		let message = this.messages.find((candidate) => candidate.id === id);
		if (!message) {
			message = {id, seq: Number.MAX_SAFE_INTEGER, role: 'tool', format: 'markdown', content, thinking: null, tool_call_name: null, pending: tool.status === 'running', liveToolName: tool.name};
			this.messages.push(message);
			this.appendMessage(message);
		} else {
			message.content = content;
			message.pending = tool.status === 'running';
			this.updateMessageElement(message);
		}
	}

	private updateSubagentToolCard(agentId: string) {
		const child = threadStream.subagents.find((candidate) => candidate.id === agentId);
		if (!child) return;
		const tool = threadStream.toolCalls.find((candidate) => candidate.id === child.parentToolCallId);
		if (!tool) return;
		const streamed = threadStream.messages
			.filter((message) => message.agentPath.includes(agentId))
			.map((message) => message.content)
			.join('');
		tool.result = child.finished ? child.result : `${child.name} (${child.model})\n${streamed || 'Working…'}`;
		threadStream.toolCalls = threadStream.toolCalls.map((candidate) => candidate.id === tool.id ? {...tool} : candidate);
		this.upsertToolCard(tool.id);
	}

	private createPendingQuiz(id: string, questions: QuizQuestion[]): PendingQuiz | null {
		if (questions.length === 0) {
			return null;
		}

		return { id, questions, index: 0, answers: [] };
	}

	private async refreshAfterRun() {
		if (!this.threadId) {
			return;
		}

		const refreshSeq = ++this.refreshSeq;
		const [messages, threads, projects] = await Promise.all([
			listMessages(this.threadId),
			listThreads(),
			listProjects(),
		]);
		if (refreshSeq !== this.refreshSeq) {
			return;
		}

		this.messages = messages;
		this.renderMessages();
		this.threads = threads;
		this.projects = projects;
		this.renderSidebar();
		this.syncTitle();
	}

	private onPromptSubmit(event: CustomEvent<{ value: string; model: string | null }>) {
		void this.submitDraft(event.detail.value.trim(), event.detail.model ?? this.selectedModel);
	}

	private async loadModels() {
		try {
			const models = await listModels();
			this.modelOptions = models.map((model) => ({
				value: model.key,
				label: model.display_name,
			}));
			if (!this.selectedModel) {
				this.selectedModel =
					models.find((model) => model.key === "default")?.key ??
					models[0]?.key ??
					"";
			}
			this.syncComposer();
		} catch {
			this.modelOptions = [];
			this.syncComposer();
		}
	}

	private async persistSelectedModel() {
		if (!this.threadId) {
			return;
		}

		const seq = ++this.modelPersistSeq;
		const threadId = this.threadId;
		const model = this.selectedModel || null;
		try {
			await updateThreadModel(threadId, model);
		} catch {
			if (seq === this.modelPersistSeq && threadId === this.threadId) {
				this.setError("Model selection was not saved.");
			}
		}
	}

	private async submitDraft(content: string, model?: string) {
		if (!content || this.running) {
			return;
		}

		const stagedUploads = this.attachedFiles.map((f) => f.id);
		this.attachedFiles = [];
		this.error = "";
		this.running = true;
		this.syncComposer();
		this.appendPendingUser(content);

		try {
			if (this.threadId) {
				await sendMessage(this.threadId, content, stagedUploads, model || undefined);
			} else {
				const response = await createThread(
					content,
					this.currentProjectId ?? undefined,
					stagedUploads,
					model || undefined,
				);
				this.threadId = response.thread_id;
				this.root.dataset.threadId = this.threadId;
				this.fileManagerEl.threadId = this.threadId;
				history.pushState(null, "", `/threads/${response.thread_id}`);
				const [threads, projects] = await Promise.all([listThreads(), listProjects()]);
				this.threads = threads;
				this.projects = projects;
				this.renderSidebar();
				await this.loadThread(this.threadId);
			}
		} catch (error) {
			this.running = false;
			this.syncComposer();
			this.handleError(error);
		}
	}

	private appendPendingUser(content: string) {
		const message: ViewMessage = {
			id: `pending-user-${Date.now()}`,
			seq: Number.MAX_SAFE_INTEGER,
			role: "user",
			format: "markdown",
			content,
			thinking: null,
			tool_call_name: null,
			pending: true,
		};
		this.messages.push(message);
		this.appendMessage(message);
		this.scrollToBottom();
	}

	private async loadThread(threadId: string) {
		this.closeEvents();
		resetThreadStream(threadId);
		this.lastEventSeq = 0;
		this.pendingApproval = null;
		this.pendingQuiz = null;
		this.attachedFiles = [];
		this.setError("");

		try {
			this.messages = await listMessages(threadId);
			this.renderMessages();
			this.scrollInitial();
			this.syncTitle();
			this.openEvents(threadId);
		} catch (error) {
			this.handleError(error);
		}
	}

	// Reactive sidebar lists: the component reconciles keyed rows, so unchanged
	// projects and threads keep their DOM. Active selection lives in stores/ui.
	private renderSidebar() {
		this.sidebarEl.projects = this.projects.map((project) => ({
			id: project.id,
			title: project.title,
			threads: this.threads
				.filter((thread) => thread.project_id === project.id)
				.map(({ id, title }) => ({ id, title })),
		}));
		this.sidebarEl.threads = this.threads
			.filter((thread) => !thread.project_id || !this.projects.some((p) => p.id === thread.project_id))
			.map(({ id, title }) => ({ id, title }));
		sidebar.activeThread = this.threadId;
		sidebar.activeProject = this.threads.find((thread) => thread.id === this.threadId)?.project_id ?? "";
	}

	private renderMessages() {
		if (!this.threadId || this.messages.length === 0) {
			this.messagesEl.replaceChildren(this.createEmptyElement());
			return;
		}

		this.messagesEl.querySelector("[data-empty]")?.remove();
		const existing = new Map<string, MessageEl>();
		this.messagesEl.querySelectorAll<MessageEl>("app-message[data-message-id]").forEach((element) => {
			existing.set(element.dataset.messageId ?? "", element);
		});

		for (const message of this.messages) {
			const element = existing.get(message.id) ?? this.createMessageElement(message);
			this.syncMessageElement(element, message);
			this.messagesEl.append(element);
			existing.delete(message.id);
		}
		for (const stale of existing.values()) {
			stale.remove();
		}
	}

	private appendMessage(message: ViewMessage) {
		this.messagesEl.querySelector("[data-empty]")?.remove();
		const element = this.createMessageElement(message);
		this.messagesEl.append(element);
	}

	private updateMessageElement(message: ViewMessage) {
		const element = this.messagesEl.querySelector<MessageEl>(
			`app-message[data-message-id="${this.escapeSelectorValue(message.id)}"]`,
		);
		if (!element) {
			return;
		}

		this.syncMessageElement(element, message);
	}

	private syncMessageElement(element: MessageEl, message: ViewMessage) {
		const messageType = this.messageType(message);
		element.setAttribute("data-message-id", message.id);
		element.seq = message.seq;
		element.role = message.role;
		element.kind = messageType.type;
		element.format = message.format;
		element.toolName = messageType.toolName ?? "";
		element.thinking = message.thinking ? message.thinking : "";
		element.text = message.content
			? this.messageText(message, messageType.type)
			: message.pending ? "Thinking..." : "";
	}

	private scrollInitial() {
		requestAnimationFrame(() => {
			if (this.running) {
				this.scrollToLastMessageStart();
			} else {
				this.scrollToLastMessageEnd();
			}
		});
	}

	private scrollToLastMessageStart() {
		const last = this.lastMessageElement();
		if (!last) {
			return;
		}
		this.scrollEl.scrollTop = last.offsetTop - this.scrollEl.offsetTop;
	}

	private scrollToLastMessageEnd() {
		const last = this.lastMessageElement();
		if (!last) {
			return;
		}
		this.scrollEl.scrollTop = last.offsetTop - this.scrollEl.offsetTop + last.offsetHeight - this.scrollEl.clientHeight;
	}

	private scrollToBottom() {
		requestAnimationFrame(() => {
			this.scrollEl.scrollTop = this.scrollEl.scrollHeight;
		});
	}

	private lastMessageElement(): HTMLElement | null {
		return this.messagesEl.querySelector<HTMLElement>("app-message[data-message-id]:last-of-type");
	}

	private escapeSelectorValue(value: string): string {
		return typeof CSS !== "undefined" ? CSS.escape(value) : value.replace(/\"/g, '\\"');
	}

	private createMessageElement(message: ViewMessage) {
		const element = document.createElement("app-message") as MessageEl;
		this.syncMessageElement(element, message);
		return element;
	}

	private messageText(message: ThreadMessage, messageType: string): string {
		return message.content;
	}

	private createEmptyElement() {
		const empty = document.createElement("div");
		empty.className = "empty";
		empty.dataset.empty = "";

		const title = document.createElement("h2");
		title.textContent = "What are we working on?";

		const body = document.createElement("p");
		body.textContent = "Start a thread and S.T.R.I.D.E. will keep the context here.";

		empty.append(title, body);
		return empty;
	}

	private messageType(message: ThreadMessage): { type: string; toolName?: string } {
		const liveToolName = (message as ViewMessage).liveToolName;
		if (liveToolName) {
			return {type: 'tool_output', toolName: liveToolName};
		}
		if (message.tool_call_name) {
			return { type: "agent", toolName: message.tool_call_name };
		}
		if (message.role === "tool") {
			return { type: "tool_output", toolName: "Tool output" };
		}
		if (message.role === "system") {
			return { type: "agent" };
		}
		return { type: message.role };
	}

	private syncTitle() {
		this.titleEl.textContent =
			this.threads.find((thread) => thread.id === this.threadId)?.title ??
			"New thread";
	}

	private syncComposer() {
		this.promptEl.running = this.running;
		this.promptEl.placeholder = this.composerPlaceholder();
		this.promptEl.models = this.modelOptions;
		this.promptEl.selectedModel = this.selectedModel;
		const hasApproval = this.pendingApproval !== null;
		const hasQuiz = this.pendingQuiz !== null;
		this.promptEl.hidden = hasApproval || hasQuiz;
		this.approvalEl.hidden = !hasApproval;
		this.approvalEl.message = this.pendingApproval?.message ?? "";
		this.quizEl.hidden = !hasQuiz;
		const quiz = this.pendingQuiz;
		const question = quiz ? quiz.questions[quiz.index] : undefined;
		this.quizEl.question = question?.question ?? "";
		this.quizEl.options = question?.options ?? [];
		this.errorEl.textContent = this.error;
		this.fileManagerEl.threadId = this.threadId;
		this.syncMenuButton();
	}

	private composerPlaceholder(): string {
		if (this.threadId) return "Message S.T.R.I.D.E.";
		const project = this.currentProjectId
			? this.projects.find((p) => p.id === this.currentProjectId)
			: undefined;
		return project ? `New thread in ${project.title}` : "Ask S.T.R.I.D.E. anything";
	}

	private toggleFiles() {
		this.fileManagerEl.threadId = this.threadId;
		this.fileManagerEl.open = !this.fileManagerEl.open;
	}

	private syncMenuButton() {
		if (this.menuButtonEl) {
			this.menuButtonEl.style.display = this.threadId ? "inline-block" : "none";
		}
	}

	private openThreadActions() {
		if (!this.threadId || !this.menuButtonEl) return;
		const title =
			this.threads.find((thread) => thread.id === this.threadId)?.title ??
			this.titleEl.textContent ??
			"";
		openThreadMenu(
			this.menuButtonEl,
			{ id: this.threadId, title, archived: false },
			(mutation: ThreadMutation, thread) => {
				if (mutation === "delete" || mutation === "archive") {
					window.location.href = "/threads";
					return;
				}
				if (mutation === 'rename') {
					this.threads = this.threads.map((candidate) => candidate.id === thread.id ? {...candidate, title: thread.title} : candidate);
					this.syncTitle();
					this.renderSidebar();
				}
			},
		);
	}

	private async onStop() {
		if (!this.threadId || !this.running) return;
		try {
			await cancelRun(this.threadId);
		} catch {
			// Ignore errors — the RunCancelled event will update state
		}
	}

	private async onApprovalResponse(event: CustomEvent<{ approved: boolean }>) {
		if (!this.threadId || !this.pendingApproval) return;

		const approval = this.pendingApproval;
		this.pendingApproval = null;
		this.syncComposer();

		try {
			await resolveApproval(this.threadId, approval.id, event.detail.approved);
		} catch {
			this.pendingApproval = approval;
			this.setError("Approval response failed.");
		}
	}

	private async onQuizResponse(event: CustomEvent<{ answer: string }>) {
		if (!this.threadId || !this.pendingQuiz) return;

		const quiz = this.pendingQuiz;
		quiz.answers[quiz.index] = event.detail.answer;

		if (quiz.index + 1 < quiz.questions.length) {
			quiz.index += 1;
			this.syncComposer();
			return;
		}

		this.pendingQuiz = null;
		this.syncComposer();

		try {
			await answerQuiz(this.threadId, quiz.id, quiz.answers);
		} catch {
			this.pendingQuiz = quiz;
			this.setError("Quiz response failed.");
		}
	}

	private async onFilesAttach(event: CustomEvent<{ files: File[] }>) {
		const { files } = event.detail;
		const label = files.length === 1 ? files[0].name : `${files.length} files`;
		this.flash(`Uploading ${label}…`);

		try {
			const staged = await stageUploads(files);
			for (const f of staged) {
				this.attachedFiles.push({ name: f.name, id: f.id });
			}
			const count = this.attachedFiles.length;
			this.flash(`${count} file${count === 1 ? "" : "s"} attached`);
		} catch {
			this.flash("Upload failed.");
		}
	}

	private flash(message: string) {
		this.error = message;
		this.syncComposer();
		setTimeout(() => {
			if (this.error === message) {
				this.error = "";
				this.syncComposer();
			}
		}, 4000);
	}

	private setError(error: string) {
		this.error = error;
		this.syncComposer();
	}

	private handleError(error: unknown) {
		if (error instanceof Error && error.message === "401") {
			this.navigate("/login");
			return;
		}

		this.setError("Request failed.");
	}

	private navigate(path: string) {
		this.root.dispatchEvent(
			new CustomEvent("navigate", {
				bubbles: true,
				composed: true,
				detail: { path },
			}),
		);
	}
}

if (root) {
	new ThreadsPageHydrator(root);
}
