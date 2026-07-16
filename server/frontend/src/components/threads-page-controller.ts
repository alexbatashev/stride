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
import {threadStream} from '../stores/thread-stream.js';
import {findTranscript, upsertTranscript} from './agent-transcripts.js';
import {threadView} from '../stores/thread-view.js';
import { bindSidebar } from "../pages/sidebar.js";
import { openThreadMenu, type ThreadMutation } from "../pages/thread-actions.js";
import { buildClientTimeline, buildSubagentTimeline } from "./chat-timeline.js";
import { buildChatTurns, buildTimeline } from "../shared/timeline.js";
import { sidePanel } from "../stores/side-panel.js";
import { loadSubagents } from "./subagent-data.js";
import type { PromptAttachment } from "../shared/prompt-attachment.js";

const SUBAGENT_STREAM_EVENTS = new Set([
	'message_started',
	'text_delta',
	'thinking_delta',
	'message_committed',
	'tool_call_started',
	'tool_call_progress',
	'tool_call_finished',
]);

function isSubagentStreamEvent(type: string): boolean {
	return SUBAGENT_STREAM_EVENTS.has(type);
}

function resetThreadStream(threadId: string): void {
	threadStream.threadId = threadId;
	threadStream.running = false;
	threadStream.messages = [];
	threadStream.toolCalls = [];
	threadStream.subagents = [];
	threadStream.agentTranscripts = [];
	threadStream.pendingApprovals = [];
	threadStream.pendingQuizzes = [];
}

type ViewMessage = ThreadMessage & { pending?: boolean; liveToolName?: string; liveToolDetail?: string; liveToolError?: boolean };
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

type PromptEl = HTMLElement & {
	disabled: boolean;
	running: boolean;
	placeholder: string;
	models: { value: string; label: string; description: string; vision: boolean }[];
	selectedModel: string;
	selectedModelLabel: string;
	selectedModelReasoningEffort: string;
};

type ApprovalEl = HTMLElement & { message: string };
type QuizEl = HTMLElement & { question: string; options: string[] };
type SidePanelEl = HTMLElement & { open: boolean; activeTab: string };
type MobilePanelEl = HTMLElement & { open: boolean; title: string };
type SubagentViewEl = HTMLElement & {
	active: boolean;
	agents: typeof threadStream.subagents;
	selectedKey: string;
	transcript: ReturnType<typeof buildSubagentTimeline>;
};

class ThreadsPageHydrator {
	private threadId: string;
	private threads: ThreadSummary[];
	private projects: ProjectSummary[];
	private currentProjectId: string | null = null;
	private messages: ViewMessage[] = [];
	private attachedFiles: PromptAttachment[] = [];
	private modelOptions: { value: string; label: string; description: string; vision: boolean }[] = [];
	private selectedModel = "";
	private selectedModelLabel = "Choose model";
	private selectedModelReasoningEffort = "";
	private modelPersistSeq = 0;
	private running: boolean;
	private error = "";
	private events: WebSocket | null = null;
	private pendingApproval: { id: string; message: string } | null = null;
	private pendingQuiz: PendingQuiz | null = null;
	private submittingQuizId: string | null = null;
	private refreshSeq = 0;
	private lastEventSeq = 0;
	private reconnectAttempts = 0;
	private readonly titleEl: HTMLElement;
	private readonly promptEl: PromptEl;
	private readonly approvalEl: ApprovalEl;
	private readonly quizEl: QuizEl;
	private readonly sidebarEl: SidebarEl;
	private readonly sidePanelEl: SidePanelEl;
	private readonly mobilePanelEl: MobilePanelEl;
	private readonly scope: ShadowRoot;
	private composerResizeObserver: ResizeObserver | null = null;
	private menuButtonEl: HTMLElement | null = null;

	constructor(private readonly root: HTMLElement) {
		if (!root.shadowRoot) throw new Error("Threads page is not hydrated");
		this.scope = root.shadowRoot;
		threadView.active = false;
		const initial = JSON.parse(root.dataset.argonServer ?? "{}") as {
			data?: {
				selectedModel?: string;
				running?: boolean;
				models?: { value: string; label: string; description: string; vision: boolean }[];
				selectedModelLabel?: string;
				selectedModelReasoningEffort?: string;
			};
		};
		this.threadId = root.dataset.threadId ?? "";
		resetThreadStream(this.threadId);
		this.selectedModel = initial.data?.selectedModel ?? "";
		this.modelOptions = initial.data?.models ?? [];
		this.selectedModelLabel = initial.data?.selectedModelLabel ?? "Choose model";
		this.selectedModelReasoningEffort = initial.data?.selectedModelReasoningEffort ?? "";
		this.running = initial.data?.running ?? false;
		this.titleEl = this.mustQuery("[data-current-title]");
		this.promptEl = this.mustQuery("[data-prompt]");
		this.approvalEl = this.mustQuery("[data-approval]");
		this.quizEl = this.mustQuery("[data-quiz]");
		this.sidebarEl = this.mustQuery("app-sidebar");
		this.sidePanelEl = this.mustQuery("[data-side-panel]");
		this.mobilePanelEl = this.mustQuery("[data-mobile-panel]");
		const composerLayer = this.scope.querySelector<HTMLElement>("[data-composer-layer]");
		const chatView = this.scope.querySelector<HTMLElement>("app-chat-view");
		if (composerLayer && chatView) {
			const syncComposerClearance = () => {
				const height = Math.ceil(composerLayer.getBoundingClientRect().height);
				chatView.style.setProperty("--composer-clearance", `${height + 16}px`);
			};
			this.composerResizeObserver = new ResizeObserver(syncComposerClearance);
			this.composerResizeObserver.observe(composerLayer);
			syncComposerClearance();
		}
		this.threads = this.readThreads();
		this.projects = this.sidebarEl.projects.map(({ id, title }) => ({ id, title }));
		this.currentProjectId = this.threadId
			? (this.threads.find((t) => t.id === this.threadId)?.project_id ?? null)
			: this.readProjectFromQuery();
		sidebar.activeThread = this.threadId;
		sidebar.activeProject = this.currentProjectId ?? "";

		this.bindEvents();
		void this.loadModels();
		this.syncComposer();

		if (this.threadId) {
			void this.hydrateMessages(this.threadId);
		} else {
			this.syncMessages();
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
			await loadSubagents(threadId);
			if (this.threadId !== threadId) return;
			this.syncSubagentViews();
			this.syncMessages();
		} catch (error) {
			this.handleError(error);
		}

		if (this.threadId === threadId) {
			this.openEvents(threadId);
		}
	}

	private mustQuery<T extends Element>(selector: string): T {
		const element = this.scope.querySelector<T>(selector);
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
		const openPanelButton = this.scope.querySelector<HTMLElement>('[data-action="side-panel-open"]');
		openPanelButton?.addEventListener("click", () => {
			sidePanel.open = true;
			openPanelButton.hidden = true;
		});
		this.scope.querySelector<HTMLElement>('[data-action="side-panel-close"]')?.addEventListener("click", () => {
			sidePanel.open = false;
			if (openPanelButton) openPanelButton.hidden = false;
		});
		this.menuButtonEl = this.scope.querySelector<HTMLElement>('[data-action="thread-menu"]');
		this.menuButtonEl?.addEventListener("click", () => this.openThreadActions());
		this.syncMenuButton();
		this.sidePanelEl.addEventListener("panel-close", () => { sidePanel.open = false; });
		this.sidePanelEl.addEventListener("tab-change", (event) => {
			this.setPanelTab((event as CustomEvent<{ value: "files" | "subagents" }>).detail.value);
		});
		this.scope.addEventListener("subagent-open", (event) => {
			const agentKey = (event as CustomEvent<{ agentKey: string }>).detail.agentKey;
			sidePanel.selectedSubagent = agentKey;
			if (window.matchMedia("(max-width: 767px)").matches) {
				this.openMobilePanel("subagents");
				const mobile = this.scope.querySelector<HTMLElement & { active: boolean; selectedKey: string }>("[data-mobile-subagents]");
				if (mobile) mobile.selectedKey = agentKey;
				return;
			}
			sidePanel.open = true;
			if (openPanelButton) openPanelButton.hidden = true;
			this.setPanelTab("subagents");
			const desktop = this.scope.querySelector<HTMLElement & { active: boolean; selectedKey: string }>("app-side-panel app-subagent-view");
			if (desktop) desktop.selectedKey = agentKey;
		});
		this.mobilePanelEl.addEventListener("close", () => { this.mobilePanelEl.open = false; });
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
		this.promptEl.addEventListener("attachment-remove", (event) => {
			const key = (event as CustomEvent<{ key: string }>).detail.key;
			this.attachedFiles = this.attachedFiles.filter((file) => file.key !== key);
			this.syncComposer();
		});
		this.approvalEl.addEventListener("approval-response", (event) =>
			void this.onApprovalResponse(event as CustomEvent<{ approved: boolean }>),
		);
		this.scope.addEventListener("quiz-response", (event) =>
			void this.onQuizResponse(event as CustomEvent<{ answer: string }>),
		);
		window.addEventListener("popstate", () => {
			window.location.href = window.location.pathname;
		});
	}

	private setPanelTab(tab: "files" | "subagents") {
		sidePanel.tab = tab;
		this.sidePanelEl.dispatchEvent(new CustomEvent("select-tab", { detail: { value: tab } }));
		const files = this.scope.querySelector<HTMLElement & { paneActive: boolean }>('app-side-panel app-file-explorer');
		const subagents = this.scope.querySelector<SubagentViewEl>('app-side-panel app-subagent-view');
		if (files) files.paneActive = tab === "files";
		if (subagents) subagents.active = tab === "subagents";
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
			this.pendingApproval = firstApproval
				? {
						id: firstApproval.approval_id,
						message: firstApproval.message,
					}
				: null;
			threadStream.pendingApprovals = event.kind.pending_approvals.map((approval) => ({id: approval.approval_id, toolCallId: '', message: approval.message}));
			threadStream.pendingQuizzes = event.kind.pending_quizzes.map((quiz) => ({id: quiz.quiz_id, questions: quiz.questions}));
			if (!this.pendingQuiz || !threadStream.pendingQuizzes.some((quiz) => quiz.id === this.pendingQuiz?.id)) {
				this.pendingQuiz = null;
				this.activateNextQuiz();
			}
			if (this.submittingQuizId && !threadStream.pendingQuizzes.some((quiz) => quiz.id === this.submittingQuizId)) {
				this.submittingQuizId = null;
			}
			this.syncComposer();
			if (event.kind.in_progress?.content) {
				const partial = event.kind.in_progress;
				threadStream.messages = [...threadStream.messages.filter((message) => message.id !== partial.message_id), {
					id: partial.message_id,
						seq: Number.MAX_SAFE_INTEGER,
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
					const message: ViewMessage = {id: partial.message_id, seq: Number.MAX_SAFE_INTEGER, created_at: Date.now(), role: 'agent', format: partial.format, content: partial.content, thinking: partial.thinking, tool_call_name: null, tool_call_id: null, tool_calls: [], pending: true};
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

		// Subagent message/tool events flow into per-agent transcript buckets, not
		// the root chat. The parent tool card still summarizes the child's output.
		if (event.agent_path.length > 0 && isSubagentStreamEvent(event.kind.type)) {
			this.applySubagentStreamEvent(event);
			this.updateSubagentToolCard(event.agent_path[event.agent_path.length - 1]);
			this.streamSubagentUpdate(event);
			this.syncSubagentViews();
			return;
		}

		if (event.kind.type === 'message_started') {
			if (event.kind.role === 'user') {
				const pending = this.messages.find((message) => message.pending && message.role === 'user');
				if (pending) {
					pending.id = event.kind.message_id;
					pending.seq = event.seq;
					pending.pending = false;
					this.syncMessages();
				}
				} else if (event.kind.role === 'assistant') {
					const messageId = event.kind.message_id;
					threadStream.messages = [
						...threadStream.messages.filter((message) => message.id !== messageId),
						{
							id: messageId,
						seq: event.seq,
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
						created_at: Date.now(),
						role: 'agent',
						format: 'markdown',
						content: '',
						thinking: null,
						tool_call_name: null,
						tool_call_id: null,
						tool_calls: [],
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
					seq: event.seq,
					createdAt: Date.now(),
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
			// The spawn event carries the parent's path; the child's own path
			// appends its id (matches `thread_agents.agent_path`).
			const agentPath = [...event.agent_path, agentId].join('/');
			threadStream.subagents = [
				...threadStream.subagents.filter((child) => child.id !== agentId),
				{
					id: agentId,
					name: event.kind.name,
					model: event.kind.model,
					result: '',
					finished: false,
					parentToolCallId: event.kind.parent_tool_call_id,
					agentPath,
					createdAt: Date.now(),
				},
			];
			this.updateSubagentToolCard(agentId);
			this.syncSubagentViews();
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
				this.syncSubagentViews();
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
			const questions = event.kind.questions;
			if (threadStream.pendingQuizzes.some((quiz) => quiz.id === quizId)) {
				threadStream.pendingQuizzes = threadStream.pendingQuizzes.map((quiz) => quiz.id === quizId ? {...quiz, questions} : quiz);
			} else {
				threadStream.pendingQuizzes = [...threadStream.pendingQuizzes, {id: quizId, questions}];
			}
			this.activateNextQuiz();
			this.syncComposer();
			return;
		}

		if (event.kind.type === 'quiz_answered') {
			this.completeQuiz(event.kind.quiz_id);
			return;
		}

		if (event.kind.type === 'run_finished' && event.agent_path.length === 0) {
			this.running = false;
			threadStream.running = false;
			this.pendingApproval = null;
			this.pendingQuiz = null;
			this.submittingQuizId = null;
			threadStream.pendingQuizzes = [];
			this.syncComposer();
			void this.refreshAfterRun();
		}

		if (event.kind.type === 'run_failed' && event.agent_path.length === 0) {
			this.running = false;
			threadStream.running = false;
			this.pendingApproval = null;
			this.pendingQuiz = null;
			this.submittingQuizId = null;
			threadStream.pendingQuizzes = [];
			this.syncComposer();
			this.setError(event.kind.error);
			void this.refreshAfterRun();
			return;
		}

		if (event.kind.type === 'run_cancelled' && event.agent_path.length === 0) {
			this.running = false;
			threadStream.running = false;
			this.pendingApproval = null;
			this.pendingQuiz = null;
			this.submittingQuizId = null;
			threadStream.pendingQuizzes = [];
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
				message = {id, seq: tool.seq, created_at: tool.createdAt, role: 'tool', format: 'markdown', content, thinking: null, tool_call_name: null, tool_call_id: tool.id, tool_calls: [], pending: tool.status === 'running', liveToolName: tool.name, liveToolDetail: tool.arguments, liveToolError: tool.isError};
			this.messages.push(message);
			this.appendMessage(message);
		} else {
				message.content = content;
				message.pending = tool.status === 'running';
				message.liveToolError = tool.isError;
			this.updateMessageElement(message);
		}
	}

	// Routes a live subagent message/tool event into its transcript bucket
	// (keyed by the agent's full slash-joined path).
	private applySubagentStreamEvent(event: ThreadEvent) {
		const key = event.agent_path.join('/');
		const source = findTranscript(key);
		const bucket = {
			key,
			messages: source ? [...source.messages] : [],
			toolCalls: source ? [...source.toolCalls] : [],
		};
		const kind = event.kind;
		if (kind.type === 'message_started' && kind.role === 'assistant') {
			bucket.messages = [
				...bucket.messages.filter((message) => message.id !== kind.message_id),
				{id: kind.message_id, seq: event.seq, content: '', thinking: '', agentPath: event.agent_path, committed: false},
			];
		} else if (kind.type === 'text_delta' || kind.type === 'thinking_delta') {
			const message = bucket.messages.find((candidate) => candidate.id === kind.message_id);
			if (message) {
				if (kind.type === 'text_delta') message.content += kind.delta;
				else message.thinking += kind.delta;
				bucket.messages = bucket.messages.map((candidate) => candidate.id === message.id ? {...message} : candidate);
			}
		} else if (kind.type === 'message_committed') {
			const message = bucket.messages.find((candidate) => candidate.id === kind.message_id);
			if (message) {
				message.committed = true;
				bucket.messages = bucket.messages.map((candidate) => candidate.id === message.id ? {...message} : candidate);
			}
		} else if (kind.type === 'tool_call_started') {
			bucket.toolCalls = [
				...bucket.toolCalls.filter((tool) => tool.id !== kind.tool_call_id),
				{id: kind.tool_call_id, seq: event.seq, createdAt: Date.now(), name: kind.name, arguments: kind.arguments, result: '', isError: false, status: 'running', agentPath: event.agent_path},
			];
		} else if (kind.type === 'tool_call_progress') {
			const tool = bucket.toolCalls.find((candidate) => candidate.id === kind.tool_call_id);
			if (tool) {
				tool.result = typeof kind.payload === 'string' ? kind.payload : JSON.stringify(kind.payload);
				bucket.toolCalls = bucket.toolCalls.map((candidate) => candidate.id === tool.id ? {...tool} : candidate);
			}
		} else if (kind.type === 'tool_call_finished') {
			const tool = bucket.toolCalls.find((candidate) => candidate.id === kind.tool_call_id);
			if (tool) {
				tool.result = kind.result;
				tool.isError = kind.is_error;
				tool.status = 'finished';
				bucket.toolCalls = bucket.toolCalls.map((candidate) => candidate.id === tool.id ? {...tool} : candidate);
			}
		}
		threadStream.agentTranscripts = upsertTranscript(bucket);
	}

	private updateSubagentToolCard(agentId: string) {
		const child = threadStream.subagents.find((candidate) => candidate.id === agentId);
		if (!child) return;
		const tool = threadStream.toolCalls.find((candidate) => candidate.id === child.parentToolCallId);
		if (!tool) return;
		const bucket = findTranscript(child.agentPath);
		const streamed = bucket ? bucket.messages.map((message) => message.content).join('') : '';
		tool.result = child.finished ? child.result : `${child.name} (${child.model})\n${streamed || 'Working…'}`;
		threadStream.toolCalls = threadStream.toolCalls.map((candidate) => candidate.id === tool.id ? {...tool} : candidate);
		this.upsertToolCard(tool.id);
	}

	private syncSubagentViews() {
		this.scope.querySelectorAll<SubagentViewEl>("app-subagent-view").forEach((view) => {
			view.agents = [...threadStream.subagents];
		});
		this.syncMessages();
	}

	private streamSubagentUpdate(event: ThreadEvent) {
		let itemId = "";
		if (event.kind.type === "text_delta" || event.kind.type === "thinking_delta" || event.kind.type === "message_committed") {
			itemId = event.kind.message_id;
		} else if (event.kind.type === "tool_call_started" || event.kind.type === "tool_call_progress" || event.kind.type === "tool_call_finished") {
			itemId = `tool:${event.kind.tool_call_id}`;
		}
		if (!itemId) return;
		this.scope.querySelectorAll<SubagentViewEl>("app-subagent-view").forEach((view) => {
			if (!view.active || !view.selectedKey) return;
			const item = buildSubagentTimeline(view.selectedKey).find((candidate) => candidate.id === itemId);
			if (item) view.dispatchEvent(new CustomEvent("transcript-update", { detail: { item } }));
		});
	}

	private createPendingQuiz(id: string, questions: QuizQuestion[]): PendingQuiz | null {
		if (questions.length === 0) {
			return null;
		}

		return { id, questions, index: 0, answers: [] };
	}

	private activateNextQuiz() {
		if (this.pendingQuiz) return;
		while (threadStream.pendingQuizzes.length > 0) {
			const next = threadStream.pendingQuizzes[0];
			this.pendingQuiz = this.createPendingQuiz(next.id, next.questions);
			if (this.pendingQuiz) return;
			threadStream.pendingQuizzes = threadStream.pendingQuizzes.slice(1);
		}
	}

	private completeQuiz(quizId: string) {
		threadStream.pendingQuizzes = threadStream.pendingQuizzes.filter((quiz) => quiz.id !== quizId);
		if (this.pendingQuiz?.id === quizId) {
			this.pendingQuiz = null;
		}
		if (this.submittingQuizId === quizId) {
			this.submittingQuizId = null;
		}
		this.activateNextQuiz();
		this.syncComposer();
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
		this.syncMessages();
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
				description: model.description,
				vision: model.vision,
			}));
			if (!this.selectedModel) {
				this.selectedModel =
					models.find((model) => model.key === "default")?.key ??
					models[0]?.key ??
					"";
			}
			const selected = models.find((model) => model.key === this.selectedModel) ?? models[0];
			this.selectedModelLabel = selected?.display_name ?? "Choose model";
			this.selectedModelReasoningEffort = selected?.reasoning_effort ?? "";
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

		const stagedUploads = this.attachedFiles
			.filter((file) => file.state === "done")
			.map((file) => file.id);
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
			tool_call_id: null,
			tool_calls: [],
			pending: true,
		};
		this.messages.push(message);
		this.appendMessage(message);
	}

	private async loadThread(threadId: string) {
		this.closeEvents();
		resetThreadStream(threadId);
		this.lastEventSeq = 0;
		this.pendingApproval = null;
		this.pendingQuiz = null;
		this.submittingQuizId = null;
		this.attachedFiles = [];
		this.setError("");

		try {
			this.messages = await listMessages(threadId);
			this.syncMessages();
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

	private syncMessages() {
		threadView.turns = buildChatTurns(buildTimeline(buildClientTimeline(this.messages)), this.running);
		threadView.active = true;
	}

	private appendMessage(_message: ViewMessage) {
		this.syncMessages();
	}

	private updateMessageElement(_message: ViewMessage) {
		this.syncMessages();
	}


	private syncTitle() {
		this.titleEl.textContent =
			this.threads.find((thread) => thread.id === this.threadId)?.title ??
			"New thread";
	}

	private syncComposer() {
		const quiz = this.pendingQuiz;
		const question = quiz ? quiz.questions[quiz.index] : undefined;
		threadView.running = this.running;
		threadView.placeholder = this.composerPlaceholder();
		threadView.models = this.modelOptions;
		threadView.selectedModel = this.selectedModel;
		threadView.selectedModelLabel = this.selectedModelLabel;
		threadView.selectedModelReasoningEffort = this.selectedModelReasoningEffort;
		threadView.attachments = this.attachedFiles.map((file) => ({ ...file }));
		threadView.approvalMessage = this.pendingApproval?.message ?? "";
		threadView.quizQuestion = question?.question ?? "";
		threadView.quizOptions = question?.options ?? [];
		threadView.quizSubmitting = this.submittingQuizId === quiz?.id;
		threadView.error = this.error;
		this.syncMenuButton();
	}

	private composerPlaceholder(): string {
		if (this.threadId) return "Message S.T.R.I.D.E.";
		const project = this.currentProjectId
			? this.projects.find((p) => p.id === this.currentProjectId)
			: undefined;
		return project ? `New thread in ${project.title}` : "Ask S.T.R.I.D.E. anything";
	}

	private syncMenuButton() {
		if (this.menuButtonEl) {
			this.menuButtonEl.hidden = !this.threadId || !window.matchMedia("(max-width: 767px)").matches;
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
			(tab) => this.openMobilePanel(tab),
		);
	}

	private openMobilePanel(tab: "files" | "subagents") {
		sidePanel.tab = tab;
		const files = this.scope.querySelector<HTMLElement>("[data-mobile-files]");
		const subagents = this.scope.querySelector<HTMLElement>("[data-mobile-subagents]");
		files?.toggleAttribute("hidden", tab !== "files");
		subagents?.toggleAttribute("hidden", tab !== "subagents");
		if (files) (files as HTMLElement & { paneActive: boolean }).paneActive = tab === "files";
		if (subagents) (subagents as HTMLElement & { active: boolean }).active = tab === "subagents";
		this.mobilePanelEl.title = tab === "files" ? "Files" : "Subagents";
		this.mobilePanelEl.open = true;
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
		if (!this.threadId || !this.pendingQuiz || this.submittingQuizId) return;

		const quiz = this.pendingQuiz;
		quiz.answers[quiz.index] = event.detail.answer;

		if (quiz.index + 1 < quiz.questions.length) {
			quiz.index += 1;
			this.syncComposer();
			return;
		}

		this.submittingQuizId = quiz.id;
		this.syncComposer();

		try {
			await answerQuiz(this.threadId, quiz.id, quiz.answers);
			this.completeQuiz(quiz.id);
		} catch {
			if (this.pendingQuiz?.id === quiz.id) {
				this.submittingQuizId = null;
				this.setError("Quiz response failed.");
			}
		}
	}

	private async onFilesAttach(event: CustomEvent<{ files: File[] }>) {
		const { files } = event.detail;
		const pending = files.map((file) => ({
			key: crypto.randomUUID(),
			id: "",
			name: file.name,
			size: file.size,
			state: "uploading" as const,
		}));
		this.attachedFiles.push(...pending);
		this.syncComposer();

		try {
			const staged = await stageUploads(files);
			for (const [index, file] of staged.entries()) {
				const attachment = this.attachedFiles.find((candidate) => candidate.key === pending[index]?.key);
				if (!attachment) continue;
				attachment.id = file.id;
				attachment.name = file.name;
				attachment.size = file.size;
				attachment.state = "done";
			}
			this.syncComposer();
		} catch {
			for (const file of pending) {
				const attachment = this.attachedFiles.find((candidate) => candidate.key === file.key);
				if (attachment) attachment.state = "error";
			}
			this.syncComposer();
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
		window.location.href = path === "/login" ? "/auth/login" : path;
	}
}

export function mountThreadsPage(root: HTMLElement): void {
	new ThreadsPageHydrator(root);
}
