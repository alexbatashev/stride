import { ProjectSummary, listProjects } from "../api/projects.js";
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
	uploadFiles,
} from "../api/threads.js";
import { bindSidebar } from "./sidebar.js";

type ViewMessage = ThreadMessage & { pending?: boolean };
type PendingQuiz = {
	id: string;
	questions: QuizQuestion[];
	index: number;
	answers: string[];
};

type SidebarEl = HTMLElement & {
	projects: { id: string; title: string; threads: { id: string; title: string }[] }[];
	threads: { id: string; title: string }[];
	activeThread: string;
};

type MessageEl = HTMLElement & {
	messageId: string;
	seq: number;
	role: string;
	kind: string;
	text: string;
	thinking: string;
	toolName: string;
};

type PromptEl = HTMLElement & {
	disabled: boolean;
	running: boolean;
	placeholder: string;
};

type ApprovalEl = HTMLElement & { message: string };
type QuizEl = HTMLElement & { question: string; options: string[] };
type FileManagerEl = HTMLElement & { threadId: string; open: boolean };

// Argon text bindings insert markup verbatim; everything user-authored is
// escaped before it is handed to a component prop.
function esc(value: string): string {
	return value
		.replace(/&/g, "&amp;")
		.replace(/</g, "&lt;")
		.replace(/>/g, "&gt;")
		.replace(/"/g, "&quot;")
		.replace(/'/g, "&#39;");
}

const root = document.querySelector<HTMLElement>("#threads-page");

class ThreadsPageHydrator {
	private threadId: string;
	private threads: ThreadSummary[];
	private projects: ProjectSummary[];
	private currentProjectId: string | null = null;
	private messages: ViewMessage[] = [];
	private attachedFiles: { name: string; path: string }[] = [];
	private running: boolean;
	private error = "";
	private events: WebSocket | null = null;
	private pendingAssistant = "";
	private pendingApproval: { id: string; message: string } | null = null;
	private pendingQuiz: PendingQuiz | null = null;
	private refreshSeq = 0;
	private lastEventSeq = 0;
	private readonly messagesEl: HTMLElement;
	private readonly titleEl: HTMLElement;
	private readonly promptEl: PromptEl;
	private readonly approvalEl: ApprovalEl;
	private readonly quizEl: QuizEl;
	private readonly errorEl: HTMLElement;
	private readonly sidebarEl: SidebarEl;
	private readonly fileManagerEl: FileManagerEl;

	constructor(private readonly root: HTMLElement) {
		this.threadId = root.dataset.threadId ?? "";
		this.running = root.dataset.running === "true";
		this.messagesEl = this.mustQuery("[data-messages]");
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
		this.syncComposer();

		if (this.threadId) {
			this.openEvents(this.threadId);
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
		this.fileManagerEl.addEventListener("files-close", () => {
			this.fileManagerEl.open = false;
		});
		this.promptEl.addEventListener("prompt-submit", (event) =>
			this.onPromptSubmit(event as CustomEvent<{ value: string }>),
		);
		this.promptEl.addEventListener("prompt-stop", () => void this.onStop());
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
			setTimeout(() => {
				if (this.threadId === threadId) {
					this.openEvents(threadId, this.lastEventSeq > 0 ? this.lastEventSeq : undefined);
				}
			}, 2000);
		};
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

		// The snapshot resets the baseline; always apply it. Any live or replayed event whose seq we
		// have already applied is a duplicate (e.g. topic backlog overlap on reconnect) and is dropped
		// so a message is never rendered twice.
		if (event.kind.type === "Snapshot") {
			this.lastEventSeq = event.seq;
		} else {
			if (event.seq <= this.lastEventSeq) {
				return;
			}
			this.lastEventSeq = event.seq;
		}

		if (event.kind.type === "Snapshot") {
			this.running = event.kind.status === "running";
			this.pendingApproval = event.kind.pending_approval
				? {
						id: event.kind.pending_approval.approval_id,
						message: event.kind.pending_approval.message,
					}
				: null;
			this.pendingQuiz = event.kind.pending_quiz
				? this.createPendingQuiz(
						event.kind.pending_quiz.quiz_id,
						event.kind.pending_quiz.questions,
					)
				: null;
			this.syncComposer();
			if (event.kind.in_progress?.content) {
				const last = this.messages[this.messages.length - 1];
				if (
					last?.role !== "agent" ||
					last.content !== event.kind.in_progress.content
				) {
					this.pendingAssistant = event.kind.in_progress.content;
					this.upsertPendingAssistant();
				}
			}
		}

		if (event.kind.type === "RunStarted") {
			this.running = true;
			this.syncComposer();
		}

		if (event.kind.type === "UserMessageCommitted") {
			const pending = this.messages.find((message) => message.pending && message.role === "user");
			if (pending) {
				pending.id = event.kind.message_id;
				pending.seq = event.kind.seq;
				pending.pending = false;
				this.renderMessages();
			}
		}

		if (event.kind.type === "AgentDelta") {
			this.pendingAssistant += event.kind.content;
			this.upsertPendingAssistant();
		}

		if (event.kind.type === "ThinkingDelta") {
			this.upsertPendingAssistant(event.kind.thinking);
		}

		if (event.kind.type === "WaitingForApproval") {
			this.running = true;
			this.pendingApproval = {
				id: event.kind.approval_id,
				message: event.kind.message,
			};
			this.syncComposer();
		}

		if (event.kind.type === "ApprovalResolved") {
			if (this.pendingApproval?.id === event.kind.approval_id) {
				this.pendingApproval = null;
				this.syncComposer();
			}
		}

		if (event.kind.type === "WaitingForQuiz") {
			this.running = true;
			this.pendingQuiz = this.createPendingQuiz(
				event.kind.quiz_id,
				event.kind.questions,
			);
			this.syncComposer();
		}

		if (event.kind.type === "QuizAnswered") {
			if (this.pendingQuiz?.id === event.kind.quiz_id) {
				this.pendingQuiz = null;
				this.syncComposer();
			}
		}

		if (event.kind.type === "AgentMessageCommitted") {
			void this.refreshAfterRun();
		}

		if (event.kind.type === "ToolStarted") {
			this.running = true;
			this.syncComposer();
		}

		if (event.kind.type === "ToolFinished") {
			this.pendingApproval = null;
			this.pendingQuiz = null;
			this.syncComposer();
			void this.refreshAfterRun();
		}

		if (event.kind.type === "RunFinished") {
			this.running = false;
			this.pendingApproval = null;
			this.pendingQuiz = null;
			this.syncComposer();
			void this.refreshAfterRun();
		}

		if (event.kind.type === "RunFailed") {
			this.running = false;
			this.pendingApproval = null;
			this.pendingQuiz = null;
			this.syncComposer();
			this.setError(event.kind.error);
		}

		if (event.kind.type === "RunCancelled") {
			this.running = false;
			this.pendingApproval = null;
			this.pendingQuiz = null;
			this.pendingAssistant = "";
			this.syncComposer();
			void this.refreshAfterRun();
		}
	}

	private createPendingQuiz(id: string, questions: QuizQuestion[]): PendingQuiz | null {
		if (questions.length === 0) {
			return null;
		}

		return { id, questions, index: 0, answers: [] };
	}

	private upsertPendingAssistant(thinking?: string) {
		const last = this.messages[this.messages.length - 1];

		if (last?.role === "agent" && !last.tool_call_name) {
			last.pending = true;
			last.content = this.pendingAssistant;
			last.thinking = thinking ? `${last.thinking ?? ""}${thinking}` : last.thinking;
			this.updateMessageElement(last);
			return;
		}

		const message: ViewMessage = {
			id: "pending-agent",
			seq: Number.MAX_SAFE_INTEGER,
			role: "agent",
			content: this.pendingAssistant,
			thinking: thinking ?? null,
			tool_call_name: null,
			pending: true,
		};
		this.messages.push(message);
		this.appendMessage(message);
	}

	private async refreshAfterRun() {
		if (!this.threadId) {
			return;
		}

		const refreshSeq = ++this.refreshSeq;
		this.pendingAssistant = "";
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

	private onPromptSubmit(event: CustomEvent<{ value: string }>) {
		void this.submitDraft(event.detail.value.trim());
	}

	private async submitDraft(content: string) {
		if (!content || this.running) {
			return;
		}

		const filePaths = this.attachedFiles.map((f) => f.path);
		this.attachedFiles = [];
		this.error = "";
		this.running = true;
		this.syncComposer();
		this.appendPendingUser(content);

		try {
			if (this.threadId) {
				await sendMessage(this.threadId, content, filePaths);
			} else {
				const response = await createThread(content, this.currentProjectId ?? undefined, filePaths);
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
			content,
			thinking: null,
			tool_call_name: null,
			pending: true,
		};
		this.messages.push(message);
		this.appendMessage(message);
	}

	private async loadThread(threadId: string) {
		this.closeEvents();
		this.lastEventSeq = 0;
		this.pendingAssistant = "";
		this.pendingApproval = null;
		this.pendingQuiz = null;
		this.attachedFiles = [];
		this.setError("");

		try {
			this.messages = await listMessages(threadId);
			this.renderMessages();
			this.syncTitle();
			this.openEvents(threadId);
		} catch (error) {
			this.handleError(error);
		}
	}

	// Reactive sidebar props: the component reconciles its keyed lists, so
	// unchanged projects and threads keep their DOM.
	private renderSidebar() {
		this.sidebarEl.projects = this.projects.map((project) => ({
			id: project.id,
			title: esc(project.title),
			threads: this.threads
				.filter((thread) => thread.project_id === project.id)
				.map(({ id, title }) => ({ id, title: esc(title) })),
		}));
		this.sidebarEl.threads = this.threads
			.filter((thread) => !thread.project_id || !this.projects.some((p) => p.id === thread.project_id))
			.map(({ id, title }) => ({ id, title: esc(title) }));
		this.sidebarEl.activeThread = this.threadId;
	}

	private renderMessages() {
		if (!this.threadId || this.messages.length === 0) {
			this.messagesEl.replaceChildren(this.createEmptyElement());
			return;
		}

		this.messagesEl.replaceChildren(
			...this.messages.map((message) => this.createMessageElement(message)),
		);
	}

	private appendMessage(message: ViewMessage) {
		this.messagesEl.querySelector("[data-empty]")?.remove();
		this.messagesEl.append(this.createMessageElement(message));
		this.messagesEl.scrollTop = this.messagesEl.scrollHeight;
	}

	private updateMessageElement(message: ViewMessage) {
		const element = this.messagesEl.querySelector<MessageEl>(
			`app-message[data-message-id="${message.id}"]`,
		);
		if (!element) {
			return;
		}

		element.text = message.content ? esc(message.content) : message.pending ? "Thinking..." : "";
		if (message.thinking) {
			element.thinking = esc(message.thinking);
		}

		this.messagesEl.scrollTop = this.messagesEl.scrollHeight;
	}

	private createMessageElement(message: ViewMessage) {
		const element = document.createElement("app-message") as MessageEl;
		const messageType = this.messageType(message);
		element.setAttribute("data-message-id", message.id);
		element.seq = message.seq;
		element.role = message.role;
		element.kind = messageType.type;
		element.toolName = esc(messageType.toolName ?? "");
		element.thinking = message.thinking ? esc(message.thinking) : "";
		element.text = message.content ? esc(message.content) : message.pending ? "Thinking..." : "";
		return element;
	}

	private createEmptyElement() {
		const empty = document.createElement("div");
		empty.className = "empty";
		empty.dataset.empty = "";

		const title = document.createElement("h2");
		title.textContent = "What are we working on?";

		const body = document.createElement("p");
		body.textContent = "Start a thread and Friday will keep the context here.";

		empty.append(title, body);
		return empty;
	}

	private messageType(message: ThreadMessage): { type: string; toolName?: string } {
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
		const hasApproval = this.pendingApproval !== null;
		const hasQuiz = this.pendingQuiz !== null;
		this.promptEl.hidden = hasApproval || hasQuiz;
		this.approvalEl.hidden = !hasApproval;
		this.approvalEl.message = esc(this.pendingApproval?.message ?? "");
		this.quizEl.hidden = !hasQuiz;
		const quiz = this.pendingQuiz;
		const question = quiz ? quiz.questions[quiz.index] : undefined;
		this.quizEl.question = esc(question?.question ?? "");
		this.quizEl.options = (question?.options ?? []).map(esc);
		this.errorEl.textContent = this.error;
		this.fileManagerEl.threadId = this.threadId;
	}

	private composerPlaceholder(): string {
		if (this.threadId) return "Message Friday";
		const project = this.currentProjectId
			? this.projects.find((p) => p.id === this.currentProjectId)
			: undefined;
		return project ? `New thread in ${project.title}` : "Ask Friday anything";
	}

	private toggleFiles() {
		this.fileManagerEl.threadId = this.threadId;
		this.fileManagerEl.open = !this.fileManagerEl.open;
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
		if (!this.threadId) {
			this.flash("Start a thread before uploading files.");
			return;
		}

		const { files } = event.detail;
		const label = files.length === 1 ? files[0].name : `${files.length} files`;
		this.flash(`Uploading ${label}…`);

		try {
			const uploaded = await uploadFiles(this.threadId, files);
			for (const f of uploaded) {
				this.attachedFiles.push({ name: f.name, path: f.path });
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
