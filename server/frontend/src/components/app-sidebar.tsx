/*
 * Design and functionality adapted from shadcn/ui sidebar component.
 * shadcn/ui — MIT License — Copyright (c) 2023 shadcn
 * https://ui.shadcn.com/docs/components/sidebar
 */
import { Component, css, emit, onMount, state } from "@frontiers-labs/argon";
import { settings } from "../stores/settings.js";
import { sidebar } from "../stores/ui.js";
import { AppAvatar } from "./app-avatar.js";
import { AppButton } from "./app-button.js";
import {
  AppSidebarContent,
  AppSidebarFooter,
  AppSidebarGroup,
  AppSidebarGroupContent,
  AppSidebarGroupLabel,
  AppSidebarHeader,
  AppSidebarMenu,
  AppSidebarMenuAction,
  AppSidebarMenuButton,
  AppSidebarMenuItem,
  AppSidebarPanel,
  AppSidebarRail,
} from "./app-sidebar-primitives.js";
import { IconArchive } from "./icons/archive.js";
import { IconMessagesSquare } from "./icons/messages-square.js";
import { IconChevronDown } from "./icons/chevron-down.js";
import { IconChevronRight } from "./icons/chevron-right.js";
import { IconChevronsUpDown } from "./icons/chevrons-up-down.js";
import { IconFolder } from "./icons/folder.js";
import { IconPanelLeftClose } from "./icons/panel-left-close.js";
import { IconPanelLeftOpen } from "./icons/panel-left-open.js";
import { IconPlus } from "./icons/plus.js";
import { IconSettingsHorizontal } from "./icons/settings-horizontal.js";
import { IconClock } from "./icons/clock.js";
import { IconStrideMark } from "./icons/stride-mark.js";

const styles = css`
  :host {
    --sidebar-width: 260px;
    --sidebar-width-icon: 48px;
    display: block;
    height: 100%;
    width: fit-content;
  }
  app-sidebar-panel {
    --sidebar-width: 260px;
    --sidebar-width-icon: 48px;
  }
  .scrim {
    border: 0;
    display: none;
    padding: 0;
  }
  .brand {
    align-items: center;
    display: flex;
    gap: 8px;
    min-width: 0;
  }
  .mark {
    align-items: center;
    background: var(--primary);
    border-radius: 8px;
    color: var(--primary-foreground);
    display: inline-flex;
    flex: 0 0 auto;
    height: 32px;
    justify-content: center;
    width: 32px;
  }
  .brand strong {
    color: var(--foreground);
    flex: 1;
    font-size: 14px;
    font-weight: 600;
    min-width: 0;
  }
  app-sidebar-panel[state="collapsed"] .mark,
  app-sidebar-panel[state="collapsed"] .brand strong,
  app-sidebar-panel[state="collapsed"] .groups {
    display: none;
  }
  .icon {
    align-items: center;
    display: inline-flex;
    flex: 0 0 16px;
    height: 16px;
    justify-content: center;
    width: 16px;
  }
  .icon > * {
    height: 16px;
    width: 16px;
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .nav-action-item {
    box-sizing: border-box;
    list-style: none;
    padding: 0 8px;
    width: 100%;
  }
  .nav-action {
    align-items: center;
    background: transparent;
    border: 0;
    border-radius: 6px;
    box-sizing: border-box;
    color: var(--sidebar-fg, var(--foreground));
    cursor: pointer;
    display: flex;
    font: inherit;
    font-size: 14px;
    gap: 8px;
    height: 32px;
    outline: none;
    overflow: hidden;
    padding: 0 8px;
    text-align: left;
    white-space: nowrap;
    width: 100%;
  }
  .nav-action:hover,
  .nav-action:focus-visible {
    background: var(--sidebar-accent, var(--accent));
  }
  .nav-action:focus-visible {
    box-shadow: 0 0 0 2px var(--ring-shadow, rgb(24 24 27 / 12%));
  }
  app-sidebar-panel[state="collapsed"] .nav-action {
    width: 32px;
  }
  app-sidebar-panel[state="collapsed"] .nav-action .label {
    display: none;
  }
  .groups {
    width: 100%;
  }
  .group-title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chevron {
    align-items: center;
    color: var(--muted-foreground);
    display: inline-flex;
    flex: 0 0 16px;
    height: 16px;
    justify-content: center;
    opacity: 0;
    transition: opacity 140ms ease;
    width: 16px;
  }
  app-sidebar-group-label:hover .chevron,
  app-sidebar-group-label:focus-within .chevron {
    opacity: 1;
  }
  .chevron > *,
  .chevron > * > * {
    align-items: center;
    display: inline-flex;
    height: 16px;
    justify-content: center;
    width: 16px;
  }
  .chevron .closed-mark,
  app-sidebar-group.closed .chevron .open-mark {
    display: none;
  }
  app-sidebar-group.closed .chevron .closed-mark {
    display: inline-flex;
  }
  .thread-label {
    align-items: center;
    display: flex;
    gap: 7px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .run-pulse {
    animation: run-pulse 1.6s ease-out infinite;
    background: var(--primary);
    border-radius: 999px;
    box-shadow: 0 0 0 0 color-mix(in srgb, var(--primary) 35%, transparent);
    display: inline-block;
    flex: 0 0 6px;
    height: 6px;
    width: 6px;
  }
  .thread-menu {
    background: var(--sidebar-bg, var(--secondary));
    display: none;
    position: absolute;
    right: 4px;
    top: 3px;
  }
  app-sidebar-menu-item:hover .thread-menu,
  .thread-menu[aria-expanded="true"] {
    display: inline-flex;
  }
  @keyframes run-pulse {
    70% {
      box-shadow: 0 0 0 5px transparent;
    }
    100% {
      box-shadow: 0 0 0 0 transparent;
    }
  }
  @media (max-width: 767px) {
    :host([hydrated]) app-sidebar-panel[state="open"] .scrim {
      background: rgb(0 0 0 / 36%);
      display: block;
      inset: 0;
      position: fixed;
      z-index: -1;
    }
    .icon {
      flex-basis: 20px;
      height: 20px;
      width: 20px;
    }
    .icon > * {
      height: 20px;
      width: 20px;
    }
    .thread-menu {
      display: inline-flex;
      top: 8px;
    }
    .nav-action {
      font-size: 16px;
      height: 44px;
      padding: 0 12px;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .chevron {
      transition: none;
    }
    .run-pulse {
      animation: none;
    }
  }
`;

const accountFooterStyles = css`
  .account-footer {
    display: block;
    position: relative;
    width: 100%;
  }

  .trigger {
    align-items: center;
    background: transparent;
    border: 0;
    border-radius: 8px;
    box-sizing: border-box;
    color: var(--sidebar-fg, var(--foreground));
    cursor: pointer;
    display: flex;
    font: inherit;
    gap: 10px;
    min-height: 48px;
    outline: none;
    padding: 8px;
    text-align: left;
    width: 100%;
  }

  .trigger:hover,
  .trigger:focus-visible,
  .trigger[aria-expanded="true"] {
    background: var(--sidebar-accent, var(--accent));
  }

  .trigger:focus-visible {
    box-shadow: 0 0 0 2px var(--ring-shadow, rgb(24 24 27 / 12%));
  }

  .identity {
    display: grid;
    flex: 1;
    line-height: 1.25;
    min-width: 0;
  }

  .name,
  .username {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .name {
    font-size: 14px;
    font-weight: 600;
  }

  .username {
    color: var(--muted-foreground);
    font-size: 12px;
  }

  .selector {
    align-items: center;
    color: var(--muted-foreground);
    display: inline-flex;
    justify-content: center;
    margin-left: auto;
  }

  .menu {
    background: var(--popover, var(--background));
    border: 1px solid var(--border);
    border-radius: 8px;
    bottom: calc(100% + 4px);
    box-shadow: 0 8px 24px rgb(0 0 0 / 12%);
    box-sizing: border-box;
    display: grid;
    gap: 2px;
    left: 0;
    padding: 4px;
    position: absolute;
    right: 0;
    z-index: 80;
  }

  .menu[hidden] {
    display: none;
  }

  .menu a,
  .menu button {
    align-items: center;
    background: transparent;
    border: 0;
    border-radius: 6px;
    box-sizing: border-box;
    color: var(--popover-foreground, var(--foreground));
    cursor: pointer;
    display: flex;
    font: inherit;
    font-size: 14px;
    gap: 8px;
    height: 34px;
    outline: none;
    padding: 0 8px;
    text-align: left;
    text-decoration: none;
    width: 100%;
  }

  .menu a:hover,
  .menu a:focus-visible,
  .menu button:hover,
  .menu button:focus-visible {
    background: var(--accent);
  }

  .menu .icon,
  .menu .icon > * {
    align-items: center;
    display: inline-flex;
    height: 16px;
    justify-content: center;
    width: 16px;
  }

  app-sidebar-panel[state="collapsed"] .account-footer .trigger {
    height: 32px;
    min-height: 32px;
    padding: 0;
    width: 32px;
  }

  app-sidebar-panel[state="collapsed"] .account-footer .identity,
  app-sidebar-panel[state="collapsed"] .account-footer .selector {
    display: none;
  }

  @media (max-width: 767px) {
    .trigger {
      min-height: 52px;
    }

    .menu a,
    .menu button {
      height: 42px;
      font-size: 16px;
    }
  }
`;

const toggleStyles = css`
  :host {
    display: inline-flex;
  }
  icon-panel-left-open,
  icon-panel-left-close {
    height: 16px;
    width: 16px;
  }
  .brand-mark {
    align-items: center;
    background: var(--primary);
    border-radius: 8px;
    color: var(--primary-foreground);
    display: inline-flex;
    height: 32px;
    justify-content: center;
    width: 32px;
  }
  .hover-icon {
    display: none;
  }
  .with-brand:hover .brand-mark,
  .with-brand:focus-within .brand-mark {
    display: none;
  }
  .with-brand:hover .hover-icon,
  .with-brand:focus-within .hover-icon {
    align-items: center;
    display: inline-flex;
    height: 16px;
    justify-content: center;
    width: 16px;
  }
`;

const navigationItemStyles = css`
  :host {
    display: block;
    width: 100%;
  }
  .icon {
    align-items: center;
    display: inline-flex;
    flex: 0 0 16px;
    height: 16px;
    justify-content: center;
    width: 16px;
  }
  .icon > * {
    height: 16px;
    width: 16px;
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  @media (max-width: 767px) {
    .icon {
      flex-basis: 20px;
      height: 20px;
      width: 20px;
    }
    .icon > * {
      height: 20px;
      width: 20px;
    }
  }
`;

const threadGroupStyles = css`
  :host {
    display: block;
    width: 100%;
  }
  .group-title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chevron {
    align-items: center;
    color: var(--muted-foreground);
    display: inline-flex;
    flex: 0 0 16px;
    height: 16px;
    justify-content: center;
    opacity: 0;
    transition: opacity 140ms ease;
    width: 16px;
  }
  app-sidebar-group-label:hover .chevron,
  app-sidebar-group-label:focus-within .chevron {
    opacity: 1;
  }
  .chevron > *,
  .chevron > * > * {
    align-items: center;
    display: inline-flex;
    height: 16px;
    justify-content: center;
    width: 16px;
  }
  .chevron .closed-mark,
  app-sidebar-group.closed .chevron .open-mark {
    display: none;
  }
  app-sidebar-group.closed .chevron .closed-mark {
    display: inline-flex;
  }
  .thread-label {
    align-items: center;
    display: flex;
    gap: 7px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .run-pulse {
    animation: run-pulse 1.6s ease-out infinite;
    background: var(--primary);
    border-radius: 999px;
    box-shadow: 0 0 0 0 color-mix(in srgb, var(--primary) 35%, transparent);
    display: inline-block;
    flex: 0 0 6px;
    height: 6px;
    width: 6px;
  }
  .thread-menu-wrap {
    display: none;
    position: absolute;
    right: 4px;
    top: 3px;
  }
  app-sidebar-menu-item:hover .thread-menu-wrap,
  .thread-menu-wrap:focus-within {
    display: inline-flex;
  }
  @keyframes run-pulse {
    70% {
      box-shadow: 0 0 0 5px transparent;
    }
    100% {
      box-shadow: 0 0 0 0 transparent;
    }
  }
  @media (max-width: 767px) {
    .thread-menu-wrap {
      display: inline-flex;
      top: 8px;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .chevron {
      transition: none;
    }
    .run-pulse {
      animation: none;
    }
  }
`;

export interface SidebarThread {
  id: string;
  title: string;
}
export interface SidebarProject {
  id: string;
  title: string;
  threads: SidebarThread[];
}

interface SidebarHost extends HTMLElement {
  projects: SidebarProject[];
  threads: SidebarThread[];
}

interface UserEvent {
  id: string;
  kind:
    | {
        type: "thread_created";
        thread_id: string;
        title: string;
        project_id: string | null;
      }
    | { type: "thread_renamed"; thread_id: string; title: string }
    | { type: "thread_archived" | "thread_deleted"; thread_id: string }
    | { type: "thread_restored" | "resync"; thread_id?: string }
    | { type: "thread_run_status"; thread_id: string; running: boolean }
    | {
        type: "notification";
        notification_id: string;
        title: string;
        message: string;
        thread_id: string | null;
      };
}

function removeThread(host: SidebarHost, threadId: string): void {
  host.threads = host.threads.filter((thread) => thread.id !== threadId);
  host.projects = host.projects.map((project) => ({
    ...project,
    threads: project.threads.filter((thread) => thread.id !== threadId),
  }));
}

function applyUserEvent(host: SidebarHost, event: UserEvent): void {
  const kind = event.kind;
  if (kind.type === "thread_created") {
    removeThread(host, kind.thread_id);
    const thread = { id: kind.thread_id, title: kind.title };
    const project = kind.project_id
      ? host.projects.find((candidate) => candidate.id === kind.project_id)
      : undefined;
    if (project)
      host.projects = host.projects.map((candidate) =>
        candidate.id === project.id
          ? { ...candidate, threads: [thread, ...candidate.threads] }
          : candidate,
      );
    else host.threads = [thread, ...host.threads];
    return;
  }
  if (kind.type === "thread_renamed") {
    host.threads = host.threads.map((thread) =>
      thread.id === kind.thread_id ? { ...thread, title: kind.title } : thread,
    );
    host.projects = host.projects.map((project) => ({
      ...project,
      threads: project.threads.map((thread) =>
        thread.id === kind.thread_id
          ? { ...thread, title: kind.title }
          : thread,
      ),
    }));
    return;
  }
  if (kind.type === "thread_archived" || kind.type === "thread_deleted") {
    removeThread(host, kind.thread_id);
    return;
  }
  if (kind.type === "thread_run_status") {
    sidebar.runningThreads = kind.running
      ? [...new Set([...sidebar.runningThreads, kind.thread_id])]
      : sidebar.runningThreads.filter((id) => id !== kind.thread_id);
    return;
  }
  if (kind.type !== "notification") void resyncThreads(host);
}

async function resyncThreads(host: SidebarHost): Promise<void> {
  const response = await fetch("/api/threads");
  if (!response.ok) return;
  const threads = (await response.json()) as {
    id: string;
    title: string;
    project_id: string | null;
  }[];
  host.projects = host.projects.map((project) => ({
    ...project,
    threads: threads
      .filter((thread) => thread.project_id === project.id)
      .map(({ id, title }) => ({ id, title })),
  }));
  const projectIds = new Set(host.projects.map((project) => project.id));
  host.threads = threads
    .filter(
      (thread) => !thread.project_id || !projectIds.has(thread.project_id),
    )
    .map(({ id, title }) => ({ id, title }));
}

const MOBILE_QUERY = "(max-width: 767px)";

function toggleSidebar(): void {
  if (window.matchMedia(MOBILE_QUERY).matches)
    sidebar.status = sidebar.status === "open" ? "hidden" : "open";
  else sidebar.status = sidebar.status === "collapsed" ? "open" : "collapsed";
}

export function SidebarNavigationItem({
  href,
  label,
  kind,
  active = false,
  collapsed = false,
}: {
  href: string;
  label: string;
  kind: string;
  active?: boolean;
  collapsed?: boolean;
}): Component {
  return (
    <>
      <style>{navigationItemStyles}</style>
      <AppSidebarMenuItem>
        <AppSidebarMenuButton href={href} active={active} collapsed={collapsed}>
          <span class="icon">
            {kind === "tasks" ? (
              <IconMessagesSquare />
            ) : kind === "files" ? (
              <IconFolder />
            ) : kind === "automations" ? (
              <IconClock />
            ) : (
              <IconArchive />
            )}
          </span>
          <span class="label">{label}</span>
        </AppSidebarMenuButton>
      </AppSidebarMenuItem>
    </>
  );
}

function profileInitials(fullName: string, username: string): string {
  const source = fullName.trim() || username.trim();
  if (source.length === 0) return "?";
  return source.slice(0, 1).toUpperCase();
}

export function SidebarThreadGroup({
  title,
  threads = [],
  projectId = "",
  projectTitle = "",
}: {
  title: string;
  threads?: SidebarThread[];
  projectId?: string;
  projectTitle?: string;
}): Component {
  let closed = state(false);
  return (
    <>
      <style>{threadGroupStyles}</style>
      <AppSidebarGroup class={closed ? "closed" : ""}>
        <AppSidebarGroupLabel on:toggle={() => (closed = !closed)}>
          <span class="group-title">{title}</span>
          <span class="chevron" aria-hidden="true">
            <span class="open-mark">
              <IconChevronDown />
            </span>
            <span class="closed-mark">
              <IconChevronRight />
            </span>
          </span>
          {projectId !== "" && (
            <>
              <AppSidebarMenuAction
                slot="actions"
                small
                title="New thread"
                on:select={() =>
                  emit(this, "project-new-thread", {
                    id: projectId,
                    title: projectTitle,
                  })
                }
              >
                +
              </AppSidebarMenuAction>
              <AppSidebarMenuAction
                slot="actions"
                small
                title="Rename"
                on:select={() =>
                  emit(this, "project-rename", {
                    id: projectId,
                    title: projectTitle,
                  })
                }
              >
                ✎
              </AppSidebarMenuAction>
              <AppSidebarMenuAction
                slot="actions"
                small
                title="Delete"
                on:select={() =>
                  emit(this, "project-delete", { id: projectId })
                }
              >
                ✕
              </AppSidebarMenuAction>
            </>
          )}
        </AppSidebarGroupLabel>
        <AppSidebarGroupContent hidden={closed}>
          <AppSidebarMenu>
            {threads.map((thread) => (
              <AppSidebarMenuItem key={thread.id}>
                <AppSidebarMenuButton
                  compact
                  href={`/threads/${thread.id}`}
                  active={thread.id === sidebar.activeThread}
                >
                  <span class="thread-label">
                    {sidebar.runningThreads.includes(thread.id) && (
                      <span class="run-pulse" title="Running" />
                    )}
                    {thread.title}
                  </span>
                </AppSidebarMenuButton>
                <span class="thread-menu-wrap">
                  <AppSidebarMenuAction
                    title="Thread actions"
                    aria-label="Thread actions"
                    on:select={(event: Event) =>
                      emit(this, "thread-menu", {
                        id: thread.id,
                        title: thread.title,
                        anchor: event.currentTarget as HTMLElement,
                      })
                    }
                  >
                    ⋯
                  </AppSidebarMenuAction>
                </span>
              </AppSidebarMenuItem>
            ))}
          </AppSidebarMenu>
        </AppSidebarGroupContent>
      </AppSidebarGroup>
    </>
  );
}

export function AppSidebar({
  projects = [],
  threads = [],
  username = "",
  fullName = "",
}: {
  projects?: SidebarProject[];
  threads?: SidebarThread[];
  username?: string;
  fullName?: string;
}): Component {
  let accountOpen = state(false);
  onMount(() => {
    const mq = window.matchMedia(MOBILE_QUERY);
    const sync = () => {
      sidebar.status = mq.matches ? "hidden" : "open";
    };
    sync();
    mq.addEventListener("change", sync);
    const host = this as SidebarHost;
    let socket: WebSocket | null = null;
    let retry: ReturnType<typeof setTimeout> | null = null;
    let stopped = false;
    const closeAccount = (event: Event) => {
      const footer = this.shadowRoot?.querySelector(".account-footer");
      if (accountOpen && footer && !event.composedPath().includes(footer)) accountOpen = false;
    };
    document.addEventListener("click", closeAccount);
    const connect = () => {
      const protocol = location.protocol === "https:" ? "wss:" : "ws:";
      socket = new WebSocket(`${protocol}//${location.host}/api/events`);
      socket.onopen = () => void resyncThreads(host);
      socket.onmessage = (event) =>
        applyUserEvent(host, JSON.parse(event.data as string) as UserEvent);
      socket.onclose = () => {
        if (!stopped) retry = setTimeout(connect, 2000 + Math.random() * 3000);
      };
    };
    connect();
    return () => {
      stopped = true;
      mq.removeEventListener("change", sync);
      document.removeEventListener("click", closeAccount);
      if (retry) clearTimeout(retry);
      socket?.close();
    };
  });

  const collapsed = sidebar.status === "collapsed";
  return (
    <>
      <style>{styles}</style>
      <style>{accountFooterStyles}</style>
      <AppSidebarPanel state={sidebar.status}>
        <button
          class="scrim"
          type="button"
          aria-label="Close sidebar"
          onClick={() => (sidebar.status = "hidden")}
        ></button>
        <AppSidebarHeader>
          <div class="brand">
            <span class="mark" aria-hidden="true">
              <IconStrideMark />
            </span>
            <strong>S.T.R.I.D.E.</strong>
            <AppSidebarToggle brand="stride" />
          </div>
        </AppSidebarHeader>
        <AppSidebarContent state={sidebar.status}>
          <AppSidebarMenu>
            <SidebarNavigationItem
              href="/threads"
              label="New task"
              kind="tasks"
              collapsed={collapsed}
            />
            <SidebarNavigationItem
              href="/files"
              label="Files"
              kind="files"
              active={sidebar.activePage === "files"}
              collapsed={collapsed}
            />
            <SidebarNavigationItem
              href="/automations"
              label="Automations"
              kind="automations"
              active={sidebar.activePage === "automations"}
              collapsed={collapsed}
            />
            <li class="nav-action-item">
              <button class="nav-action" type="button" onClick={() => emit(this, "new-project")}>
                <span class="icon"><IconPlus /></span>
                <span class="label">New project</span>
              </button>
            </li>
          </AppSidebarMenu>
          <div class="groups">
            {projects.map((project) => (
              <SidebarThreadGroup
                key={project.id}
                title={project.title}
                threads={project.threads}
                projectId={project.id}
                projectTitle={project.title}
              />
            ))}
            {threads.length > 0 && (
              <SidebarThreadGroup title="Threads" threads={threads} />
            )}
          </div>
        </AppSidebarContent>
        <AppSidebarFooter>
          <div class="account-footer">
            <button
              class="trigger"
              type="button"
              aria-haspopup="menu"
              aria-expanded={accountOpen ? "true" : "false"}
              title={collapsed ? settings.fullName || fullName : "Account menu"}
              onClick={(event: Event) => {
                event.stopPropagation();
                if (sidebar.status === "collapsed") sidebar.status = "open";
                accountOpen = !accountOpen;
              }}
            >
              <AppAvatar fallback={profileInitials(settings.fullName || fullName, settings.username || username)} />
              <span class="identity">
                <span class="name">{settings.fullName || fullName}</span>
                <span class="username">@{settings.username || username}</span>
              </span>
              <span class="selector" aria-hidden="true">
                <IconChevronsUpDown />
              </span>
            </button>
            <div class="menu" role="menu" hidden={!accountOpen}>
              <a href="/archived" role="menuitem" onClick={() => (accountOpen = false)}>
                <span class="icon"><IconArchive /></span>
                Archived
              </a>
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  accountOpen = false;
                  settings.open = true;
                  if (window.matchMedia(MOBILE_QUERY).matches) sidebar.status = "hidden";
                }}
              >
                <span class="icon"><IconSettingsHorizontal /></span>
                Settings
              </button>
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  accountOpen = false;
                  emit(this, "logout");
                }}
              >
                <span class="icon" aria-hidden="true">↪</span>
                Log out
              </button>
            </div>
          </div>
        </AppSidebarFooter>
        <AppSidebarRail
          collapsed={collapsed}
          on:toggle={() => toggleSidebar()}
        />
      </AppSidebarPanel>
    </>
  );
}

export function AppSidebarToggle({
  brand = "",
}: {
  brand?: string;
}): Component {
  const closed = sidebar.status !== "open";
  return (
    <>
      <style>{toggleStyles}</style>
      <AppButton
        variant="ghost"
        size={brand !== "" ? "icon" : "icon-xs"}
        title={closed ? "Open sidebar" : "Close sidebar"}
        class={brand !== "" ? "with-brand" : ""}
        onClick={() => toggleSidebar()}
      >
        {brand !== "" && closed ? (
          <span class="with-brand">
            <span class="brand-mark" aria-hidden="true">
              <IconStrideMark />
            </span>
            <span class="hover-icon">
              <IconPanelLeftOpen />
            </span>
          </span>
        ) : closed ? (
          <IconPanelLeftOpen />
        ) : (
          <IconPanelLeftClose />
        )}
      </AppButton>
    </>
  );
}
