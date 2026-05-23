import { logout } from "../api/auth.js";
import {
	ProjectSummary,
	createProject,
	deleteProject,
	listProjects,
	renameProject,
} from "../api/projects.js";
import {
	ThreadEvent,
	ThreadMessage,
	ThreadSummary,
	cancelRun,
	createThread,
	listMessages,
	listThreads,
	sendMessage,
} from "../api/threads.js";
import "../components/app-button.js";
import "../components/app-message.js";
import "../components/app-prompt-input.js";
import "../components/app-sidebar.js";

type ViewMessage = ThreadMessage & { pending?: boolean };

const root = document.querySelector<HTMLElement>("#threads-page");

class ThreadsPageHydrator {
	private threadId: string;
	private threads: ThreadSummary[];
	private projects: ProjectSummary[];
	private currentProjectId: string | null = null;
	private messages: ViewMessage[] = [];
	private draft = "";
	private running: boolean;
	private error = "";
	private events: WebSocket | null = null;
	private pendingAssistant = "";
	private refreshSeq = 0;
	private lastEventSeq = 0;
	private readonly messagesEl: HTMLElement;
	private readonly titleEl: HTMLElement;
	private readonly promptEl: HTMLElement & {
		value: string;
		disabled: boolean;
		running: boolean;
		placeholder: string;
	};
	private readonly errorEl: HTMLElement;
	private readonly sidebarListEl: HTMLElement;

	constructor(private readonly root: HTMLElement) {
		this.threadId = root.dataset.threadId ?? "";
		this.running = root.dataset.running === "true";
		this.messagesEl = this.mustQuery("[data-messages]");
		this.titleEl = this.mustQuery("[data-current-title]");
		this.promptEl = this.mustQuery("[data-prompt]");
		this.errorEl = this.mustQuery("[data-error]");
		this.sidebarListEl = this.mustQuery("[data-sidebar-list]");
		this.threads = this.readThreads();
		this.projects = this.readProjects();
		this.currentProjectId = this.threads.find((t) => t.id === this.threadId)?.project_id ?? null;

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

	private bindEvents() {
		this.root
			.querySelector<HTMLElement>('[data-action="new-thread"]')
			?.addEventListener("click", () => this.startNew());
		this.root
			.querySelector<HTMLElement>('[data-action="logout"]')
			?.addEventListener("click", () => void this.onLogout());
		this.root
			.querySelector<HTMLElement>('[data-action="new-project"]')
			?.addEventListener("click", () => void this.onNewProject());
		this.sidebarListEl.addEventListener("click", (event) =>
			this.onSidebarClick(event),
		);
		this.promptEl.addEventListener("value-change", (event) =>
			this.onDraft(event as CustomEvent<{ value: string }>),
		);
		this.promptEl.addEventListener("prompt-submit", (event) =>
			this.onPromptSubmit(event as CustomEvent<{ value: string }>),
		);
		this.promptEl.addEventListener("prompt-stop", () => void this.onStop());
		window.addEventListener("popstate", () => {
			window.location.href = window.location.pathname;
		});
	}

	private readThreads(): ThreadSummary[] {
		return Array.from(
			this.sidebarListEl.querySelectorAll<HTMLElement>("[data-thread-id]"),
		).map((element) => ({
			id: element.dataset.threadId ?? "",
			title: element.textContent?.trim() ?? "Untitled",
			project_id: element.dataset.projectId ?? null,
		}));
	}

	private readProjects(): ProjectSummary[] {
		return Array.from(
			this.sidebarListEl.querySelectorAll<HTMLElement>("[data-project-id]"),
		).map((element) => ({
			id: element.getAttribute("data-project-id") ?? "",
			title: element.getAttribute("title") ?? element.textContent?.trim() ?? "",
		}));
	}

	private readMessages(): ViewMessage[] {
		return Array.from(
			this.messagesEl.querySelectorAll<HTMLElement>("app-message[data-role]"),
		).map((element) => {
			const toolNames = element.getAttribute("tool_names");

			return {
				id: element.dataset.messageId ?? "",
				seq: Number(element.dataset.seq ?? 0),
				role: this.readRole(element.dataset.role),
				content:
					element.querySelector<HTMLElement>("[data-content]")?.textContent ?? "",
				thinking:
					element.querySelector<HTMLElement>("[data-thinking]")?.textContent ??
					null,
				tool_call_name: toolNames
					?.split(",")
					.map((name) => name.trim())
					.find(Boolean) ?? null,
			};
		});
	}

	private readRole(role: string | undefined): ThreadMessage["role"] {
		if (
			role === "system" ||
			role === "agent" ||
			role === "user" ||
			role === "tool"
		) {
			return role;
		}

		return "agent";
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

		this.lastEventSeq = Math.max(this.lastEventSeq, event.seq);

		if (event.kind.type === "Snapshot") {
			this.running = event.kind.status === "running";
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

		if (event.kind.type === "AgentDelta") {
			this.pendingAssistant += event.kind.content;
			this.upsertPendingAssistant();
		}

		if (event.kind.type === "ThinkingDelta") {
			this.upsertPendingAssistant(event.kind.thinking);
		}

		if (event.kind.type === "AgentMessageCommitted") {
			void this.refreshAfterRun();
		}

		if (event.kind.type === "ToolStarted") {
			this.running = true;
			this.syncComposer();
		}

		if (event.kind.type === "ToolFinished") {
			void this.refreshAfterRun();
		}

		if (event.kind.type === "RunFinished") {
			this.running = false;
			this.syncComposer();
			void this.refreshAfterRun();
		}

		if (event.kind.type === "RunFailed") {
			this.running = false;
			this.syncComposer();
			this.setError(event.kind.error);
		}

		if (event.kind.type === "RunCancelled") {
			this.running = false;
			this.pendingAssistant = "";
			this.syncComposer();
			void this.refreshAfterRun();
		}
	}

	private upsertPendingAssistant(thinking?: string) {
		const last = this.messages[this.messages.length - 1];

		if (last?.pending && last.role === "agent") {
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

	private canSend() {
		return this.draft.trim().length > 0 && !this.running;
	}

	private onDraft(event: CustomEvent<{ value: string }>) {
		this.draft = event.detail.value;
	}

	private onPromptSubmit(event: CustomEvent<{ value: string }>) {
		this.draft = event.detail.value;
		void this.submitDraft();
	}

	private async submitDraft() {
		if (!this.canSend()) {
			return;
		}

		const content = this.draft.trim();
		this.draft = "";
		this.error = "";
		this.running = true;
		this.syncComposer();
		this.appendPendingUser(content);

		try {
			if (this.threadId) {
				await sendMessage(this.threadId, content);
			} else {
				const response = await createThread(content, this.currentProjectId ?? undefined);
				this.threadId = response.thread_id;
				this.root.dataset.threadId = this.threadId;
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

	private startNew() {
		this.closeEvents();
		this.threadId = "";
		this.currentProjectId = null;
		this.root.dataset.threadId = "";
		this.messages = [];
		this.draft = "";
		this.running = false;
		this.pendingAssistant = "";
		this.lastEventSeq = 0;
		this.renderMessages();
		this.renderSidebar();
		this.syncTitle();
		this.syncComposer();
		history.pushState(null, "", "/threads");
	}

	private onSidebarClick(event: Event) {
		const item = (event.target as Element).closest<HTMLElement>(
			"[data-thread-id]",
		);
		if (!item) {
			return;
		}

		event.preventDefault();
		const id = item.dataset.threadId ?? "";
		if (!id || id === this.threadId) {
			return;
		}

		this.threadId = id;
		this.root.dataset.threadId = id;
		this.currentProjectId = this.threads.find((t) => t.id === id)?.project_id ?? null;
		this.renderSidebar();
		history.pushState(null, "", `/threads/${id}`);
		void this.loadThread(id);
	}

	private async onLogout() {
		await logout();
		this.navigate("/login");
	}

	private async onNewProject() {
		const title = this.promptProjectTitle();
		if (!title) return;

		try {
			const project = await createProject(title);
			this.projects = [project, ...this.projects];
			this.renderSidebar();
		} catch {
			this.setError("Failed to create project.");
		}
	}

	private promptProjectTitle(existing?: string): string | null {
		const input = window.prompt("Project name:", existing ?? "");
		if (input === null) return null;
		const trimmed = input.trim();
		return trimmed.length > 0 ? trimmed : null;
	}

	private async onRenameProject(projectId: string, currentTitle: string) {
		const title = this.promptProjectTitle(currentTitle);
		if (!title || title === currentTitle) return;

		try {
			const updated = await renameProject(projectId, title);
			const idx = this.projects.findIndex((p) => p.id === projectId);
			if (idx >= 0) this.projects[idx] = updated;
			this.renderSidebar();
		} catch {
			this.setError("Failed to rename project.");
		}
	}

	private async onDeleteProject(projectId: string) {
		if (!window.confirm("Delete this project? Threads will be kept but unlinked.")) return;

		try {
			await deleteProject(projectId);
			this.projects = this.projects.filter((p) => p.id !== projectId);
			this.threads = this.threads.map((t) =>
				t.project_id === projectId ? { ...t, project_id: null } : t,
			);
			if (this.currentProjectId === projectId) {
				this.currentProjectId = null;
			}
			this.renderSidebar();
		} catch {
			this.setError("Failed to delete project.");
		}
	}

	private renderSidebar() {
		const grouped = new Map<string, ThreadSummary[]>();
		const ungrouped: ThreadSummary[] = [];

		for (const project of this.projects) {
			grouped.set(project.id, []);
		}
		for (const thread of this.threads) {
			if (thread.project_id && grouped.has(thread.project_id)) {
				grouped.get(thread.project_id)!.push(thread);
			} else {
				ungrouped.push(thread);
			}
		}

		const children: HTMLElement[] = [];

		for (const project of this.projects) {
			const group = this.createProjectGroup(project, grouped.get(project.id) ?? []);
			children.push(group);
		}

		if (ungrouped.length > 0) {
			const group = this.createThreadGroup("Threads", ungrouped);
			children.push(group);
		}

		this.sidebarListEl.replaceChildren(...children);
	}

	private createProjectGroup(project: ProjectSummary, threads: ThreadSummary[]): HTMLElement {
		const group = document.createElement("app-sidebar-group") as HTMLElement & {
			title: string;
		};
		group.title = project.title;
		group.setAttribute("data-project-id", project.id);

		const actions = document.createElement("span");
		actions.className = "project-actions";

		const renameBtn = document.createElement("button");
		renameBtn.type = "button";
		renameBtn.className = "project-action-btn";
		renameBtn.title = "Rename";
		renameBtn.textContent = "✎";
		renameBtn.addEventListener("click", (e) => {
			e.stopPropagation();
			void this.onRenameProject(project.id, project.title);
		});

		const deleteBtn = document.createElement("button");
		deleteBtn.type = "button";
		deleteBtn.className = "project-action-btn";
		deleteBtn.title = "Delete";
		deleteBtn.textContent = "✕";
		deleteBtn.addEventListener("click", (e) => {
			e.stopPropagation();
			void this.onDeleteProject(project.id);
		});

		actions.append(renameBtn, deleteBtn);
		group.append(actions);

		for (const thread of threads) {
			group.append(this.createThreadElement(thread));
		}

		return group;
	}

	private createThreadGroup(title: string, threads: ThreadSummary[]): HTMLElement {
		const group = document.createElement("app-sidebar-group") as HTMLElement & {
			title: string;
		};
		group.title = title;

		for (const thread of threads) {
			group.append(this.createThreadElement(thread));
		}

		return group;
	}

	private createThreadElement(thread: ThreadSummary) {
		const item = document.createElement("app-sidebar-group-item") as HTMLElement & {
			target: string;
			active: boolean;
		};
		item.setAttribute("target", `/threads/${thread.id}`);
		item.dataset.threadId = thread.id;
		if (thread.project_id) {
			item.dataset.projectId = thread.project_id;
		}
		item.active = thread.id === this.threadId;
		item.toggleAttribute("active", item.active);

		const label = document.createElement("span");
		label.className = "thread-label";
		label.textContent = thread.title;
		item.append(label);
		return item;
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
		const element = this.messagesEl.querySelector<HTMLElement & { text: string; with_thinking: boolean }>(
			`app-message[data-message-id="${message.id}"]`,
		);
		if (!element) {
			return;
		}

		element.text = message.content || (message.pending ? "Thinking..." : "");

		if (message.thinking) {
			let thinking = element.querySelector<HTMLElement>("[data-thinking]");
			if (!thinking) {
				thinking = document.createElement("span");
				thinking.slot = "thinking";
				thinking.dataset.thinking = "";
				element.prepend(thinking);
			}
			thinking.textContent = message.thinking;
			element.with_thinking = true;
		}

		this.messagesEl.scrollTop = this.messagesEl.scrollHeight;
	}

	private createMessageElement(message: ViewMessage) {
		const element = document.createElement("app-message") as HTMLElement & {
			message_id: string;
			type: string;
			tool_names: string[];
			with_thinking: boolean;
			text: string;
		};
		const messageType = this.messageType(message);
		element.message_id = message.id;
		element.type = messageType.type;
		element.dataset.messageId = message.id;
		element.dataset.seq = String(message.seq);
		element.dataset.role = message.role;
		if (messageType.toolName) {
			element.tool_names = [messageType.toolName];
			element.setAttribute("tool_names", messageType.toolName);
		}
		if (message.thinking) {
			element.with_thinking = true;
			const thinking = document.createElement("span");
			thinking.slot = "thinking";
			thinking.dataset.thinking = "";
			thinking.textContent = message.thinking;
			element.append(thinking);
		}

		if (messageType.type === "tool_output") {
			const content = document.createElement("span");
			content.dataset.content = "";
			content.textContent = message.content || "";
			element.append(content);
		} else {
			element.text = message.content || (message.pending ? "Thinking..." : "");
		}

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

	private messageType(message: ThreadMessage) {
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
		this.promptEl.value = this.draft;
		this.promptEl.running = this.running;
		this.promptEl.placeholder = this.threadId ? "Message Friday" : "Ask Friday anything";
		this.errorEl.textContent = this.error;
	}

	private async onStop() {
		if (!this.threadId || !this.running) return;
		try {
			await cancelRun(this.threadId);
		} catch {
			// Ignore errors — the RunCancelled event will update state
		}
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
