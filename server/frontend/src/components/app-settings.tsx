import { Component, css, effect, onMount } from "@frontiers-labs/argon";
import {
  connectGitHubPat,
  createEmailAccount,
  createMcpServer,
  createProvider,
  createSkill,
  createUserModel,
  createWritableDir,
  deleteEmailAccount,
  deleteMemory,
  deleteMcpServer,
  deleteProvider,
  deleteSkill,
  deleteUserModel,
  deleteWritableDir,
  disconnectGitHub,
  disconnectGoogle,
  disconnectTelegram,
  getAgentSettings,
  getGitHubSettings,
  getGoogleSettings,
  getTelegramSettings,
  getThreadRetention,
  listEmailAccounts,
  listMemories,
  listMcpServers,
  listModels,
  listProviders,
  listSkills,
  listUserModels,
  listWritableDirs,
  loginTelegram,
  startGitHubAuthorize,
  startGoogleAuthorize,
  updateAgentSettings,
  updateSkill,
  updateThreadRetention,
  type AgentSettings,
  type EmailAccount,
  type Memory,
  type MemoryRoom,
  type MemorySettings,
  type MemoryWing,
  type ModelSummary,
  type McpServer,
  type ProviderSummary,
  type Skill,
  type TelegramAuthData,
  type UserModelSummary,
  type WritableDir,
} from "../api/settings.js";

type SettingsHost = HTMLElement & {
  activeSection: string;
  tgConfigured: boolean;
  tgConnected: boolean;
  tgStatus: string;
  tgBotUsername: string;
  tgError: string;
  ghConfigured: boolean;
  ghConnected: boolean;
  ghStatus: string;
  ghLogin: string;
  ghError: string;
  goConfigured: boolean;
  goConnected: boolean;
  goStatus: string;
  goEmail: string;
  goError: string;
  emails: EmailAccount[];
  emailLoaded: boolean;
  emailError: string;
  mcps: McpServer[];
  mcpLoaded: boolean;
  mcpError: string;
  skills: Skill[];
  skillLoaded: boolean;
  skillError: string;
  editingSkill: Skill | null;
  writableDirs: WritableDir[];
  writableDirLoaded: boolean;
  writableDirError: string;
  memoryWings: MemoryWing[];
  memoryRooms: MemoryRoom[];
  memories: Memory[];
  memoryLoaded: boolean;
  memoryError: string;
  memoryQuery: string;
  selectedMemoryId: string;
  retentionArchiveEnabled: boolean;
  retentionArchiveDays: number;
  retentionRemoveEnabled: boolean;
  retentionRemoveDays: number;
  retentionLoaded: boolean;
  retentionError: string;
  retentionSaved: boolean;
  availableModels: ModelSummary[];
  providers: ProviderSummary[];
  userModels: UserModelSummary[];
  modelsLoaded: boolean;
  modelsError: string;
  agentSettings: AgentSettings;
  agentSettingsLoaded: boolean;
  agentSettingsError: string;
  agentSettingsSaved: boolean;
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

type ModelItemView = { id: string; name: string; meta: string; badge?: string };

function modelSettingsMeta(model: {
  description: string;
  slug: string;
  provider: string;
  vision: boolean;
}): string {
  if (model.description.trim()) {
    return model.description;
  }
  return `${model.slug} · ${model.provider}${model.vision ? " · vision" : ""}`;
}

function configModelView(model: ModelSummary): ModelItemView {
  return {
    id: model.key,
    name: escapeHtml(model.display_name),
    meta: escapeHtml(modelSettingsMeta(model)),
    badge: "Server",
  };
}

function userModelItemView(model: UserModelSummary): AccountView {
  return {
    id: model.id,
    name: escapeHtml(model.display_name),
    meta: escapeHtml(
      modelSettingsMeta({
        description: model.description,
        slug: model.slug,
        provider: model.provider_name,
        vision: model.vision,
      }),
    ),
  };
}

type SubagentModelView = { key: string; label: string; checked: boolean };

function subagentModelView(model: ModelSummary, allowed: string[]): SubagentModelView {
  return {
    key: model.key,
    label: escapeHtml(model.display_name),
    checked: allowed.includes(model.key),
  };
}

type SkillView = { id: string; name: string; meta: string };

function skillView(skill: Skill): SkillView {
  return {
    id: skill.id,
    name: escapeHtml(skill.title),
    meta: escapeHtml(`${skill.name} · ${skill.description}`),
  };
}

function writableDirView(dir: WritableDir): AccountView {
  return {
    id: dir.id,
    name: escapeHtml(`/${dir.path}`),
    meta: "Writable by your agents, including every subdirectory.",
  };
}

type MemoryView = {
  id: string;
  title: string;
  path: string;
  summary: string;
  content: string;
  source: string;
  keywords: string;
  created: string;
  rawSearch: string;
};

type RoomView = {
  id: string;
  wing: string;
  name: string;
  description: string;
  memories: number;
};

type WingView = {
  id: string;
  name: string;
  memories: number;
  rooms: RoomView[];
};

function formatDate(seconds: number): string {
  if (!seconds) return "Unknown date";
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(seconds * 1000));
}

function memoryView(memory: Memory): MemoryView {
  const title = memory.title || memory.summary || "Untitled memory";
  const summary = memory.summary || memory.content.slice(0, 180);
  const source = memory.source || "Agent memory";
  return {
    id: memory.id,
    title: escapeHtml(title),
    path: escapeHtml(`${memory.wing} / ${memory.room}`),
    summary: escapeHtml(summary),
    content: escapeHtml(memory.content),
    source: escapeHtml(source),
    keywords: escapeHtml(memory.keywords),
    created: escapeHtml(formatDate(memory.created_at)),
    rawSearch: `${title} ${summary} ${memory.content} ${memory.wing} ${memory.room} ${memory.keywords}`.toLowerCase(),
  };
}

function roomView(room: MemoryRoom): RoomView {
  return {
    id: room.id,
    wing: escapeHtml(room.wing),
    name: escapeHtml(room.name),
    description: escapeHtml(room.description),
    memories: room.memories,
  };
}

function wingView(wing: MemoryWing, rooms: MemoryRoom[]): WingView {
  return {
    id: wing.id,
    name: escapeHtml(wing.name),
    memories: wing.memories,
    rooms: rooms.filter((room) => room.wing === wing.name).map(roomView),
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

async function refreshGitHub(host: SettingsHost): Promise<void> {
  try {
    const settings = await getGitHubSettings();
    host.ghError = "";
    host.ghConfigured = settings.configured;
    host.ghConnected = settings.connected;
    host.ghLogin = settings.login ?? "";
    if (settings.connected) {
      const via = settings.auth_method === "pat" ? " via personal access token" : "";
      host.ghStatus = settings.login
        ? `Connected as @${settings.login}${via}.`
        : "GitHub is connected.";
    } else if (settings.configured) {
      host.ghStatus = "GitHub is not connected.";
    } else {
      host.ghStatus = "Connect a personal access token below to enable GitHub tools.";
    }
  } catch {
    host.ghError = "Failed to load GitHub settings.";
  }
}

async function connectGitHubWithPat(host: SettingsHost, form: HTMLFormElement): Promise<void> {
  const data = new FormData(form);
  const token = String(data.get("token") ?? "").trim();
  host.ghError = "";
  if (!token) {
    host.ghError = "Enter a personal access token.";
    return;
  }
  try {
    await connectGitHubPat(token);
    form.reset();
    await refreshGitHub(host);
  } catch (error) {
    host.ghError = error instanceof Error ? error.message : "Failed to connect GitHub.";
  }
}

async function connectGitHub(host: SettingsHost): Promise<void> {
  host.ghError = "";
  try {
    window.location.assign(await startGitHubAuthorize());
  } catch {
    host.ghError = "Failed to start GitHub sign in.";
  }
}

async function refreshGoogle(host: SettingsHost): Promise<void> {
  try {
    const settings = await getGoogleSettings();
    host.goError = "";
    host.goConfigured = settings.configured;
    host.goConnected = settings.connected;
    host.goEmail = settings.email ?? "";
    if (!settings.configured) {
      host.goStatus = "Google is not configured on this server.";
    } else if (settings.connected) {
      host.goStatus = settings.email ? `Connected as ${settings.email}.` : "Google is connected.";
    } else {
      host.goStatus = "Google is not connected.";
    }
  } catch {
    host.goError = "Failed to load Google settings.";
  }
}

async function connectGoogle(host: SettingsHost): Promise<void> {
  host.goError = "";
  try {
    window.location.assign(await startGoogleAuthorize());
  } catch {
    host.goError = "Failed to start Google sign in.";
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

async function refreshSkills(host: SettingsHost): Promise<void> {
  try {
    host.skills = await listSkills();
    host.skillLoaded = true;
    host.skillError = "";
  } catch {
    host.skillError = "Failed to load skills.";
  }
}

async function refreshWritableDirs(host: SettingsHost): Promise<void> {
  try {
    host.writableDirs = await listWritableDirs();
    host.writableDirLoaded = true;
    host.writableDirError = "";
  } catch {
    host.writableDirError = "Failed to load writable directories.";
  }
}

async function refreshMemories(host: SettingsHost): Promise<void> {
  try {
    const settings: MemorySettings = await listMemories();
    host.memoryWings = settings.wings;
    host.memoryRooms = settings.rooms;
    host.memories = settings.memories;
    host.memoryLoaded = true;
    host.memoryError = "";
    if (host.selectedMemoryId && !settings.memories.some((memory) => memory.id === host.selectedMemoryId)) {
      host.selectedMemoryId = settings.memories[0]?.id ?? "";
    }
  } catch {
    host.memoryError = "Failed to load memories.";
  }
}

async function refreshRetention(host: SettingsHost): Promise<void> {
  try {
    const settings = await getThreadRetention();
    host.retentionArchiveEnabled = settings.archive_after_days != null;
    host.retentionArchiveDays = settings.archive_after_days ?? 14;
    host.retentionRemoveEnabled = settings.remove_after_days != null;
    host.retentionRemoveDays = settings.remove_after_days ?? 90;
    host.retentionLoaded = true;
    host.retentionError = "";
  } catch {
    host.retentionError = "Failed to load thread settings.";
  }
}

async function refreshModels(host: SettingsHost): Promise<void> {
  try {
    const [availableModels, providers, userModels, agentSettings] = await Promise.all([
      listModels(),
      listProviders(),
      listUserModels(),
      getAgentSettings(),
    ]);
    host.availableModels = availableModels;
    host.providers = providers;
    host.userModels = userModels;
    host.agentSettings = agentSettings;
    host.modelsLoaded = true;
    host.modelsError = "";
    host.agentSettingsLoaded = true;
    host.agentSettingsError = "";
  } catch {
    host.modelsError = "Failed to load model settings.";
  }
}

async function submitProvider(host: SettingsHost, form: HTMLFormElement): Promise<void> {
  const data = new FormData(form);
  host.modelsError = "";
  try {
    await createProvider({
      name: String(data.get("name") ?? "").trim(),
      kind: String(data.get("kind") ?? "").trim(),
      url: String(data.get("url") ?? "").trim(),
      token: String(data.get("token") ?? "").trim(),
    });
    form.reset();
    await refreshModels(host);
  } catch (error) {
    host.modelsError = error instanceof Error ? error.message : "Failed to add provider.";
  }
}

async function submitUserModel(host: SettingsHost, form: HTMLFormElement): Promise<void> {
  const data = new FormData(form);
  host.modelsError = "";
  try {
    await createUserModel({
      name: String(data.get("name") ?? "").trim(),
      slug: String(data.get("slug") ?? "").trim(),
      provider_id: String(data.get("provider_id") ?? "").trim(),
      display_name: String(data.get("display_name") ?? "").trim() || null,
      description: String(data.get("description") ?? "").trim() || null,
      reasoning_effort: String(data.get("reasoning_effort") ?? "").trim() || null,
      vision: data.get("vision") === "on",
    });
    form.reset();
    await refreshModels(host);
  } catch (error) {
    host.modelsError = error instanceof Error ? error.message : "Failed to add model.";
  }
}

async function submitAgentSettings(host: SettingsHost): Promise<void> {
  host.agentSettingsError = "";
  host.agentSettingsSaved = false;
  try {
    host.agentSettings = await updateAgentSettings(host.agentSettings);
    host.agentSettingsSaved = true;
  } catch (error) {
    host.agentSettingsError =
      error instanceof Error ? error.message : "Failed to save agent settings.";
  }
}

function toggleSubagentModel(host: SettingsHost, modelKey: string, enabled: boolean): void {
  const current = new Set(host.agentSettings.subagent_allowed_models);
  if (enabled) {
    current.add(modelKey);
  } else {
    current.delete(modelKey);
  }
  host.agentSettings = {
    ...host.agentSettings,
    subagent_allowed_models: [...current],
  };
}

async function submitRetention(host: SettingsHost): Promise<void> {
  host.retentionError = "";
  host.retentionSaved = false;
  const archiveDays = Math.min(3650, Math.max(1, Math.round(host.retentionArchiveDays) || 14));
  const removeDays = Math.min(3650, Math.max(1, Math.round(host.retentionRemoveDays) || 90));
  try {
    const saved = await updateThreadRetention({
      archive_after_days: host.retentionArchiveEnabled ? archiveDays : null,
      remove_after_days: host.retentionRemoveEnabled ? removeDays : null,
    });
    host.retentionArchiveEnabled = saved.archive_after_days != null;
    host.retentionArchiveDays = saved.archive_after_days ?? host.retentionArchiveDays;
    host.retentionRemoveEnabled = saved.remove_after_days != null;
    host.retentionRemoveDays = saved.remove_after_days ?? host.retentionRemoveDays;
    host.retentionSaved = true;
  } catch (error) {
    host.retentionError = error instanceof Error ? error.message : "Failed to save thread settings.";
  }
}

async function submitWritableDir(host: SettingsHost, form: HTMLFormElement): Promise<void> {
  const data = new FormData(form);
  host.writableDirError = "";
  try {
    await createWritableDir(String(data.get("path") ?? "").trim());
    form.reset();
    await refreshWritableDirs(host);
  } catch (error) {
    host.writableDirError = error instanceof Error ? error.message : "Failed to add directory.";
  }
}

async function submitSkill(host: SettingsHost, form: HTMLFormElement): Promise<void> {
  const data = new FormData(form);
  host.skillError = "";
  try {
    await createSkill({
      name: String(data.get("name") ?? "").trim(),
      title: String(data.get("title") ?? "").trim(),
      description: String(data.get("description") ?? "").trim(),
      content: String(data.get("content") ?? "").trim(),
    });
    form.reset();
    await refreshSkills(host);
  } catch (error) {
    host.skillError = error instanceof Error ? error.message : "Failed to add skill.";
  }
}

async function submitSkillEdit(host: SettingsHost, form: HTMLFormElement, id: string): Promise<void> {
  const data = new FormData(form);
  host.skillError = "";
  try {
    await updateSkill(id, {
      title: String(data.get("title") ?? "").trim(),
      description: String(data.get("description") ?? "").trim(),
      content: String(data.get("content") ?? "").trim(),
    });
    host.editingSkill = null;
    await refreshSkills(host);
  } catch (error) {
    host.skillError = error instanceof Error ? error.message : "Failed to update skill.";
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

  .layout[data-active="connections"] .tab[data-section="connections"],
  .layout[data-active="email"] .tab[data-section="email"],
  .layout[data-active="mcp"] .tab[data-section="mcp"],
  .layout[data-active="files"] .tab[data-section="files"],
  .layout[data-active="memories"] .tab[data-section="memories"],
  .layout[data-active="skills"] .tab[data-section="skills"],
  .layout[data-active="threads"] .tab[data-section="threads"],
  .layout[data-active="models"] .tab[data-section="models"] {
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

  .layout[data-active="connections"] .panel[data-panel="connections"],
  .layout[data-active="email"] .panel[data-panel="email"],
  .layout[data-active="mcp"] .panel[data-panel="mcp"],
  .layout[data-active="files"] .panel[data-panel="files"],
  .layout[data-active="memories"] .panel[data-panel="memories"],
  .layout[data-active="skills"] .panel[data-panel="skills"],
  .layout[data-active="threads"] .panel[data-panel="threads"],
  .layout[data-active="models"] .panel[data-panel="models"] {
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

  .memory-overview {
    display: grid;
    gap: 14px;
    grid-template-columns: repeat(3, minmax(0, 1fr));
  }

  .memory-stat {
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 12px;
  }

  .memory-stat .value {
    color: var(--foreground);
    font-size: 24px;
    font-weight: 650;
    line-height: 1;
  }

  .memory-stat .label {
    color: var(--muted-foreground);
    font-size: 11px;
    letter-spacing: 0.08em;
    margin-top: 7px;
    text-transform: uppercase;
  }

  .memory-workspace {
    align-items: start;
    display: grid;
    gap: 16px;
    grid-template-columns: minmax(220px, 0.8fr) minmax(0, 1.2fr);
  }

  .memory-map,
  .memory-ledger,
  .memory-detail {
    border: 1px solid var(--border);
    border-radius: 8px;
    min-width: 0;
  }

  .memory-map {
    background:
      linear-gradient(90deg, color-mix(in srgb, var(--border) 45%, transparent) 1px, transparent 1px) 0 0 / 24px 24px,
      linear-gradient(color-mix(in srgb, var(--border) 45%, transparent) 1px, transparent 1px) 0 0 / 24px 24px;
    padding: 14px;
  }

  .map-wing {
    display: grid;
    gap: 8px;
  }

  .map-wing + .map-wing {
    border-top: 1px solid color-mix(in srgb, var(--border) 70%, transparent);
    margin-top: 14px;
    padding-top: 14px;
  }

  .map-wing-head {
    align-items: baseline;
    display: flex;
    gap: 10px;
    justify-content: space-between;
  }

  .map-wing-name {
    color: var(--foreground);
    font-size: 13px;
    font-weight: 650;
    overflow-wrap: anywhere;
  }

  .map-wing-count {
    color: var(--muted-foreground);
    flex: 0 0 auto;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .map-room {
    align-items: center;
    color: var(--muted-foreground);
    display: grid;
    font-size: 12px;
    gap: 8px;
    grid-template-columns: 18px minmax(0, 1fr) auto;
    min-height: 24px;
  }

  .map-room::before {
    background: var(--background);
    border: 1px solid var(--border);
    border-radius: 999px;
    content: "";
    height: 7px;
    justify-self: center;
    width: 7px;
  }

  .map-room .room-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .map-room .room-count {
    color: var(--foreground);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .memory-tools {
    display: grid;
    gap: 12px;
    margin-bottom: 14px;
  }

  .memory-search {
    position: relative;
  }

  .memory-search input {
    padding-left: 34px;
  }

  .memory-search::before {
    color: var(--muted-foreground);
    content: "⌕";
    font-size: 19px;
    left: 12px;
    line-height: 1;
    position: absolute;
    top: 8px;
  }

  .memory-ledger {
    overflow: hidden;
  }

  .memory-row {
    background: transparent;
    border: 0;
    border-bottom: 1px solid var(--border);
    color: inherit;
    cursor: pointer;
    display: grid;
    gap: 5px;
    padding: 12px 14px;
    text-align: left;
    width: 100%;
  }

  .memory-row:last-child {
    border-bottom: 0;
  }

  .memory-row:hover,
  .memory-row[aria-current="true"] {
    background: var(--accent);
  }

  .memory-row-title {
    color: var(--foreground);
    font-size: 14px;
    font-weight: 650;
    overflow-wrap: anywhere;
  }

  .memory-row-path,
  .memory-row-summary {
    color: var(--muted-foreground);
    font-size: 12px;
    line-height: 1.45;
  }

  .memory-row-summary {
    display: -webkit-box;
    overflow: hidden;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
  }

  .memory-detail {
    display: grid;
    gap: 14px;
    padding: 14px;
  }

  .memory-detail-head {
    align-items: start;
    display: flex;
    gap: 14px;
    justify-content: space-between;
  }

  .memory-detail h3 {
    color: var(--foreground);
    font-size: 16px;
    line-height: 1.25;
    margin: 0;
    overflow-wrap: anywhere;
  }

  .memory-detail-meta {
    color: var(--muted-foreground);
    font-size: 12px;
    line-height: 1.5;
  }

  .memory-detail-content {
    color: var(--foreground);
    font-size: 13px;
    line-height: 1.55;
    max-height: 280px;
    overflow: auto;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }

  .memory-tags {
    color: var(--muted-foreground);
    font-size: 12px;
    line-height: 1.45;
    overflow-wrap: anywhere;
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

  .row-actions {
    display: flex;
    flex: 0 0 auto;
    gap: 8px;
  }

  .skill-content textarea {
    min-height: 200px;
    font-family:
      ui-monospace,
      SFMono-Regular,
      Menlo,
      monospace;
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

  .retention-row {
    align-items: center;
    display: flex;
    gap: 16px;
    justify-content: space-between;
  }

  .retention-info {
    min-width: 0;
  }

  .retention-info .name {
    color: var(--foreground);
    font-size: 14px;
    font-weight: 600;
  }

  .retention-info .desc {
    color: var(--muted-foreground);
    font-size: 12px;
    line-height: 1.45;
    margin-top: 3px;
  }

  .retention-days {
    align-items: center;
    color: var(--muted-foreground);
    display: flex;
    flex-wrap: wrap;
    font-size: 14px;
    gap: 8px;
    margin-top: 4px;
  }

  .retention-days input {
    height: 34px;
    text-align: right;
    width: 76px;
  }

  .retention-days.off {
    opacity: 0.5;
  }

  .saved {
    color: var(--muted-foreground);
    font-size: 13px;
  }

  .error {
    color: var(--destructive);
    font-size: 13px;
  }

  .error:empty {
    display: none;
  }

  .checkbox-row {
    align-items: center;
    display: flex;
    gap: 10px;
  }

  .checkbox-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .model-list {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .model-item {
    align-items: center;
    border: 1px solid var(--border);
    border-radius: 8px;
    display: flex;
    gap: 12px;
    justify-content: space-between;
    padding: 12px;
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

    .memory-overview,
    .memory-workspace {
      grid-template-columns: 1fr;
    }
  }
`;

const DEFAULT_AGENT_SETTINGS: AgentSettings = {
  subagent_allowed_models: [],
  subagent_guidelines: "",
  using_server_defaults: true,
  server_default_guidelines: "",
};

export function AppSettings({
  activeSection = "connections",
  tgConfigured = false,
  tgConnected = false,
  tgStatus = "Loading…",
  tgBotUsername = "",
  tgError = "",
  ghConfigured = false,
  ghConnected = false,
  ghStatus = "Loading…",
  ghLogin = "",
  ghError = "",
  goConfigured = false,
  goConnected = false,
  goStatus = "Loading…",
  goEmail = "",
  goError = "",
  emails = [],
  emailLoaded = false,
  emailError = "",
  mcps = [],
  mcpLoaded = false,
  mcpError = "",
  skills = [],
  skillLoaded = false,
  skillError = "",
  editingSkill = null,
  writableDirs = [],
  writableDirLoaded = false,
  writableDirError = "",
  memoryWings = [],
  memoryRooms = [],
  memories = [],
  memoryLoaded = false,
  memoryError = "",
  memoryQuery = "",
  selectedMemoryId = "",
  retentionArchiveEnabled = true,
  retentionArchiveDays = 14,
  retentionRemoveEnabled = true,
  retentionRemoveDays = 90,
  retentionLoaded = false,
  retentionError = "",
  retentionSaved = false,
  availableModels = [],
  providers = [],
  userModels = [],
  modelsLoaded = false,
  modelsError = "",
  agentSettings = DEFAULT_AGENT_SETTINGS,
  agentSettingsLoaded = false,
  agentSettingsError = "",
  agentSettingsSaved = false,
}: {
  activeSection?: string;
  tgConfigured?: boolean;
  tgConnected?: boolean;
  tgStatus?: string;
  tgBotUsername?: string;
  tgError?: string;
  ghConfigured?: boolean;
  ghConnected?: boolean;
  ghStatus?: string;
  ghLogin?: string;
  ghError?: string;
  goConfigured?: boolean;
  goConnected?: boolean;
  goStatus?: string;
  goEmail?: string;
  goError?: string;
  emails?: EmailAccount[];
  emailLoaded?: boolean;
  emailError?: string;
  mcps?: McpServer[];
  mcpLoaded?: boolean;
  mcpError?: string;
  skills?: Skill[];
  skillLoaded?: boolean;
  skillError?: string;
  editingSkill?: Skill | null;
  writableDirs?: WritableDir[];
  writableDirLoaded?: boolean;
  writableDirError?: string;
  memoryWings?: MemoryWing[];
  memoryRooms?: MemoryRoom[];
  memories?: Memory[];
  memoryLoaded?: boolean;
  memoryError?: string;
  memoryQuery?: string;
  selectedMemoryId?: string;
  retentionArchiveEnabled?: boolean;
  retentionArchiveDays?: number;
  retentionRemoveEnabled?: boolean;
  retentionRemoveDays?: number;
  retentionLoaded?: boolean;
  retentionError?: string;
  retentionSaved?: boolean;
  availableModels?: ModelSummary[];
  providers?: ProviderSummary[];
  userModels?: UserModelSummary[];
  modelsLoaded?: boolean;
  modelsError?: string;
  agentSettings?: AgentSettings;
  agentSettingsLoaded?: boolean;
  agentSettingsError?: string;
  agentSettingsSaved?: boolean;
}): Component {
  onMount(() => {
    (window as unknown as Record<string, unknown>).onTelegramAuth = (user: TelegramAuthData) => {
      void handleAuth(this, user);
    };
    void refreshTelegram(this);
    void refreshGitHub(this);
    void refreshGoogle(this);
    void refreshEmails(this);
    void refreshMcps(this);
    void refreshSkills(this);
    void refreshWritableDirs(this);
    void refreshMemories(this);
    void refreshRetention(this);
    void refreshModels(this);
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
  const configModelViews = availableModels.filter((model) => model.source === "config").map(configModelView);
  const providerViews = providers.map((provider) => ({
    id: provider.id,
    name: escapeHtml(provider.name),
    meta: escapeHtml(`${provider.kind} · ${provider.url}`),
  }));
  const userModelViews = userModels.map(userModelItemView);
  const subagentModelViews = availableModels.map((model) =>
    subagentModelView(model, agentSettings.subagent_allowed_models),
  );
  const providerOptions = providers
    .map((provider) => `<option value="${provider.id}">${escapeHtml(provider.name)}</option>`)
    .join("");
  const providerSelectHtml = `<option value="">Select provider</option>${providerOptions}`;
  const skillViews = skills.map(skillView);
  const writableDirViews = writableDirs.map(writableDirView);
  const query = memoryQuery.trim().toLowerCase();
  const memoryViews = memories.map(memoryView);
  const filteredMemories = query
    ? memoryViews.filter((memory) => memory.rawSearch.includes(query))
    : memoryViews;
  const selectedMemory = filteredMemories.find((memory) => memory.id === selectedMemoryId)
    ?? filteredMemories[0]
    ?? null;
  const totalRooms = memoryRooms.length;
  const totalMemories = memories.length;
  const memoryWingViews = memoryWings.map((wing) => wingView(wing, memoryRooms));
  const editing = editingSkill
    ? {
        id: editingSkill.id,
        title: escapeHtml(editingSkill.title),
        description: escapeHtml(editingSkill.description),
        content: escapeHtml(editingSkill.content),
      }
    : null;

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
            case "gh-connect":
              void connectGitHub(this);
              return;
            case "gh-pat": {
              const form = action.closest<HTMLFormElement>("form");
              if (form) void connectGitHubWithPat(this, form);
              return;
            }
            case "gh-disconnect":
              void disconnectGitHub()
                .then(() => refreshGitHub(this))
                .catch(() => {
                  this.ghError = "Failed to disconnect GitHub.";
                });
              return;
            case "go-connect":
              void connectGoogle(this);
              return;
            case "go-disconnect":
              void disconnectGoogle()
                .then(() => refreshGoogle(this))
                .catch(() => {
                  this.goError = "Failed to disconnect Google.";
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
            case "add-writable-dir": {
              const form = action.closest<HTMLFormElement>("form");
              if (form) void submitWritableDir(this, form);
              return;
            }
            case "del-writable-dir":
              if (action.dataset.id && window.confirm("Revoke write access to this directory?")) {
                void deleteWritableDir(action.dataset.id)
                  .then(() => refreshWritableDirs(this))
                  .catch(() => {
                    this.writableDirError = "Failed to remove directory.";
                  });
              }
              return;
            case "refresh-memories":
              void refreshMemories(this);
              return;
            case "save-retention":
              void submitRetention(this);
              return;
            case "add-provider": {
              const form = action.closest<HTMLFormElement>("form");
              if (form) void submitProvider(this, form);
              return;
            }
            case "add-user-model": {
              const form = action.closest<HTMLFormElement>("form");
              if (form) void submitUserModel(this, form);
              return;
            }
            case "save-agent-settings":
              void submitAgentSettings(this);
              return;
            case "del-provider":
              if (action.dataset.id && window.confirm("Remove this provider and its models?")) {
                void deleteProvider(action.dataset.id)
                  .then(() => refreshModels(this))
                  .catch(() => {
                    this.modelsError = "Failed to remove provider.";
                  });
              }
              return;
            case "del-user-model":
              if (action.dataset.id && window.confirm("Remove this model?")) {
                void deleteUserModel(action.dataset.id)
                  .then(() => refreshModels(this))
                  .catch(() => {
                    this.modelsError = "Failed to remove model.";
                  });
              }
              return;
            case "select-memory":
              this.selectedMemoryId = action.dataset.id ?? "";
              return;
            case "del-memory":
              if (action.dataset.id && window.confirm("Remove this memory? This cannot be undone.")) {
                void deleteMemory(action.dataset.id)
                  .then(() => refreshMemories(this))
                  .catch(() => {
                    this.memoryError = "Failed to remove memory.";
                  });
              }
              return;
            case "del-email":
              if (action.dataset.id && window.confirm("Remove this IMAP account from S.T.R.I.D.E.?")) {
                void deleteEmailAccount(action.dataset.id)
                  .then(() => refreshEmails(this))
                  .catch(() => {
                    this.emailError = "Failed to remove email account.";
                  });
              }
              return;
            case "del-mcp":
              if (action.dataset.id && window.confirm("Remove this MCP server from S.T.R.I.D.E.?")) {
                void deleteMcpServer(action.dataset.id)
                  .then(() => refreshMcps(this))
                  .catch(() => {
                    this.mcpError = "Failed to remove MCP server.";
                  });
              }
              return;
            case "add-skill": {
              const form = action.closest<HTMLFormElement>("form");
              if (form) void submitSkill(this, form);
              return;
            }
            case "edit-skill": {
              const skill = this.skills.find((item) => item.id === action.dataset.id);
              if (skill) {
                this.skillError = "";
                this.editingSkill = skill;
              }
              return;
            }
            case "save-skill": {
              const form = action.closest<HTMLFormElement>("form");
              if (form && this.editingSkill) void submitSkillEdit(this, form, this.editingSkill.id);
              return;
            }
            case "cancel-skill":
              this.skillError = "";
              this.editingSkill = null;
              return;
            case "del-skill":
              if (action.dataset.id && window.confirm("Remove this skill from S.T.R.I.D.E.?")) {
                void deleteSkill(action.dataset.id)
                  .then(() => {
                    if (this.editingSkill?.id === action.dataset.id) this.editingSkill = null;
                    return refreshSkills(this);
                  })
                  .catch(() => {
                    this.skillError = "Failed to remove skill.";
                  });
              }
              return;
          }
        }}
        onInput={(event: Event) => {
          const input = event.target as HTMLInputElement;
          if (input.name === "memory-query") {
            this.memoryQuery = input.value;
          }
          if (input.name === "archive-days") {
            this.retentionArchiveDays = Number(input.value);
            this.retentionSaved = false;
          }
          if (input.name === "remove-days") {
            this.retentionRemoveDays = Number(input.value);
            this.retentionSaved = false;
          }
          if (input.name === "subagent-guidelines") {
            this.agentSettings = {
              ...this.agentSettings,
              subagent_guidelines: input.value,
            };
            this.agentSettingsSaved = false;
          }
        }}
        onChange={(event: Event) => {
          const checkbox = (event.target as HTMLElement).closest<HTMLElement>("app-checkbox");
          if (checkbox?.dataset.model) {
            const checked = (event as CustomEvent<{ checked: boolean }>).detail?.checked;
            if (typeof checked === "boolean") {
              toggleSubagentModel(this, checkbox.dataset.model, checked);
              this.agentSettingsSaved = false;
            }
            return;
          }
          const wrap = (event.target as HTMLElement).closest<HTMLElement>("[data-switch]");
          if (!wrap) return;
          const checked = (event as CustomEvent<{ checked: boolean }>).detail?.checked;
          if (typeof checked !== "boolean") return;
          if (wrap.dataset.switch === "archive") this.retentionArchiveEnabled = checked;
          if (wrap.dataset.switch === "remove") this.retentionRemoveEnabled = checked;
          this.retentionSaved = false;
        }}
        onSubmit={(event: Event) => {
          event.preventDefault();
          const form = event.target as HTMLFormElement;
          if (form.dataset.form === "email") void submitEmail(this, form);
          if (form.dataset.form === "mcp") void submitMcp(this, form);
          if (form.dataset.form === "writable-dir") void submitWritableDir(this, form);
          if (form.dataset.form === "skill") void submitSkill(this, form);
          if (form.dataset.form === "skill-edit" && this.editingSkill) {
            void submitSkillEdit(this, form, this.editingSkill.id);
          }
          if (form.dataset.form === "provider") void submitProvider(this, form);
          if (form.dataset.form === "user-model") void submitUserModel(this, form);
        }}
      >
        <div class="shell">
          <header>
            <h1 class="page-title">Settings</h1>
            <p class="lead">Manage account integrations S.T.R.I.D.E. uses on your behalf.</p>
          </header>

          <div class="layout" data-active={activeSection}>
            <nav class="tabs" aria-label="Settings sections">
              <button type="button" class="tab" data-section="connections">Connections</button>
              <button type="button" class="tab" data-section="email">Email</button>
              <button type="button" class="tab" data-section="mcp">MCP servers</button>
              <button type="button" class="tab" data-section="files">Writable folders</button>
              <button type="button" class="tab" data-section="memories">Memories</button>
              <button type="button" class="tab" data-section="skills">Skills</button>
              <button type="button" class="tab" data-section="models">Models</button>
              <button type="button" class="tab" data-section="threads">Threads</button>
            </nav>

            <div class="panels">
              <section class="panel" data-panel="connections">
                <app-card title="Telegram" description="Connect your Telegram account with the S.T.R.I.D.E. bot.">
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

                <app-card title="GitHub" description="Give your agents the official GitHub MCP tools for repositories, issues, and pull requests. Sign in with GitHub, or paste a personal access token if your server has no GitHub app configured.">
                  <div class="status-row">
                    {ghConnected
                      ? <app-badge>Connected</app-badge>
                      : <app-badge variant="outline">Not connected</app-badge>}
                    <span class="status">{ghStatus}</span>
                  </div>
                  {ghConnected
                    ? <div><app-button variant="outline" data-action="gh-disconnect">Disconnect</app-button></div>
                    : (
                      <>
                        {ghConfigured
                          ? <div><app-button data-action="gh-connect">Sign in with GitHub</app-button></div>
                          : ""}
                        <form data-form="github-pat">
                          <label>
                            Personal access token
                            <input
                              name="token"
                              type="password"
                              placeholder="ghp_… or github_pat_…"
                              autocomplete="off"
                            />
                          </label>
                          <p class="muted">
                            Create a token with the scopes your agents need (for example <code>repo</code> and <code>read:org</code>) at github.com/settings/tokens. It is encrypted at rest and forwarded only to the GitHub MCP server.
                          </p>
                          <div class="actions"><app-button data-action="gh-pat">Connect with token</app-button></div>
                        </form>
                      </>
                    )}
                  <p class="error">{ghError}</p>
                </app-card>

                <app-card title="Google" description="Connect your Google account to give your agents native Gmail, Calendar, and Drive tools, and to trigger automations on new Gmail. Gmail is read and draft only — agents never send mail.">
                  <div class="status-row">
                    {goConnected
                      ? <app-badge>Connected</app-badge>
                      : goConfigured
                        ? <app-badge variant="outline">Not connected</app-badge>
                        : <app-badge variant="secondary">Unavailable</app-badge>}
                    <span class="status">{goStatus}</span>
                  </div>
                  {goConfigured
                    ? (goConnected
                      ? <div><app-button variant="outline" data-action="go-disconnect">Disconnect</app-button></div>
                      : <div><app-button data-action="go-connect">Sign in with Google</app-button></div>)
                    : ""}
                  <p class="error">{goError}</p>
                </app-card>
              </section>

              <section class="panel" data-panel="email">
                <app-card
                  title="Email accounts"
                  description="Connect one or more TLS IMAP accounts. S.T.R.I.D.E. can read incoming and sent mail and save reply-all drafts. It cannot send email."
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

              <section class="panel" data-panel="files">
                <app-card
                  title="Writable folders"
                  description="By default your agents may only write inside a thread's workspace or its project folder. Add personal folders here to let agents create and edit files in them. Every subfolder is included."
                >
                  {writableDirViews.length > 0
                    ? (
                      <div class="account-list">
                        {writableDirViews.map((dir) => (
                          <div class="account" key={dir.id}>
                            <div>
                              <div class="name">{dir.name}</div>
                              <div class="meta">{dir.meta}</div>
                            </div>
                            <app-button variant="outline" size="sm" data-action="del-writable-dir" data-id={dir.id}>Remove</app-button>
                          </div>
                        )).join("")}
                      </div>
                    )
                    : <p class="muted">{writableDirLoaded ? "No writable folders yet. Agents can still write to the thread workspace and project folders." : "Loading folders…"}</p>}
                </app-card>

                <app-card title="Add writable folder" description="Enter a path relative to your files, e.g. Documents or Notes/Personal. The folder and everything under it becomes writable.">
                  <form data-form="writable-dir">
                    <label class="full">Folder path<input name="path" required placeholder="Documents/Notes" autocomplete="off" /></label>
                    <div class="actions"><app-button data-action="add-writable-dir">Add folder</app-button></div>
                    <p class="error">{writableDirError}</p>
                  </form>
                </app-card>
              </section>

              <section class="panel" data-panel="memories">
                <app-card
                  title="Memory palace"
                  description="Review durable memories your agents can recall across threads. New memories are still created by asking the agent to remember something."
                >
                  <div class="memory-overview">
                    <div class="memory-stat">
                      <div class="value">{memoryWings.length}</div>
                      <div class="label">Wings</div>
                    </div>
                    <div class="memory-stat">
                      <div class="value">{totalRooms}</div>
                      <div class="label">Rooms</div>
                    </div>
                    <div class="memory-stat">
                      <div class="value">{totalMemories}</div>
                      <div class="label">Memories</div>
                    </div>
                  </div>
                  <div class="status-row">
                    <app-button variant="outline" size="sm" data-action="refresh-memories">Refresh</app-button>
                    <span class="muted">{memoryLoaded ? "Showing saved memory structure." : "Loading memories…"}</span>
                  </div>
                  <p class="error">{memoryError}</p>
                </app-card>

                <div class="memory-workspace">
                  <app-card title="Palace map" description="Wings hold rooms; rooms hold individual memories. Empty rooms stay visible so the structure is easy to audit.">
                    {memoryWings.length > 0
                      ? (
                        <div class="memory-map">
                          {memoryWingViews.map((wing) => (
                            <div class="map-wing" key={wing.id}>
                              <div class="map-wing-head">
                                <div class="map-wing-name">{wing.name}</div>
                                <div class="map-wing-count">{wing.memories} memories</div>
                              </div>
                              {wing.rooms.length > 0
                                ? wing.rooms.map((room) => (
                                  <div class="map-room" key={room.id} title={room.description}>
                                    <span class="room-name">{room.name}</span>
                                    <span class="room-count">{room.memories}</span>
                                  </div>
                                )).join("")
                                : <p class="muted">No rooms yet.</p>}
                            </div>
                          )).join("")}
                        </div>
                      )
                      : <p class="muted">{memoryLoaded ? "No memory wings yet." : "Loading palace map…"}</p>}
                  </app-card>

                  <div class="panels">
                    <app-card title="Memory ledger" description="Search titles, summaries, rooms, and original contents. Removing a memory deletes the saved drawer and its search card.">
                      <div class="memory-tools">
                        <label class="memory-search">
                          <input name="memory-query" value={memoryQuery} placeholder="Search memories" aria-label="Search memories" autocomplete="off" />
                        </label>
                        <span class="hint">{filteredMemories.length} of {totalMemories} memories shown</span>
                      </div>
                      {filteredMemories.length > 0
                        ? (
                          <div class="memory-ledger">
                            {filteredMemories.map((memory) => (
                              <button
                                type="button"
                                class="memory-row"
                                data-action="select-memory"
                                data-id={memory.id}
                                aria-current={selectedMemory?.id === memory.id ? "true" : "false"}
                              >
                                <span class="memory-row-title">{memory.title}</span>
                                <span class="memory-row-path">{memory.path}</span>
                                <span class="memory-row-summary">{memory.summary}</span>
                              </button>
                            )).join("")}
                          </div>
                        )
                        : <p class="muted">{memoryLoaded ? "No memories match this search." : "Loading memories…"}</p>}
                    </app-card>

                    <app-card title="Selected memory" description="Inspect the stored summary, original content, source, and search keywords before removing anything.">
                      {selectedMemory
                        ? (
                          <div class="memory-detail">
                            <div class="memory-detail-head">
                              <div>
                                <h3>{selectedMemory.title}</h3>
                                <div class="memory-detail-meta">{selectedMemory.path} · {selectedMemory.created}</div>
                              </div>
                              <app-button variant="outline" size="sm" data-action="del-memory" data-id={selectedMemory.id}>Remove</app-button>
                            </div>
                            <p class="muted">{selectedMemory.summary}</p>
                            <div class="memory-detail-content">{selectedMemory.content}</div>
                            <div class="memory-tags">Source: {selectedMemory.source}</div>
                            {selectedMemory.keywords ? <div class="memory-tags">Keywords: {selectedMemory.keywords}</div> : ""}
                          </div>
                        )
                        : <p class="muted">{memoryLoaded ? "Select a memory to inspect it." : "Loading selected memory…"}</p>}
                    </app-card>
                  </div>
                </div>
              </section>

              <section class="panel" data-panel="skills">
                <app-card
                  title="Skills"
                  description="Skills are reusable instruction sets your agents load on demand. Built-in skills are always available and are not listed here."
                >
                  {skillViews.length > 0
                    ? (
                      <div class="account-list">
                        {skillViews.map((skill) => (
                          <div class="account" key={skill.id}>
                            <div>
                              <div class="name">{skill.name}</div>
                              <div class="meta">{skill.meta}</div>
                            </div>
                            <div class="row-actions">
                              <app-button variant="outline" size="sm" data-action="edit-skill" data-id={skill.id}>Edit</app-button>
                              <app-button variant="outline" size="sm" data-action="del-skill" data-id={skill.id}>Remove</app-button>
                            </div>
                          </div>
                        )).join("")}
                      </div>
                    )
                    : <p class="muted">{skillLoaded ? "No skills yet." : "Loading skills…"}</p>}
                </app-card>

                {editing
                  ? (
                    <app-card title="Edit skill" description="Update the title, description, or content. The skill name cannot be changed.">
                      <form data-form="skill-edit">
                        <label class="full">Title<input name="title" required value={editing.title} autocomplete="off" /></label>
                        <label class="full">Description<input name="description" required value={editing.description} autocomplete="off" /></label>
                        <label class="full skill-content">Content<textarea name="content" required>{editing.content}</textarea></label>
                        <div class="actions">
                          <app-button data-action="save-skill" data-id={editing.id}>Save changes</app-button>
                          <app-button variant="outline" data-action="cancel-skill">Cancel</app-button>
                        </div>
                        <p class="error">{skillError}</p>
                      </form>
                    </app-card>
                  )
                  : (
                    <app-card title="Add skill" description="The name is a unique slug, e.g. python-debugging. Content is Markdown instructions the agent follows when this skill is active.">
                      <form data-form="skill">
                        <label class="full">Name<input name="name" required placeholder="python-debugging" autocomplete="off" pattern="[a-z][a-z0-9-]{1,63}" /></label>
                        <label class="full">Title<input name="title" required placeholder="Python Debugging Guide" autocomplete="off" /></label>
                        <label class="full">Description<input name="description" required placeholder="One or two sentence summary used for search." autocomplete="off" /></label>
                        <label class="full skill-content">Content<textarea name="content" required placeholder="Markdown instructions, context, or steps the agent should follow."></textarea></label>
                        <div class="actions"><app-button data-action="add-skill">Add skill</app-button></div>
                        <p class="error">{skillError}</p>
                      </form>
                    </app-card>
                  )}
              </section>

              <section class="panel" data-panel="models">
                <app-card title="Server models" description="Add chat models in config.toml under [models.&lt;key&gt;]. Set display_name for labels in the composer and description for this list. Reserved keys embeddings, transcription, title_generator, expert, and explorer are internal and not shown here.">
                  {configModelViews.length > 0
                    ? (
                      <div class="model-list">
                        {configModelViews.map((model) => (
                          <div class="model-item">
                            <div>
                              <div class="name">{model.name}</div>
                              <div class="desc">{model.meta}</div>
                            </div>
                            <app-badge variant="secondary">{model.badge}</app-badge>
                          </div>
                        ))}
                      </div>
                    )
                    : <p class="muted">{modelsLoaded ? "No server models are configured." : "Loading models…"}</p>}
                  <p class="muted">Example: duplicate a [models.*] block in config.toml.example, set display_name and description, then restart the server.</p>
                </app-card>

                <app-card title="Providers" description="Add your own LLM provider credentials. Models you define below will use these providers.">
                  {providerViews.length > 0
                    ? (
                      <div class="model-list">
                        {providerViews.map((provider) => (
                          <div class="model-item">
                            <div>
                              <div class="name">{provider.name}</div>
                              <div class="desc">{provider.meta}</div>
                            </div>
                            <app-button variant="outline" size="sm" data-action="del-provider" data-id={provider.id}>Remove</app-button>
                          </div>
                        ))}
                      </div>
                    )
                    : <p class="muted">{modelsLoaded ? "No personal providers yet." : "Loading providers…"}</p>}
                  <form data-form="provider">
                    <div class="grid">
                      <label>Name<input name="name" required placeholder="my_openai" autocomplete="off" pattern="[A-Za-z0-9_-]+" /></label>
                      <label>Kind<select name="kind" required>
                        <option value="openai">OpenAI</option>
                        <option value="openrouter">OpenRouter</option>
                        <option value="anthropic">Anthropic</option>
                        <option value="ollama">Ollama</option>
                        <option value="ollama_cloud">Ollama Cloud</option>
                      </select></label>
                      <label class="full">URL<input name="url" type="url" required placeholder="https://api.openai.com/v1" autocomplete="off" /></label>
                      <label class="full">API token<input name="token" type="password" required autocomplete="off" /></label>
                    </div>
                    <div class="actions"><app-button data-action="add-provider">Add provider</app-button></div>
                  </form>
                </app-card>

                <app-card title="Personal models" description="Define models that use your providers. The registry key is internal; display_name is shown in the composer and description appears here.">
                  {userModelViews.length > 0
                    ? (
                      <div class="model-list">
                        {userModelViews.map((model) => (
                          <div class="model-item">
                            <div>
                              <div class="name">{model.name}</div>
                              <div class="desc">{model.meta}</div>
                            </div>
                            <app-button variant="outline" size="sm" data-action="del-user-model" data-id={model.id}>Remove</app-button>
                          </div>
                        ))}
                      </div>
                    )
                    : <p class="muted">{modelsLoaded ? "No personal models yet." : "Loading models…"}</p>}
                  <form data-form="user-model">
                    <div class="grid">
                      <label>Registry key<input name="name" required placeholder="my_sonnet" autocomplete="off" pattern="[A-Za-z0-9_-]+" /></label>
                      <label>Display name<input name="display_name" placeholder="Claude Sonnet" autocomplete="off" /></label>
                      <label class="full">Description<textarea name="description" placeholder="When to use this model." rows="2"></textarea></label>
                      <label>Model slug<input name="slug" required placeholder="claude-sonnet-4-20250514" autocomplete="off" /></label>
                      <label>Provider<select name="provider_id" required innerHTML={providerSelectHtml}></select></label>
                      <label>Reasoning effort<select name="reasoning_effort">
                        <option value="">Disabled</option>
                        <option value="low">Low</option>
                        <option value="medium">Medium</option>
                        <option value="high">High</option>
                        <option value="xhigh">XHigh</option>
                      </select></label>
                      <label class="checkbox-row"><app-checkbox name="vision" value="on" /> Supports vision</label>
                    </div>
                    <div class="actions"><app-button data-action="add-user-model">Add model</app-button></div>
                  </form>
                </app-card>

                <app-card title="Subagent settings" description="Control which models the main agent can use when spawning subagents, and add guidance on when to pick each one. Users without saved settings inherit the server default routing guide from config.">
                  <p class="muted">Allowed subagent models</p>
                  <div class="checkbox-list">
                    {subagentModelViews.map((model) => (
                      <label class="checkbox-row">
                        <app-checkbox data-model={model.key} checked={model.checked} />
                        <span>{model.label}</span>
                      </label>
                    ))}
                  </div>
                  <label class="full skill-content">Model selection guidelines
                    <textarea
                      name="subagent-guidelines"
                      placeholder="Describe when to use faster vs stronger models, cost constraints, or task-specific preferences."
                    >{escapeHtml(agentSettings.subagent_guidelines)}</textarea>
                  </label>
                  {agentSettings.using_server_defaults
                    ? <p class="muted">Showing the server default from config. Save to keep your own copy; your settings will not change when admins update config.</p>
                    : <p class="muted">Using your saved settings.</p>}
                  <div class="actions">
                    <app-button data-action="save-agent-settings">Save agent settings</app-button>
                    {agentSettingsSaved ? <span class="saved">Saved.</span> : ""}
                  </div>
                  <p class="error">{agentSettingsError}</p>
                </app-card>

                <p class="error">{modelsError}</p>
              </section>

              <section class="panel" data-panel="threads">
                <app-card
                  title="Auto-archive"
                  description="Archive threads automatically once they have been inactive for a while. Archived threads leave the sidebar but keep all messages and files, and can be restored anytime."
                >
                  <div class="retention-row">
                    <div class="retention-info">
                      <div class="name">Archive inactive threads</div>
                      <div class="desc">Turn this off to keep every thread in the sidebar until you archive it yourself.</div>
                    </div>
                    <span data-switch="archive"><app-switch checked={retentionArchiveEnabled}></app-switch></span>
                  </div>
                  <div class={retentionArchiveEnabled ? "retention-days" : "retention-days off"}>
                    <span>Archive after</span>
                    <input name="archive-days" type="number" min="1" max="3650" value={String(retentionArchiveDays)} autocomplete="off" />
                    <span>days of inactivity</span>
                  </div>
                </app-card>

                <app-card
                  title="Auto-remove"
                  description="Permanently delete an archived thread — including its workspace files and version history — once it has stayed archived long enough. This cannot be undone."
                >
                  <div class="retention-row">
                    <div class="retention-info">
                      <div class="name">Delete old archived threads</div>
                      <div class="desc">Turn this off to keep archived threads until you delete them yourself.</div>
                    </div>
                    <span data-switch="remove"><app-switch checked={retentionRemoveEnabled}></app-switch></span>
                  </div>
                  <div class={retentionRemoveEnabled ? "retention-days" : "retention-days off"}>
                    <span>Delete after</span>
                    <input name="remove-days" type="number" min="1" max="3650" value={String(retentionRemoveDays)} autocomplete="off" />
                    <span>days since archival</span>
                  </div>
                </app-card>

                <div class="status-row actions">
                  <app-button data-action="save-retention">Save changes</app-button>
                  {retentionSaved ? <span class="saved">Saved.</span> : ""}
                  {retentionLoaded ? "" : <span class="saved">Loading…</span>}
                </div>
                <p class="error">{retentionError}</p>
              </section>
            </div>
          </div>
        </div>
      </div>
    </>
  );
}
