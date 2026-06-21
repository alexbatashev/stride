import { Component, css, effect, onMount } from "@frontiers-labs/argon";
import {
  createEmailAccount,
  createMcpServer,
  deleteEmailAccount,
  deleteMcpServer,
  disconnectTelegram,
  getTelegramSettings,
  listEmailAccounts,
  listMcpServers,
  loginTelegram,
  type EmailAccount,
  type McpServer,
  type TelegramAuthData,
} from "../api/settings.js";

type SettingsHost = HTMLElement & {
  activeSection: string;
  tgConfigured: boolean;
  tgConnected: boolean;
  tgStatus: string;
  tgBotUsername: string;
  tgError: string;
  emails: EmailAccount[];
  emailLoaded: boolean;
  emailError: string;
  mcps: McpServer[];
  mcpLoaded: boolean;
  mcpError: string;
};

function escapeHtml(value: string): string {
  return value
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    .replace(/'/g, "&#39;");
}

type AccountView = { id: string; name: string; meta: string };

function emailView(account: EmailAccount): AccountView {
  return {
    id: account.id,
    name: escapeHtml(account.name),
    meta: escapeHtml(`${account.email} · ${account.host}:${account.port}`),
  };
}

function mcpView(server: McpServer): AccountView {
  const headers = [server.has_authorization ? "Authorization" : "", ...server.header_names].filter(Boolean);
  return {
    id: server.id,
    name: escapeHtml(server.name),
    meta: escapeHtml(headers.length > 0 ? `${server.url} · headers: ${headers.join(", ")}` : server.url),
  };
}

async function refreshTelegram(host: SettingsHost): Promise<void> {
  try {
    const settings = await getTelegramSettings();
    host.tgError = "";
    host.tgConfigured = settings.bot_configured;
    host.tgConnected = settings.connected;
    host.tgBotUsername = settings.bot_username ?? "";
    if (!settings.bot_configured) {
      host.tgStatus = "Telegram bot is not configured on this server.";
    } else if (settings.connected) {
      const name = settings.username
        ? `@${settings.username}`
        : [settings.first_name, settings.last_name].filter(Boolean).join(" ");
      host.tgStatus = name ? `Connected as ${name}.` : "Telegram is connected.";
    } else if (settings.bot_username) {
      host.tgStatus = "Telegram is not connected.";
    } else {
      host.tgStatus = "Telegram bot username is unavailable, so the login button cannot be shown.";
    }
  } catch {
    host.tgError = "Failed to load Telegram settings.";
  }
}

async function refreshEmails(host: SettingsHost): Promise<void> {
  try {
    host.emails = await listEmailAccounts();
    host.emailLoaded = true;
    host.emailError = "";
  } catch {
    host.emailError = "Failed to load email accounts.";
  }
}

async function refreshMcps(host: SettingsHost): Promise<void> {
  try {
    host.mcps = await listMcpServers();
    host.mcpLoaded = true;
    host.mcpError = "";
  } catch {
    host.mcpError = "Failed to load MCP servers.";
  }
}

async function handleAuth(host: SettingsHost, user: TelegramAuthData): Promise<void> {
  try {
    await loginTelegram(user);
    await refreshTelegram(host);
  } catch {
    host.tgError = "Failed to connect Telegram.";
  }
}

async function submitEmail(host: SettingsHost, form: HTMLFormElement): Promise<void> {
  const data = new FormData(form);
  host.emailError = "";
  try {
    await createEmailAccount({
      name: String(data.get("name") ?? "").trim(),
      email: String(data.get("email") ?? "").trim(),
      host: String(data.get("host") ?? "").trim(),
      port: Number(data.get("port") ?? 993),
      username: String(data.get("username") ?? "").trim(),
      password: String(data.get("password") ?? ""),
      inbox_mailbox: String(data.get("inbox_mailbox") ?? "INBOX").trim(),
      sent_mailbox: String(data.get("sent_mailbox") ?? "Sent").trim(),
      drafts_mailbox: String(data.get("drafts_mailbox") ?? "Drafts").trim(),
    });
    form.reset();
    await refreshEmails(host);
  } catch (error) {
    host.emailError = error instanceof Error ? error.message : "Failed to add email account.";
  }
}

async function submitMcp(host: SettingsHost, form: HTMLFormElement): Promise<void> {
  const data = new FormData(form);
  host.mcpError = "";
  try {
    await createMcpServer({
      name: String(data.get("name") ?? "").trim(),
      url: String(data.get("url") ?? "").trim(),
      bearer_token: String(data.get("bearer_token") ?? ""),
      headers_json: String(data.get("headers_json") ?? "").trim(),
      enabled: true,
    });
    form.reset();
    await refreshMcps(host);
  } catch (error) {
    host.mcpError = error instanceof Error ? error.message : "Failed to add MCP server.";
  }
}

const styles = css`
  :host {
    display: block;
    height: 100%;
    min-height: 0;
    overflow: auto;
  }

  .root {
    box-sizing: border-box;
    min-height: 100%;
    padding: 32px 24px 64px;
  }

  .shell {
    display: flex;
    flex-direction: column;
    gap: 28px;
    margin: 0 auto;
    max-width: 920px;
    width: 100%;
  }

  h1,
  h2,
  p {
    margin: 0;
  }

  .page-title {
    color: var(--foreground);
    font-size: 26px;
    letter-spacing: -0.02em;
    line-height: 1.2;
  }

  .lead {
    color: var(--muted-foreground);
    font-size: 14px;
    line-height: 1.5;
    margin-top: 6px;
  }

  .layout {
    align-items: start;
    display: grid;
    gap: 28px;
    grid-template-columns: 200px minmax(0, 1fr);
  }

  .tabs {
    display: flex;
    flex-direction: column;
    gap: 2px;
    position: sticky;
    top: 0;
  }

  .tab {
    background: transparent;
    border: 0;
    border-radius: 8px;
    color: var(--muted-foreground);
    cursor: pointer;
    font: inherit;
    font-size: 14px;
    font-weight: 500;
    padding: 8px 12px;
    text-align: left;
    transition:
      background-color 140ms ease,
      color 140ms ease;
    white-space: nowrap;
  }

  .tab:hover {
    background: var(--accent);
    color: var(--foreground);
  }

  .layout[data-active="telegram"] .tab[data-section="telegram"],
  .layout[data-active="email"] .tab[data-section="email"],
  .layout[data-active="mcp"] .tab[data-section="mcp"] {
    background: var(--accent);
    color: var(--foreground);
    font-weight: 600;
  }

  .panels {
    display: flex;
    flex-direction: column;
    gap: 20px;
    min-width: 0;
  }

  .panel {
    display: none;
    flex-direction: column;
    gap: 20px;
  }

  .layout[data-active="telegram"] .panel[data-panel="telegram"],
  .layout[data-active="email"] .panel[data-panel="email"],
  .layout[data-active="mcp"] .panel[data-panel="mcp"] {
    display: flex;
  }

  .status-row {
    align-items: center;
    display: flex;
    flex-wrap: wrap;
    gap: 10px;
  }

  .status {
    color: var(--foreground);
    font-size: 14px;
  }

  .muted {
    color: var(--muted-foreground);
    font-size: 14px;
    line-height: 1.5;
  }

  .tg-widget:not(:has(::slotted(*))) {
    display: none;
  }

  .account-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .account {
    align-items: center;
    border: 1px solid var(--border);
    border-radius: 10px;
    display: flex;
    gap: 16px;
    justify-content: space-between;
    padding: 12px 14px;
  }

  .account .name {
    color: var(--foreground);
    font-size: 14px;
    font-weight: 600;
  }

  .account .meta {
    color: var(--muted-foreground);
    font-size: 12px;
    margin-top: 3px;
    overflow-wrap: anywhere;
  }

  .account app-button {
    flex: 0 0 auto;
    width: auto;
  }

  form {
    display: grid;
    gap: 14px;
  }

  .grid {
    display: grid;
    gap: 14px;
    grid-template-columns: 1fr 1fr;
  }

  label {
    color: var(--foreground);
    display: grid;
    font-size: 13px;
    font-weight: 500;
    gap: 6px;
  }

  label.full {
    grid-column: 1 / -1;
  }

  input,
  textarea {
    background: var(--background);
    border: 1px solid var(--input);
    border-radius: 8px;
    box-sizing: border-box;
    color: var(--foreground);
    font: inherit;
    font-size: 14px;
    outline: none;
    padding: 8px 10px;
    transition:
      border-color 140ms ease,
      box-shadow 140ms ease;
    width: 100%;
  }

  input {
    height: 36px;
  }

  textarea {
    min-height: 84px;
    resize: vertical;
  }

  input:focus,
  textarea:focus {
    border-color: var(--ring);
    box-shadow: 0 0 0 3px var(--ring-shadow);
  }

  input::placeholder,
  textarea::placeholder {
    color: var(--muted-foreground);
  }

  details summary {
    color: var(--foreground);
    cursor: pointer;
    font-size: 13px;
    font-weight: 500;
  }

  details .grid {
    margin-top: 14px;
  }

  .hint {
    color: var(--muted-foreground);
    font-size: 12px;
    line-height: 1.5;
  }

  .actions app-button {
    width: auto;
  }

  .error {
    color: var(--destructive);
    font-size: 13px;
  }

  .error:empty {
    display: none;
  }

  @media (max-width: 760px) {
    .root {
      padding: 24px 16px 48px;
    }

    .layout {
      grid-template-columns: 1fr;
      gap: 16px;
    }

    .tabs {
      flex-direction: row;
      overflow-x: auto;
      position: static;
    }

    .grid {
      grid-template-columns: 1fr;
    }
  }
`;

export function AppSettings({
  activeSection = "telegram",
  tgConfigured = false,
  tgConnected = false,
  tgStatus = "Loading…",
  tgBotUsername = "",
  tgError = "",
  emails = [],
  emailLoaded = false,
  emailError = "",
  mcps = [],
  mcpLoaded = false,
  mcpError = "",
}: {
  activeSection?: string;
  tgConfigured?: boolean;
  tgConnected?: boolean;
  tgStatus?: string;
  tgBotUsername?: string;
  tgError?: string;
  emails?: EmailAccount[];
  emailLoaded?: boolean;
  emailError?: string;
  mcps?: McpServer[];
  mcpLoaded?: boolean;
  mcpError?: string;
}): Component {
  onMount(() => {
    (window as unknown as Record<string, unknown>).onTelegramAuth = (user: TelegramAuthData) => {
      void handleAuth(this, user);
    };
    void refreshTelegram(this);
    void refreshEmails(this);
    void refreshMcps(this);
  });

  // Telegram's widget script finds its own <script> tag in the document and
  // injects the login iframe next to it. A script inside this component's
  // shadow DOM is not part of the document tree, so that lookup fails and no
  // button appears. Inject into a light-DOM child of the host and surface it
  // through the tg-widget slot; reinject only when the bot or state changes.
  effect(() => {
    const host = this;
    const show = tgConfigured && !tgConnected && tgBotUsername;
    const existing = host.querySelector<HTMLElement>(":scope > [data-tg-widget]");
    if (!show) {
      existing?.remove();
      return;
    }
    if (existing?.dataset.bot === tgBotUsername) return;
    existing?.remove();
    const container = document.createElement("div");
    container.dataset.tgWidget = "";
    container.dataset.bot = tgBotUsername;
    container.setAttribute("slot", "tg-widget");
    const script = document.createElement("script");
    script.async = true;
    script.src = "https://telegram.org/js/telegram-widget.js?22";
    script.setAttribute("data-telegram-login", tgBotUsername);
    script.setAttribute("data-size", "large");
    script.setAttribute("data-request-access", "write");
    script.setAttribute("data-onauth", "onTelegramAuth(user)");
    container.appendChild(script);
    host.appendChild(container);
  });

  const emailViews = emails.map(emailView);
  const mcpViews = mcps.map(mcpView);

  return (
    <>
      <style>{styles}</style>
      <div
        class="root"
        onClick={(event: Event) => {
          const node = event.target as HTMLElement;
          const tab = node.closest<HTMLElement>("[data-section]");
          if (tab?.dataset.section) {
            this.activeSection = tab.dataset.section;
            return;
          }
          const action = node.closest<HTMLElement>("[data-action]");
          if (!action) return;
          switch (action.dataset.action) {
            case "tg-disconnect":
              void disconnectTelegram()
                .then(() => refreshTelegram(this))
                .catch(() => {
                  this.tgError = "Failed to disconnect Telegram.";
                });
              return;
            case "add-email": {
              const form = action.closest<HTMLFormElement>("form");
              if (form) void submitEmail(this, form);
              return;
            }
            case "add-mcp": {
              const form = action.closest<HTMLFormElement>("form");
              if (form) void submitMcp(this, form);
              return;
            }
            case "del-email":
              if (action.dataset.id && window.confirm("Remove this IMAP account from Friday?")) {
                void deleteEmailAccount(action.dataset.id)
                  .then(() => refreshEmails(this))
                  .catch(() => {
                    this.emailError = "Failed to remove email account.";
                  });
              }
              return;
            case "del-mcp":
              if (action.dataset.id && window.confirm("Remove this MCP server from Friday?")) {
                void deleteMcpServer(action.dataset.id)
                  .then(() => refreshMcps(this))
                  .catch(() => {
                    this.mcpError = "Failed to remove MCP server.";
                  });
              }
              return;
          }
        }}
        onSubmit={(event: Event) => {
          event.preventDefault();
          const form = event.target as HTMLFormElement;
          if (form.dataset.form === "email") void submitEmail(this, form);
          if (form.dataset.form === "mcp") void submitMcp(this, form);
        }}
      >
        <div class="shell">
          <header>
            <h1 class="page-title">Settings</h1>
            <p class="lead">Manage account integrations Friday uses on your behalf.</p>
          </header>

          <div class="layout" data-active={activeSection}>
            <nav class="tabs" aria-label="Settings sections">
              <button type="button" class="tab" data-section="telegram">Telegram</button>
              <button type="button" class="tab" data-section="email">Email</button>
              <button type="button" class="tab" data-section="mcp">MCP servers</button>
            </nav>

            <div class="panels">
              <section class="panel" data-panel="telegram">
                <app-card title="Telegram" description="Connect your Telegram account with the Friday bot.">
                  <div class="status-row">
                    {tgConnected
                      ? <app-badge>Connected</app-badge>
                      : tgConfigured
                        ? <app-badge variant="outline">Not connected</app-badge>
                        : <app-badge variant="secondary">Unavailable</app-badge>}
                    <span class="status">{tgStatus}</span>
                  </div>
                  <div class="tg-widget"><slot name="tg-widget"></slot></div>
                  {tgConnected
                    ? <div><app-button variant="outline" data-action="tg-disconnect">Disconnect</app-button></div>
                    : ""}
                  <p class="error">{tgError}</p>
                </app-card>
              </section>

              <section class="panel" data-panel="email">
                <app-card
                  title="Email accounts"
                  description="Connect one or more TLS IMAP accounts. Friday can read incoming and sent mail and save reply-all drafts. It cannot send email."
                >
                  {emailViews.length > 0
                    ? (
                      <div class="account-list">
                        {emailViews.map((account) => (
                          <div class="account" key={account.id}>
                            <div>
                              <div class="name">{account.name}</div>
                              <div class="meta">{account.meta}</div>
                            </div>
                            <app-button variant="outline" size="sm" data-action="del-email" data-id={account.id}>Remove</app-button>
                          </div>
                        )).join("")}
                      </div>
                    )
                    : <p class="muted">{emailLoaded ? "No IMAP accounts yet." : "Loading accounts…"}</p>}
                </app-card>

                <app-card title="Add IMAP server" description="The connection is verified before it is saved. Credentials are encrypted at rest.">
                  <form data-form="email">
                    <div class="grid">
                      <label>Account name<input name="name" required placeholder="Work" autocomplete="off" /></label>
                      <label>Email address<input name="email" type="email" required placeholder="you@example.com" autocomplete="email" /></label>
                      <label>IMAP host<input name="host" required placeholder="imap.example.com" autocomplete="off" /></label>
                      <label>Port<input name="port" type="number" min="1" max="65535" value="993" required /></label>
                      <label>Username<input name="username" required placeholder="you@example.com" autocomplete="username" /></label>
                      <label>Password or app password<input name="password" type="password" required autocomplete="new-password" /></label>
                    </div>
                    <details>
                      <summary>Mailbox names</summary>
                      <div class="grid">
                        <label>Inbox<input name="inbox_mailbox" value="INBOX" required /></label>
                        <label>Sent<input name="sent_mailbox" value="Sent" required /></label>
                        <label>Drafts<input name="drafts_mailbox" value="Drafts" required /></label>
                      </div>
                    </details>
                    <div class="actions"><app-button data-action="add-email">Add account</app-button></div>
                    <p class="error">{emailError}</p>
                  </form>
                </app-card>
              </section>

              <section class="panel" data-panel="mcp">
                <app-card
                  title="MCP servers"
                  description="Add remote HTTP MCP servers for your agents. Tools from these servers load alongside the global MCP servers."
                >
                  {mcpViews.length > 0
                    ? (
                      <div class="account-list">
                        {mcpViews.map((server) => (
                          <div class="account" key={server.id}>
                            <div>
                              <div class="name">{server.name}</div>
                              <div class="meta">{server.meta}</div>
                            </div>
                            <app-button variant="outline" size="sm" data-action="del-mcp" data-id={server.id}>Remove</app-button>
                          </div>
                        )).join("")}
                      </div>
                    )
                    : <p class="muted">{mcpLoaded ? "No custom MCP servers yet." : "Loading servers…"}</p>}
                </app-card>

                <app-card title="Add MCP server" description="Only Streamable HTTP MCP servers are supported here. Authorization values are stored but not shown again.">
                  <form data-form="mcp">
                    <div class="grid">
                      <label>Name<input name="name" required placeholder="deepwiki" autocomplete="off" pattern="[A-Za-z][A-Za-z0-9_]{1,47}" /></label>
                      <label>URL<input name="url" type="url" required placeholder="https://mcp.example.com/mcp" autocomplete="off" /></label>
                    </div>
                    <label class="full">Bearer token<input name="bearer_token" type="password" autocomplete="new-password" /></label>
                    <label class="full">Headers JSON<textarea name="headers_json" placeholder='{"X-Tenant":"acme"}'></textarea></label>
                    <div class="actions"><app-button data-action="add-mcp">Add server</app-button></div>
                    <p class="error">{mcpError}</p>
                  </form>
                </app-card>
              </section>
            </div>
          </div>
        </div>
      </div>
    </>
  );
}
