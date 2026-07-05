import { ProjectSummary, listProjects } from "../api/projects.js";
import { listModels } from "../api/settings.js";
import {
	QuizQuestion,
	RunInfo,
	RunStatus,
	ThreadEvent,
	ThreadMessage,
	ThreadSummary,
	answerQuiz,
	cancelRun,
	createThread,
	fetchRuns,
	listMessages,
	listThreads,
	resolveApproval,
	sendMessage,
	stageUploads,
} from "../api/threads.js";
import { bindSidebar } from "./sidebar.js";
import { openThreadMenu, type ThreadMutation } from "./thread-actions.js";

type ViewMessage = ThreadMessage & { pending?: boolean };
type PendingQuiz = {
	id: string;
	questions: QuizQuestion[];
	index: number;
	answers: string[];
};

type ToolCallState = {
	toolCallId: string;
	name: string;
	status: RunStatus;
	background: boolean;
	startedAtMs: number;
	finishedAtMs: number;
	callSeq: number;
	assistantMessageId: string | null;
	content: string;
	format: string;
};

type RunState = {
	runId: string;
	status: RunStatus;
	startedAtMs: number;
	finishedAtMs: number;
	userMessageId: string | null;
	finalMessageId: string | null;
	toolCalls: Map<string, ToolCallState>;
};

type OverrideStore = {
	runs: Record<string, boolean>;
	tools: Record<string, boolean>;
};

type SidebarEl = HTMLElement & {
	projects: { id: string; title: string; threads: { id: string; title: string }[] }[];
	threads: { id: string; title: string }[];
	activeThread: string;
};

type MessageEl = HTMLElement & {
	messageId: string;
	itemId: string;
	seq: number;
	role: string;
	source: string;
	kind: string;
	format: string;
	text: string;
	thinking: string;
	toolName: string;
};

type RunGroupEl = HTMLElement & {
	runId: string;
	status: string;
	startedAtMs: number;
	finishedAtMs: number;
	open: boolean;
};

type ToolCallEl = HTMLElement & {
	toolCallId: string;
	name: string;
	status: string;
	background: boolean;
	startedAtMs: number;
	finishedAtMs: number;
	open: boolean;
	content: string;
	format: string;
	resultText: string;
};

type AutoMarkdownEl = HTMLElement & {
	text: string;
	format: string;
};

type SpoilerEl = HTMLElement & {
	title: string;
	content: string;
	format: string;
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

// A run's items sort into the timeline by (assistant message seq, call_seq).
// Tool calls anchor to the point where they were issued, not when they finished.
export type TimelineEntry =
	| { kind: "message"; message: ViewMessage }
	| { kind: "group"; runId: string };

// The message a run currently treats as its final response: the authoritative
// finalMessageId when known, else — while the run has no committed final — the
// newest agent message provided no tool call was issued at or after it. That
// "candidate final" renders below the group; once a later tool starts it stops
// qualifying and folds back into the group as an intermediate note.
export function candidateFinalId(run: RunState, messages: ViewMessage[]): string | null {
	if (run.finalMessageId) return run.finalMessageId;

	let best: ViewMessage | null = null;
	for (const message of messages) {
		if (message.run_id !== run.runId) continue;
		if (message.id === run.userMessageId) continue;
		if (message.role !== "agent" || message.tool_call_name) continue;
		if (message.source === "tool_wakeup") continue;
		if (!best || message.seq >= best.seq) best = message;
	}
	if (!best) return null;

	for (const call of run.toolCalls.values()) {
		const anchor = call.assistantMessageId
			? messages.find((m) => m.id === call.assistantMessageId)?.seq
			: undefined;
		if (anchor !== undefined && anchor >= best.seq) return null;
	}
	return best.id;
}

// Computes the top-level timeline order. User (human) messages, legacy messages
// (run_id null), and run final responses render flat; each run gets one group
// slotted right after its triggering user message. Runs order by start time.
export function buildTimeline(messages: ViewMessage[], runs: Map<string, RunState>): TimelineEntry[] {
	const finalIds = new Set<string>();
	const userTriggerIds = new Set<string>();
	for (const run of runs.values()) {
		const finalId = candidateFinalId(run, messages);
		if (finalId) finalIds.add(finalId);
		if (run.userMessageId) userTriggerIds.add(run.userMessageId);
	}

	// Group insertion anchor: the seq of the run's triggering user message when
	// known, else the smallest seq among the run's messages, else run start.
	const anchorSeq = new Map<string, number>();
	for (const run of runs.values()) {
		anchorSeq.set(run.runId, Number.MAX_SAFE_INTEGER);
	}
	for (const message of messages) {
		if (!message.run_id) continue;
		const run = runs.get(message.run_id);
		if (!run) continue;
		const current = anchorSeq.get(run.runId) ?? Number.MAX_SAFE_INTEGER;
		if (message.seq < current) {
			anchorSeq.set(run.runId, message.seq);
		}
	}

	const entries: { sort: number; tiebreak: number; entry: TimelineEntry }[] = [];
	for (const message of messages) {
		const runId = message.run_id;
		if (!runId || !runs.has(runId)) {
			entries.push({ sort: message.seq, tiebreak: 0, entry: { kind: "message", message } });
			continue;
		}
		// The triggering user message and the final agent response render flat.
		if (userTriggerIds.has(message.id) || finalIds.has(message.id)) {
			entries.push({ sort: message.seq, tiebreak: finalIds.has(message.id) ? 2 : 0, entry: { kind: "message", message } });
		}
	}
	for (const run of runs.values()) {
		const anchor = anchorSeq.get(run.runId) ?? Number.MAX_SAFE_INTEGER;
		entries.push({ sort: anchor, tiebreak: 1, entry: { kind: "group", runId: run.runId } });
	}

	entries.sort((a, b) => a.sort - b.sort || a.tiebreak - b.tiebreak);
	return entries.map((item) => item.entry);
}

const OVERRIDE_PREFIX = "stride.ui.";

export function readOverrides(threadId: string): OverrideStore {
	if (!threadId) return { runs: {}, tools: {} };
	try {
		const raw = localStorage.getItem(OVERRIDE_PREFIX + threadId);
		if (!raw) return { runs: {}, tools: {} };
		const parsed = JSON.parse(raw) as Partial<OverrideStore>;
		return { runs: parsed.runs ?? {}, tools: parsed.tools ?? {} };
	} catch {
		return { runs: {}, tools: {} };
	}
}

export function writeOverrides(threadId: string, overrides: OverrideStore): void {
	if (!threadId) return;
	try {
		localStorage.setItem(OVERRIDE_PREFIX + threadId, JSON.stringify(overrides));
	} catch {
		// Storage may be unavailable (private mode); overrides degrade to session-only.
	}
}

const root = document.querySelector<HTMLElement>("#threads-page");

class ThreadsPageHydrator {
	private threadId: string;
	private threads: ThreadSummary[];
	private projects: ProjectSummary[];
	private currentProjectId: string | null = null;
	private messages: ViewMessage[] = [];
	private runs: Map<string, RunState> = new Map();
	private overrides: OverrideStore = { runs: {}, tools: {} };
	private attachedFiles: { name: string; id: string }[] = [];
	private modelOptions: { value: string; label: string }[] = [];
	private selectedModel = "";
	private running: boolean;
	private error = "";
	private events: WebSocket | null = null;
	private pendingAssistantRunId: string | null = null;
	private pendingAssistant = "";
	private pendingThinking = "";
	private pendingAssistantFormat: ThreadMessage["format"] = "markdown";
	private pendingApproval: { id: string; message: string } | null = null;
	private pendingQuiz: PendingQuiz | null = null;
	private refreshSeq = 0;
	private lastEventSeq = 0;
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

		this.overrides = readOverrides(this.threadId);
		this.bindEvents();
		void this.loadModels();
		this.syncComposer();

		if (this.threadId) {
			void this.hydrateMessages(this.threadId);
		}
	}

	// The server renders the timeline into the DOM, but the hydrator owns the
	// `messages`/`runs` state that every re-render reads from. Load both before
	// opening the live stream so an incoming event never renders against a
	// half-populated model.
	private async hydrateMessages(threadId: string) {
		try {
			const [messages, runs] = await Promise.all([listMessages(threadId), fetchRuns(threadId)]);
			if (this.threadId !== threadId) {
				return;
			}
			this.messages = messages;
			this.ingestRuns(runs);
			this.renderTimeline();
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
			this.selectedModel = (event as CustomEvent<{ value: string }>).detail.value;
			this.syncComposer();
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
		this.messagesEl.addEventListener("rungroup-toggle", (event) =>
			this.onRunGroupToggle(event as CustomEvent<{ open: boolean }>),
		);
		this.messagesEl.addEventListener("toolcall-toggle", (event) =>
			this.onToolCallToggle(event as CustomEvent<{ open: boolean }>),
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
			this.applySnapshot(event, event.kind);
		}

		if (event.kind.type === "RunStarted" && event.run_id) {
			this.running = true;
			this.pendingAssistantRunId = event.run_id;
			this.pendingAssistant = "";
			this.pendingThinking = "";
			this.pendingAssistantFormat = "markdown";
			this.ensureRun(event.run_id, event.kind.started_at_ms, "running");
			this.syncComposer();
		}

		if (event.kind.type === "UserMessageCommitted") {
			const pending = this.messages.find((message) => message.pending && message.role === "user");
			if (pending) {
				pending.id = event.kind.message_id;
				pending.seq = event.kind.seq;
				pending.pending = false;
				pending.run_id = this.pendingAssistantRunId;
				if (this.pendingAssistantRunId) {
					const run = this.runs.get(this.pendingAssistantRunId);
					if (run && !run.userMessageId) run.userMessageId = pending.id;
				}
				this.renderTimeline();
			}
		}

		if (event.kind.type === "AgentDelta") {
			this.pendingAssistant = event.kind.content;
			this.pendingAssistantFormat = event.kind.format;
			this.upsertPendingAssistant();
		}

		if (event.kind.type === "ThinkingDelta") {
			this.pendingThinking += event.kind.thinking;
			this.upsertPendingAssistant();
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
			this.onAgentMessageCommitted(event.run_id, event.kind.message_id, event.kind.seq);
		}

		if (event.kind.type === "ToolStarted" && event.run_id) {
			this.running = true;
			this.onToolStarted(event.run_id, event.kind);
			this.syncComposer();
		}

		if (event.kind.type === "ToolProgress") {
			this.appendToolProgress(event.kind.tool_call_id, event.kind.name, event.kind.delta, event.kind.format);
		}

		if (event.kind.type === "ToolFinished" && event.run_id) {
			this.onToolFinished(event.run_id, event.kind);
			this.pendingApproval = null;
			this.pendingQuiz = null;
			this.syncComposer();
		}

		if (event.kind.type === "RunFinished" && event.run_id) {
			this.running = false;
			this.pendingApproval = null;
			this.pendingQuiz = null;
			this.finishRun(event.run_id, "finished", event.kind.finished_at_ms, event.kind.final_message_id);
			this.syncComposer();
			void this.refreshAfterRun();
		}

		if (event.kind.type === "RunFailed" && event.run_id) {
			this.running = false;
			this.pendingApproval = null;
			this.pendingQuiz = null;
			this.finishRun(event.run_id, "failed", event.kind.finished_at_ms, undefined);
			this.syncComposer();
			this.setError(event.kind.error);
		}

		if (event.kind.type === "RunCancelled" && event.run_id) {
			this.running = false;
			this.pendingApproval = null;
			this.pendingQuiz = null;
			this.finishRun(event.run_id, "cancelled", event.kind.finished_at_ms, undefined);
			this.clearPendingAssistant();
			this.syncComposer();
			void this.refreshAfterRun();
		}
	}

	private applySnapshot(
		event: ThreadEvent,
		kind: Extract<ThreadEvent["kind"], { type: "Snapshot" }>,
	) {
		this.running = kind.status === "running";
		this.pendingApproval = kind.pending_approval
			? { id: kind.pending_approval.approval_id, message: kind.pending_approval.message }
			: null;
		this.pendingQuiz = kind.pending_quiz
			? this.createPendingQuiz(kind.pending_quiz.quiz_id, kind.pending_quiz.questions)
			: null;
		this.syncComposer();

		if (kind.run) {
			this.pendingAssistantRunId = kind.run.run_id;
			this.ensureRun(kind.run.run_id, kind.run.started_at_ms, "running");
		}
		for (const progress of kind.tool_progress) {
			this.upsertToolProgress(kind.run?.run_id ?? this.pendingAssistantRunId, progress);
		}
		if (kind.in_progress) {
			this.pendingAssistantRunId = kind.in_progress.run_id;
			this.pendingAssistant = kind.in_progress.content;
			this.pendingThinking = kind.in_progress.thinking ?? "";
			this.pendingAssistantFormat = kind.in_progress.format;
			this.upsertPendingAssistant();
		} else if (kind.status === "idle") {
			this.clearPendingAssistant();
		}
		this.renderTimeline();
	}

	private createPendingQuiz(id: string, questions: QuizQuestion[]): PendingQuiz | null {
		if (questions.length === 0) {
			return null;
		}

		return { id, questions, index: 0, answers: [] };
	}

	private ingestRuns(runs: RunInfo[]) {
		this.runs = new Map();
		for (const run of runs) {
			const toolCalls = new Map<string, ToolCallState>();
			for (const call of run.tool_calls) {
				toolCalls.set(call.tool_call_id, {
					toolCallId: call.tool_call_id,
					name: call.name,
					status: call.status,
					background: call.background,
					startedAtMs: call.started_at_ms,
					finishedAtMs: call.finished_at_ms ?? 0,
					callSeq: call.call_seq,
					assistantMessageId: call.assistant_message_id ?? null,
					content: "",
					format: this.toolFormat(call.output_format),
				});
			}
			this.runs.set(run.id, {
				runId: run.id,
				status: run.status,
				startedAtMs: run.started_at_ms,
				finishedAtMs: run.finished_at_ms ?? 0,
				userMessageId: run.user_message_id ?? null,
				finalMessageId: run.final_message_id ?? null,
				toolCalls,
			});
		}
		this.hydrateToolResultsFromMessages();
	}

	private toolFormat(output: string | null | undefined): string {
		return output === "markdown" ? "markdown" : "plaintext";
	}

	private ensureRun(runId: string, startedAtMs: number, status: RunStatus): RunState {
		let run = this.runs.get(runId);
		if (!run) {
			run = {
				runId,
				status,
				startedAtMs: startedAtMs || Date.now(),
				finishedAtMs: 0,
				userMessageId: null,
				finalMessageId: null,
				toolCalls: new Map(),
			};
			this.runs.set(runId, run);
		} else if (status === "running") {
			run.status = "running";
			if (startedAtMs > 0) run.startedAtMs = startedAtMs;
		}
		return run;
	}

	private finishRun(
		runId: string,
		status: RunStatus,
		finishedAtMs: number,
		finalMessageId: string | undefined,
	) {
		const run = this.ensureRun(runId, 0, status);
		run.status = status;
		run.finishedAtMs = finishedAtMs || Date.now();
		if (finalMessageId) run.finalMessageId = finalMessageId;
		// Auto-collapse on finish and clear any manual override so the group
		// settles into its resting collapsed state.
		if (this.overrides.runs[runId] !== undefined) {
			delete this.overrides.runs[runId];
			this.persistOverrides();
		}
		for (const call of run.toolCalls.values()) {
			if (call.status === "running") {
				call.status = status === "failed" ? "failed" : status === "cancelled" ? "cancelled" : "interrupted";
				call.finishedAtMs = run.finishedAtMs;
			}
		}
		this.renderTimeline();
	}

	private onToolStarted(
		runId: string,
		kind: Extract<ThreadEvent["kind"], { type: "ToolStarted" }>,
	) {
		const run = this.ensureRun(runId, 0, "running");
		const existing = run.toolCalls.get(kind.tool_call_id);
		if (existing) {
			existing.name = kind.name;
			existing.status = "running";
			existing.background = kind.background;
			existing.callSeq = kind.call_seq;
			if (kind.started_at_ms > 0) existing.startedAtMs = kind.started_at_ms;
		} else {
			run.toolCalls.set(kind.tool_call_id, {
				toolCallId: kind.tool_call_id,
				name: kind.name,
				status: "running",
				background: kind.background,
				startedAtMs: kind.started_at_ms || Date.now(),
				finishedAtMs: 0,
				callSeq: kind.call_seq,
				assistantMessageId: null,
				content: "",
				format: "plaintext",
			});
		}
		// A tool starting after the candidate final message means that message was
		// an intermediate note; recompute so it folds into the group.
		this.reclassifyPendingFinal(runId);
		this.renderTimeline();
	}

	private onToolFinished(
		runId: string,
		kind: Extract<ThreadEvent["kind"], { type: "ToolFinished" }>,
	) {
		const run = this.ensureRun(runId, 0, "running");
		const call = run.toolCalls.get(kind.tool_call_id);
		if (!call) return;
		call.status = kind.status;
		call.finishedAtMs = kind.finished_at_ms || Date.now();
		call.format = this.toolFormat(kind.format);
		this.syncToolCallElement(call);
	}

	private onAgentMessageCommitted(runId: string | null, messageId: string, seq: number) {
		const pending = this.findPendingAssistant() ??
			this.messages.find((message) => message.pending && message.role === "agent");
		if (pending) {
			pending.id = messageId;
			pending.seq = seq;
			pending.pending = false;
			pending.run_id = runId ?? pending.run_id;
			this.renderTimeline();
		}
	}

	// While a run streams, the freshly committed assistant message is a
	// candidate final response shown below the group. When further activity
	// happens (a new tool call), it becomes an intermediate note inside the
	// group. renderTimeline derives placement from finalMessageId + seq order,
	// so this only needs to drop the stale finalMessageId guess.
	private reclassifyPendingFinal(runId: string) {
		const run = this.runs.get(runId);
		if (run) run.finalMessageId = null;
	}

	// Folds persisted wakeup messages (source tool_wakeup, tool_call_id set)
	// into their matching tool-call slot as the result text, so they never
	// render as standalone bubbles.
	private hydrateToolResultsFromMessages() {
		for (const message of this.messages) {
			if (message.source !== "tool_wakeup" || !message.tool_call_id) continue;
			const call = this.findToolCall(message.tool_call_id);
			if (call) call.content = call.content || message.content;
		}
	}

	private findToolCall(toolCallId: string): ToolCallState | undefined {
		for (const run of this.runs.values()) {
			const call = run.toolCalls.get(toolCallId);
			if (call) return call;
		}
		return undefined;
	}

	private clearPendingAssistant() {
		this.pendingAssistantRunId = null;
		this.pendingAssistant = "";
		this.pendingThinking = "";
		this.pendingAssistantFormat = "markdown";
	}

	private upsertPendingAssistant() {
		const existing = this.findPendingAssistant();
		if (existing) {
			this.syncPendingAssistant(existing);
			return;
		}

		const message: ViewMessage = {
			id: `pending-agent-${this.pendingAssistantKey()}`,
			seq: Number.MAX_SAFE_INTEGER,
			role: "agent",
			source: "system",
			format: this.pendingAssistantFormat,
			content: this.pendingAssistant,
			thinking: this.pendingThinking || null,
			tool_call_name: null,
			tool_call_id: null,
			tool_format: null,
			run_id: this.pendingAssistantRunId,
			pending: true,
		};
		this.messages.push(message);
		this.renderTimeline();
	}

	private findPendingAssistant(): ViewMessage | undefined {
		return this.messages.find(
			(message) =>
				message.pending &&
				message.role === "agent" &&
				!message.tool_call_name &&
				message.id === `pending-agent-${this.pendingAssistantKey()}`,
		);
	}

	private syncPendingAssistant(message: ViewMessage) {
		message.pending = true;
		message.format = this.pendingAssistantFormat;
		message.content = this.pendingAssistant;
		message.thinking = this.pendingThinking || null;
		message.run_id = this.pendingAssistantRunId;
		// A streaming pending message is always the candidate final response;
		// update it in place without a full timeline rebuild.
		this.syncFlatMessage(message);
	}

	private appendToolProgress(toolCallId: string, name: string, delta: string, format: ThreadMessage["tool_format"]) {
		const call = this.findToolCall(toolCallId);
		if (call) {
			call.content = (call.content ?? "") + delta;
			call.name = call.name || name;
			call.format = this.toolFormat(format);
			this.syncToolCallElement(call);
			return;
		}
		// Progress before ToolStarted arrived (rare ordering) — stash on the
		// active run so the slot picks it up once created.
		const run = this.pendingAssistantRunId ? this.runs.get(this.pendingAssistantRunId) : undefined;
		if (run) {
			run.toolCalls.set(toolCallId, {
				toolCallId,
				name,
				status: "running",
				background: false,
				startedAtMs: Date.now(),
				finishedAtMs: 0,
				callSeq: run.toolCalls.size,
				assistantMessageId: null,
				content: delta,
				format: this.toolFormat(format),
			});
			this.renderTimeline();
		}
	}

	private upsertToolProgress(
		runId: string | null,
		progress: { tool_call_id: string; name: string; content: string; format: string; call_seq: number; background: boolean; started_at_ms: number },
	) {
		const call = this.findToolCall(progress.tool_call_id);
		if (call) {
			call.content = progress.content;
			call.name = progress.name || call.name;
			call.format = this.toolFormat(progress.format);
			call.background = progress.background;
			call.callSeq = progress.call_seq;
			if (progress.started_at_ms > 0) call.startedAtMs = progress.started_at_ms;
			return;
		}
		if (!runId) return;
		const run = this.ensureRun(runId, 0, "running");
		run.toolCalls.set(progress.tool_call_id, {
			toolCallId: progress.tool_call_id,
			name: progress.name,
			status: "running",
			background: progress.background,
			startedAtMs: progress.started_at_ms || Date.now(),
			finishedAtMs: 0,
			callSeq: progress.call_seq,
			assistantMessageId: null,
			content: progress.content,
			format: this.toolFormat(progress.format),
		});
	}

	private async refreshAfterRun() {
		if (!this.threadId) {
			return;
		}

		const refreshSeq = ++this.refreshSeq;
		this.clearPendingAssistant();
		const [messages, runs, threads, projects] = await Promise.all([
			listMessages(this.threadId),
			fetchRuns(this.threadId),
			listThreads(),
			listProjects(),
		]);
		if (refreshSeq !== this.refreshSeq) {
			return;
		}

		this.messages = messages;
		this.ingestRuns(runs);
		this.renderTimeline();
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
				this.overrides = readOverrides(this.threadId);
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
			source: "human",
			format: "markdown",
			content,
			thinking: null,
			tool_call_name: null,
			tool_call_id: null,
			tool_format: null,
			run_id: null,
			pending: true,
		};
		this.messages.push(message);
		this.renderTimeline();
		this.scrollToBottom();
	}

	private async loadThread(threadId: string) {
		this.closeEvents();
		this.lastEventSeq = 0;
		this.clearPendingAssistant();
		this.pendingApproval = null;
		this.pendingQuiz = null;
		this.attachedFiles = [];
		this.setError("");
		this.overrides = readOverrides(threadId);

		try {
			const [messages, runs] = await Promise.all([listMessages(threadId), fetchRuns(threadId)]);
			this.messages = messages;
			this.ingestRuns(runs);
			this.renderTimeline();
			this.scrollInitial();
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

	// Rebuilds the top-level timeline: flat messages and run groups reconciled
	// by stable keys so element instances (and their timers/state) survive.
	private renderTimeline() {
		if (!this.threadId || (this.messages.length === 0 && this.runs.size === 0)) {
			this.messagesEl.replaceChildren(this.createEmptyElement());
			return;
		}
		this.messagesEl.querySelector("[data-empty]")?.remove();

		const messageEls = new Map<string, MessageEl>();
		this.messagesEl.querySelectorAll<MessageEl>(":scope > app-message[data-message-id]").forEach((el) => {
			messageEls.set(el.dataset.itemId ?? el.dataset.messageId ?? "", el);
		});
		const groupEls = new Map<string, RunGroupEl>();
		this.messagesEl.querySelectorAll<RunGroupEl>(":scope > app-run-group[data-run-id]").forEach((el) => {
			groupEls.set(el.dataset.runId ?? "", el);
		});

		const timeline = buildTimeline(this.messages, this.runs);
		let cursor: ChildNode | null = null;
		for (const entry of timeline) {
			if (entry.kind === "message") {
				const itemId = this.messageItemId(entry.message);
				const element = messageEls.get(itemId) ?? this.createMessageElement(entry.message);
				this.syncMessageElement(element, entry.message);
				this.placeAfter(element, cursor);
				messageEls.delete(itemId);
				cursor = element;
			} else {
				const element = groupEls.get(entry.runId) ?? this.createRunGroupElement(entry.runId);
				this.syncRunGroupElement(element, entry.runId);
				this.placeAfter(element, cursor);
				groupEls.delete(entry.runId);
				cursor = element;
			}
		}
		for (const stale of messageEls.values()) stale.remove();
		for (const stale of groupEls.values()) stale.remove();
	}

	// Moves an element to sit right after `cursor` (or at the front) without
	// detaching it when it is already in place — preserving component state.
	private placeAfter(element: HTMLElement, cursor: ChildNode | null) {
		const target = cursor ? cursor.nextSibling : this.messagesEl.firstChild;
		if (target === element) return;
		this.messagesEl.insertBefore(element, target);
	}

	private createRunGroupElement(runId: string): RunGroupEl {
		const element = document.createElement("app-run-group") as RunGroupEl;
		element.dataset.runId = runId;
		return element;
	}

	private syncRunGroupElement(element: RunGroupEl, runId: string) {
		const run = this.runs.get(runId);
		if (!run) return;
		element.setAttribute("data-run-id", runId);
		element.runId = runId;
		element.status = run.status;
		element.startedAtMs = run.startedAtMs;
		element.finishedAtMs = run.finishedAtMs;
		element.open = this.runOpen(run);
		this.syncRunChildren(element, run);
	}

	// Group open state: overrides win; otherwise running groups are open and
	// finished groups collapsed.
	private runOpen(run: RunState): boolean {
		const override = this.overrides.runs[run.runId];
		if (override !== undefined) return override;
		return run.status === "running";
	}

	private toolOpen(call: ToolCallState): boolean {
		const override = this.overrides.tools[call.toolCallId];
		if (override !== undefined) return override;
		return false;
	}

	// Reconciles a run group's light-DOM children: intermediate agent messages
	// and one app-tool-call per tool call, ordered by (anchor seq, call_seq).
	private syncRunChildren(host: RunGroupEl, run: RunState) {
		const childMessageEls = new Map<string, MessageEl>();
		host.querySelectorAll<MessageEl>(":scope > app-message[data-message-id]").forEach((el) => {
			childMessageEls.set(el.dataset.itemId ?? el.dataset.messageId ?? "", el);
		});
		const toolEls = new Map<string, ToolCallEl>();
		host.querySelectorAll<ToolCallEl>(":scope > app-tool-call[data-tool-call-id]").forEach((el) => {
			toolEls.set(el.dataset.toolCallId ?? "", el);
		});

		const items = this.runChildItems(run);
		let cursor: ChildNode | null = null;
		for (const item of items) {
			if (item.kind === "message") {
				const itemId = this.messageItemId(item.message);
				const element = childMessageEls.get(itemId) ?? this.createMessageElement(item.message);
				this.syncMessageElement(element, item.message);
				this.placeChildAfter(host, element, cursor);
				childMessageEls.delete(itemId);
				cursor = element;
			} else {
				const element = toolEls.get(item.call.toolCallId) ?? this.createToolCallElement(item.call.toolCallId);
				this.syncToolCallElement(item.call, element);
				this.placeChildAfter(host, element, cursor);
				toolEls.delete(item.call.toolCallId);
				cursor = element;
			}
		}
		for (const stale of childMessageEls.values()) stale.remove();
		for (const stale of toolEls.values()) stale.remove();
	}

	private placeChildAfter(host: HTMLElement, element: HTMLElement, cursor: ChildNode | null) {
		const target = cursor ? cursor.nextSibling : host.firstChild;
		if (target === element) return;
		host.insertBefore(element, target);
	}

	// A run's in-group items: intermediate agent messages (not the trigger,
	// not the final) and tool calls, ordered by anchor seq then call_seq.
	private runChildItems(run: RunState): (
		| { kind: "message"; message: ViewMessage; sort: number; tiebreak: number }
		| { kind: "tool"; call: ToolCallState; sort: number; tiebreak: number }
	)[] {
		const seqById = new Map<string, number>();
		for (const message of this.messages) seqById.set(message.id, message.seq);
		const finalId = candidateFinalId(run, this.messages);

		const items: (
			| { kind: "message"; message: ViewMessage; sort: number; tiebreak: number }
			| { kind: "tool"; call: ToolCallState; sort: number; tiebreak: number }
		)[] = [];

		for (const message of this.messages) {
			if (message.run_id !== run.runId) continue;
			if (message.id === run.userMessageId || message.id === finalId) continue;
			if (message.source === "human") continue;
			if (message.role === "tool") continue;
			if (message.source === "tool_wakeup") continue;
			items.push({ kind: "message", message, sort: message.seq, tiebreak: 0 });
		}
		for (const call of run.toolCalls.values()) {
			const anchor = call.assistantMessageId ? seqById.get(call.assistantMessageId) : undefined;
			const sort = anchor ?? call.startedAtMs;
			items.push({ kind: "tool", call, sort, tiebreak: 1 + call.callSeq });
		}
		items.sort((a, b) => a.sort - b.sort || a.tiebreak - b.tiebreak);
		return items;
	}

	private createToolCallElement(toolCallId: string): ToolCallEl {
		const element = document.createElement("app-tool-call") as ToolCallEl;
		element.dataset.toolCallId = toolCallId;
		return element;
	}

	// Updates a tool-call slot in place. The wakeup result rides in resultText;
	// accumulated stream content is the body.
	private syncToolCallElement(call: ToolCallState, element?: ToolCallEl) {
		const target = element ?? this.messagesEl.querySelector<ToolCallEl>(
			`app-tool-call[data-tool-call-id="${this.escapeSelectorValue(call.toolCallId)}"]`,
		);
		if (!target) return;
		target.setAttribute("data-tool-call-id", call.toolCallId);
		target.toolCallId = call.toolCallId;
		target.name = esc(call.name);
		target.status = call.status;
		target.background = call.background;
		target.startedAtMs = call.startedAtMs;
		target.finishedAtMs = call.finishedAtMs;
		target.open = this.toolOpen(call);
		target.format = call.format;
		target.content = call.content ? esc(call.content) : "";
		target.resultText = this.toolResultText(call);
	}

	// The wakeup message content becomes the tool's result section.
	private toolResultText(call: ToolCallState): string {
		const wakeup = this.messages.find(
			(message) => message.source === "tool_wakeup" && message.tool_call_id === call.toolCallId,
		);
		return wakeup?.content ? esc(wakeup.content) : "";
	}

	private onRunGroupToggle(event: CustomEvent<{ open: boolean }>) {
		const host = (event.target as Element | null)?.closest("app-run-group") as RunGroupEl | null;
		if (!host) return;
		const runId = host.dataset.runId ?? "";
		const run = this.runs.get(runId);
		if (!run) return;
		host.open = event.detail.open;
		this.overrides.runs[runId] = event.detail.open;
		this.persistOverrides();
	}

	private onToolCallToggle(event: CustomEvent<{ open: boolean }>) {
		const host = (event.target as Element | null)?.closest("app-tool-call") as ToolCallEl | null;
		if (!host) return;
		const toolCallId = host.dataset.toolCallId ?? "";
		host.open = event.detail.open;
		this.overrides.tools[toolCallId] = event.detail.open;
		this.persistOverrides();
	}

	private persistOverrides() {
		writeOverrides(this.threadId, this.overrides);
	}

	// Cheap in-place update of a top-level flat message element (used for the
	// streaming candidate-final path). Falls back to a full render if absent.
	private syncFlatMessage(message: ViewMessage) {
		const itemId = this.messageItemId(message);
		const element = this.messagesEl.querySelector<MessageEl>(
			`:scope > app-message[data-item-id="${this.escapeSelectorValue(itemId)}"]`,
		);
		if (!element) {
			this.renderTimeline();
			return;
		}
		this.syncMessageElement(element, message);
	}

	private syncMessageElement(element: MessageEl, message: ViewMessage) {
		const messageType = this.messageType(message);
		const itemId = this.messageItemId(message);
		element.setAttribute("data-message-id", message.id);
		element.setAttribute("data-item-id", itemId);
		element.itemId = itemId;
		element.seq = message.seq;
		element.role = message.role;
		element.source = message.source;

		if (element.kind === "tool_output" && messageType.type === "tool_output") {
			this.syncToolSpoiler(element, message, messageType.toolName ?? "Tool output");
			return;
		}

		if (element.kind === "agent" && messageType.type === "agent") {
			if (this.syncAgentBlock(element, message, messageType.toolName ?? "")) {
				return;
			}
		}

		element.kind = messageType.type;
		element.toolName = esc(messageType.toolName ?? "");
		element.thinking = message.thinking ? esc(message.thinking) : "";

		if (messageType.type === "tool_output") {
			element.format = message.tool_format === "markdown" ? "markdown" : "plaintext";
			element.text = message.content ? esc(message.content) : message.pending ? "Running..." : "";
			return;
		}

		element.format = message.format;
		element.text = message.content
			? this.messageText(message, messageType.type)
			: message.pending ? "Thinking..." : "";
	}

	private syncAgentBlock(element: MessageEl, message: ViewMessage, toolName: string): boolean {
		const markdown = element.shadowRoot?.querySelector<AutoMarkdownEl>("auto-markdown");
		const hasThinking = (message.thinking ?? "") !== "";
		let didFallback = false;

		const spoiler = element.shadowRoot?.querySelector<SpoilerEl>("app-spoiler");
		if (hasThinking) {
			if (spoiler) {
				spoiler.content = esc(message.thinking ?? "");
			} else {
				didFallback = true;
			}
		} else if (spoiler) {
			didFallback = true;
		}

		if (markdown) {
			markdown.format = message.format;
			markdown.text = message.content ? this.messageText(message, "agent") : message.pending ? "Thinking..." : "";
		} else {
			didFallback = true;
		}

		if (didFallback) {
			element.toolName = esc(toolName);
			element.format = message.format;
			element.thinking = message.thinking ? esc(message.thinking) : "";
			element.text = message.content ? this.messageText(message, "agent") : message.pending ? "Thinking..." : "";
			return false;
		}

		return true;
	}

	private syncToolSpoiler(element: MessageEl, message: ViewMessage, toolName: string) {
		element.toolName = esc(toolName);
		const spoiler = element.shadowRoot?.querySelector<SpoilerEl>("app-spoiler");
		if (!spoiler) {
			element.kind = "tool_output";
			element.format = message.tool_format === "markdown" ? "markdown" : "plaintext";
			element.text = message.content ? esc(message.content) : message.pending ? "Running..." : "";
			return;
		}

		spoiler.title = esc(toolName);
		spoiler.format = message.tool_format === "markdown" ? "markdown" : "plaintext";
		spoiler.content = message.content ? esc(message.content) : message.pending ? "Running..." : "";
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
		const last = this.lastTimelineElement();
		if (!last) {
			return;
		}
		this.scrollEl.scrollTop = last.offsetTop - this.scrollEl.offsetTop;
	}

	private scrollToLastMessageEnd() {
		const last = this.lastTimelineElement();
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

	private lastTimelineElement(): HTMLElement | null {
		const items = this.messagesEl.querySelectorAll<HTMLElement>(
			":scope > app-message, :scope > app-run-group",
		);
		return items.length > 0 ? items[items.length - 1] : null;
	}

	private escapeSelectorValue(value: string): string {
		return typeof CSS !== "undefined" ? CSS.escape(value) : value.replace(/\"/g, '\\"');
	}

	private createMessageElement(message: ViewMessage) {
		const element = document.createElement("app-message") as MessageEl;
		this.syncMessageElement(element, message);
		return element;
	}

	private messageItemId(message: ViewMessage): string {
		if (message.tool_call_id) {
			return `tool:${message.tool_call_id}`;
		}
		if (message.pending && message.role === "agent" && !message.tool_call_name) {
			const prefix = "pending-agent-";
			if (message.id.startsWith(prefix)) {
				return `agent:${message.id.slice(prefix.length) || "pending"}`;
			}
			return this.pendingAssistantItemId();
		}
		return message.id;
	}

	private pendingAssistantItemId(): string {
		return `agent:${this.pendingAssistantKey()}`;
	}

	private pendingAssistantKey(): string {
		return this.pendingAssistantRunId ?? "pending";
	}

	private messageText(message: ThreadMessage, messageType: string): string {
		return messageType === "agent" && message.format === "html"
			? message.content
			: esc(message.content);
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
		if (message.tool_call_name) {
			return { type: "agent", toolName: message.tool_call_name };
		}
		if (message.role === "user" && message.source !== "human") {
			return { type: "agent_note" };
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
		this.approvalEl.message = esc(this.pendingApproval?.message ?? "");
		this.quizEl.hidden = !hasQuiz;
		const quiz = this.pendingQuiz;
		const question = quiz ? quiz.questions[quiz.index] : undefined;
		this.quizEl.question = esc(question?.question ?? "");
		this.quizEl.options = (question?.options ?? []).map(esc);
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
			(mutation: ThreadMutation) => {
				if (mutation === "delete" || mutation === "archive") {
					window.location.href = "/threads";
					return;
				}
				window.location.reload();
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
