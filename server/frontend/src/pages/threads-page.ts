import { LitElement, css, html, nothing } from "lit";
import { logout } from "../api/auth.js";
import {
  ThreadEvent,
  ThreadMessage,
  ThreadSummary,
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

export class ThreadsPage extends LitElement {
  static properties = {
    threadId: { type: String, attribute: "thread-id" },
    threads: { state: true },
    messages: { state: true },
    draft: { state: true },
    running: { state: true },
    loading: { state: true },
    error: { state: true },
  };

  threadId = "";
  threads: ThreadSummary[] = [];
  messages: ViewMessage[] = [];
  draft = "";
  running = false;
  loading = true;
  error = "";

  private events: EventSource | null = null;
  private pendingAssistant = "";

  static styles = css`
    :host {
      /*--background: #121212;
			--foreground: #e5e5e5;
			--card: #1f1f1f;
			--card-foreground: #e5e5e5;
			--primary: #3a3a3a;
			--primary-foreground: #f4f4f5;
			--primary-hover: #454545;
			--secondary: #242424;
			--secondary-foreground: #fafafa;
			--secondary-hover: #303030;
			--muted: #1a1a1a;
			--muted-foreground: #a3a3a3;
			--accent: #2a2a2a;
			--accent-foreground: #fafafa;
			--border: #303030;
			--input: #3a3a3a;
			--input-disabled: rgb(58 58 58 / 55%);
			--ring: #a3a3a3;
			--ring-shadow: rgb(255 255 255 / 10%);
			--destructive: #f87171;
			--destructive-muted: rgb(248 113 113 / 14%);
			--destructive-hover: rgb(248 113 113 / 22%);
			--destructive-ring: rgb(248 113 113 / 40%);
			--destructive-shadow: rgb(248 113 113 / 24%);
			--surface-gradient: linear-gradient(rgb(18 18 18 / 0), var(--background) 28px);
			--topbar-bg: rgb(18 18 18 / 84%);
			--message-user-bg: #2f2f2f;
			--message-user-fg: #f4f4f5;
			--message-agent-bg: #1f1f1f;
			--message-agent-border: #303030;
			--message-agent-shadow: 0 1px 2px rgb(0 0 0 / 20%);*/
      background: var(--background);
      color: var(--foreground);
      display: block;
      font-family:
        ui-sans-serif,
        system-ui,
        -apple-system,
        BlinkMacSystemFont,
        "Segoe UI",
        sans-serif;
      min-height: 100svh;
    }

    app-sidebar-provider {
      --sidebar-width: 17.5rem;
      --sidebar-bg: var(--muted);
      --sidebar-fg: var(--muted-foreground);
      --sidebar-accent: var(--accent);
      --sidebar-accent-fg: var(--accent-foreground);
      --sidebar-border: var(--border);
      background: var(--background);
      min-height: 100svh;
    }

    .brand {
      align-items: center;
      display: flex;
      gap: 10px;
      padding: 4px;
    }

    .mark {
      align-items: center;
      background: var(--primary);
      border-radius: 8px;
      color: var(--primary-foreground);
      display: inline-flex;
      font-size: 13px;
      font-weight: 700;
      height: 32px;
      justify-content: center;
      width: 32px;
    }

    .brand strong {
      color: var(--foreground);
      font-size: 14px;
      font-weight: 650;
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
      height: 100svh;
      min-height: 0;
      overflow: hidden;
    }

    .topbar {
      align-items: center;
      backdrop-filter: blur(18px);
      background: var(--topbar-bg);
      border-bottom: 1px solid var(--border);
      display: flex;
      gap: 10px;
      min-height: 52px;
      padding: 0 clamp(14px, 2.4vw, 28px);
      position: sticky;
      top: 0;
      z-index: 10;
    }

    .topbar h1 {
      color: var(--card-foreground);
      font-size: 14px;
      font-weight: 600;
      margin: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .messages {
      margin: 0 auto;
      max-width: 800px;
      min-height: 0;
      overflow-y: auto;
      padding: 32px clamp(18px, 4vw, 32px) 24px;
      scrollbar-width: thin;
      width: 100%;
    }

    .empty {
      align-content: center;
      display: grid;
      justify-items: center;
      min-height: 100%;
      padding-bottom: 96px;
      text-align: center;
    }

    .empty h2 {
      color: var(--foreground);
      font-size: clamp(28px, 4vw, 40px);
      font-weight: 700;
      letter-spacing: -0.02em;
      line-height: 1.08;
      margin: 0 0 12px;
    }

    .empty p {
      color: var(--muted-foreground);
      font-size: 15px;
      line-height: 1.5;
      margin: 0;
      max-width: 420px;
    }

    .composer-wrap {
      background: var(--surface-gradient);
      padding: 18px clamp(14px, 4vw, 28px) 24px;
      position: sticky;
      bottom: 0;
      z-index: 10;
    }

    app-prompt-input {
      margin: 0 auto;
      max-width: 860px;
      width: 100%;
    }

    app-button.sidebar-action {
      width: 100%;
    }

    .error {
      color: var(--destructive);
      font-size: 13px;
      margin: 10px auto 0;
      max-width: 860px;
    }

    @media (max-width: 760px) {
      .main {
        height: 100svh;
      }

      .messages {
        max-width: none;
        padding: 20px 14px 18px;
        width: 100%;
      }

      .composer-wrap {
        padding: 12px 10px 12px;
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
    if (changed.has("threadId")) {
      void this.loadThread();
    }
  }

  render() {
    return html`
      <app-sidebar-provider>
        <app-sidebar collapsible="offcanvas">
          <app-sidebar-header>
            <div class="brand">
              <span class="mark">F</span><strong>Friday</strong>
            </div>
            <app-button
              class="sidebar-action"
              variant="secondary"
              @click=${this.startNew}
              >New thread</app-button
            >
          </app-sidebar-header>
          <app-sidebar-content>
            <app-sidebar-group>
              <app-sidebar-group-label>Threads</app-sidebar-group-label>
              <app-sidebar-group-content>
                <app-sidebar-menu
                  >${this.threads.map((thread) =>
                    this.renderThread(thread),
                  )}</app-sidebar-menu
                >
              </app-sidebar-group-content>
            </app-sidebar-group>
          </app-sidebar-content>
          <app-sidebar-footer>
            <app-button
              class="sidebar-action"
              variant="secondary"
              @click=${this.onLogout}
              >Log out</app-button
            >
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
              <app-prompt-input
                .value=${this.draft}
                placeholder=${this.threadId
                  ? "Message Friday"
                  : "Ask Friday anything"}
                ?disabled=${this.running}
                @value-change=${this.onDraft}
                @prompt-submit=${this.onPromptSubmit}
              ></app-prompt-input>
              ${this.error
                ? html`<div class="error">${this.error}</div>`
                : nothing}
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
      return html`<div class="empty">
        <h2>What are we working on?</h2>
        <p>Start a thread and Friday will keep the context here.</p>
      </div>`;
    }

    return this.messages.map((message) => this.renderMessage(message));
  }

  private renderMessage(message: ViewMessage) {
    const type =
      message.role === "tool"
        ? "tool_output"
        : message.role === "system"
          ? "agent"
          : message.role;

    return html`
      <app-message
        .message_id=${message.id}
        .type=${type}
        .tool_name=${message.role === "tool" ? "Tool output" : undefined}
        .with_thinking=${Boolean(message.thinking)}
      >
        ${message.thinking
          ? html`<span slot="thinking">${message.thinking}</span>`
          : nothing}
        ${message.content || (message.pending ? "Thinking..." : "")}
      </app-message>
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
    this.pendingAssistant = "";
    this.error = "";

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
    this.events.onmessage = (event) =>
      this.applyEvent(JSON.parse(event.data) as ThreadEvent);
    this.events.onerror = () => {
      this.error = "Live updates disconnected.";
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

    if (event.kind.type === "Snapshot") {
      this.running = event.kind.status === "running";
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
    }

    if (event.kind.type === "AgentDelta") {
      this.pendingAssistant += event.kind.content;
      this.upsertPendingAssistant();
    }

    if (event.kind.type === "ThinkingDelta") {
      this.upsertPendingAssistant(event.kind.thinking);
    }

    if (
      event.kind.type === "RunFinished" ||
      event.kind.type === "AgentMessageCommitted"
    ) {
      this.running = false;
      void this.refreshAfterRun();
    }

    if (event.kind.type === "RunFailed") {
      this.running = false;
      this.error = event.kind.error;
    }
  }

  private upsertPendingAssistant(thinking?: string) {
    const messages = [...this.messages];
    const last = messages[messages.length - 1];

    if (last?.pending && last.role === "agent") {
      messages[messages.length - 1] = {
        ...last,
        content: this.pendingAssistant,
        thinking: thinking
          ? `${last.thinking ?? ""}${thinking}`
          : last.thinking,
      };
    } else {
      messages.push({
        id: "pending-agent",
        seq: Number.MAX_SAFE_INTEGER,
        role: "agent",
        content: this.pendingAssistant,
        thinking: thinking ?? null,
        pending: true,
      });
    }

    this.messages = messages;
  }

  private async refreshAfterRun() {
    if (!this.threadId) {
      return;
    }

    this.pendingAssistant = "";
    this.messages = await listMessages(this.threadId);
    this.threads = await listThreads();
  }

  private currentTitle() {
    return (
      this.threads.find((thread) => thread.id === this.threadId)?.title ??
      "New thread"
    );
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
    this.messages = [
      ...this.messages,
      {
        id: `pending-user-${Date.now()}`,
        seq: Number.MAX_SAFE_INTEGER,
        role: "user",
        content,
        thinking: null,
        pending: true,
      },
    ];

    try {
      if (this.threadId) {
        await sendMessage(this.threadId, content);
      } else {
        const response = await createThread(content);
        this.threadId = response.thread_id;
        history.pushState(null, "", `/threads/${response.thread_id}`);
        this.threads = await listThreads();
        await this.loadThread();
      }
    } catch (error) {
      this.running = false;
      this.handleError(error);
    }
  }

  private startNew() {
    this.threadId = "";
    this.messages = [];
    this.draft = "";
    this.running = false;
    history.pushState(null, "", "/threads");
  }

  private onThreadClick(event: MouseEvent) {
    event.preventDefault();
    const href =
      (event.currentTarget as HTMLElement).getAttribute("href") ?? "";
    const id = href.split("/").pop() ?? "";
    if (!id || id === this.threadId) {
      return;
    }
    this.threadId = id;
    history.pushState(null, "", `/threads/${id}`);
  }

  private async onLogout() {
    await logout();
    this.dispatchEvent(
      new CustomEvent("navigate", {
        bubbles: true,
        composed: true,
        detail: { path: "/login" },
      }),
    );
  }

  private handleError(error: unknown) {
    if (error instanceof Error && error.message === "401") {
      this.dispatchEvent(
        new CustomEvent("navigate", {
          bubbles: true,
          composed: true,
          detail: { path: "/login" },
        }),
      );
      return;
    }

    this.error = "Request failed.";
  }
}

customElements.define("threads-page", ThreadsPage);
