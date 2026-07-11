/*
 * Design and functionality adapted from shadcn/ui sidebar component.
 * shadcn/ui — MIT License — Copyright (c) 2023 shadcn
 * https://ui.shadcn.com/docs/components/sidebar
 */
import { Component, css, emit, onMount } from "@frontiers-labs/argon";
import { sidebar } from "../stores/ui.js";
import { AppButton } from "./app-button.js";
import { IconArchive } from "./icons/archive.js";
import { IconBotMessageSquare } from "./icons/bot-message-square.js";
import { IconChevronDown } from "./icons/chevron-down.js";
import { IconChevronRight } from "./icons/chevron-right.js";
import { IconFiles } from "./icons/files.js";
import { IconPanelLeftClose } from "./icons/panel-left-close.js";
import { IconPanelLeftOpen } from "./icons/panel-left-open.js";
import { IconSettingsHorizontal } from "./icons/settings-horizontal.js";
import { IconWorkflow } from "./icons/workflow.js";

const styles = css`:host{--sidebar-width:260px;--sidebar-width-icon:48px;--sidebar-menu-button-size:32px;display:block;height:100%;width:fit-content;}.root{background:var(--sidebar-bg,var(--secondary));border-right:1px solid var(--sidebar-border,var(--border));box-sizing:border-box;display:flex;flex-direction:column;align-items:stretch;height:100%;overflow:hidden;position:relative;transition:width 200ms linear;width:var(--sidebar-width);}.root.collapsed{width:var(--sidebar-width-icon);}.root.hidden{display:none;}.scrim{border:0;display:none;padding:0;}.header{flex:0 0 auto;width:100%;}.brand{align-items:center;display:flex;gap:8px;padding:8px;}.mark{align-items:center;background:var(--primary);border-radius:8px;color:var(--primary-foreground);display:inline-flex;flex:0 0 auto;font-size:13px;font-weight:700;height:32px;justify-content:center;width:32px;}.brand strong{color:var(--foreground);flex:1;font-size:14px;font-weight:600;min-width:0;}.root.collapsed .brand{padding:8px;}.root.collapsed .brand .mark,.root.collapsed .brand strong{display:none;}.main{flex:1;overflow:auto;padding:8px 0;width:100%;}.root.collapsed .main{overflow:hidden;}.footer{display:flex;flex:0 0 auto;flex-direction:column;gap:4px;padding:8px;width:100%;box-sizing:border-box;}.footer app-button{width:100%;}.root.collapsed .footer,.root.collapsed .groups{display:none;}.nav-item{box-sizing:border-box;display:block;padding:0 8px;width:100%;}.nav-item a{align-items:center;border-radius:6px;box-sizing:border-box;color:var(--sidebar-fg,var(--foreground));display:flex;font-size:14px;font-weight:400;gap:8px;height:32px;line-height:20px;outline:none;overflow:hidden;padding:0 8px;text-align:left;text-decoration:none;transition:background-color 140ms ease,color 140ms ease,width 200ms linear;user-select:none;white-space:nowrap;width:100%;}.nav-item a:hover,.nav-item a[aria-current="page"]{background:var(--sidebar-accent,var(--accent));color:var(--sidebar-accent-fg,var(--accent-foreground));}.nav-item a[aria-current="page"]{font-weight:500;}.nav-item a:focus-visible{box-shadow:0 0 0 2px var(--ring-shadow,rgb(24 24 27 / 12%));}.nav-item .icon{align-items:center;display:inline-flex;flex:0 0 16px;height:16px;justify-content:center;width:16px;}.nav-item .icon>*{height:16px;width:16px;}.nav-item .label{flex:1;min-width:0;overflow:hidden;text-overflow:ellipsis;}.root.collapsed .nav-item{padding:0 8px;}.root.collapsed .nav-item a{height:32px;padding:0 8px;width:var(--sidebar-menu-button-size);}.root.collapsed .nav-item .label{display:none;}.group{width:100%;padding:8px 0;}.group-toggle{align-items:center;background:transparent;border:0;border-radius:6px;box-sizing:border-box;color:var(--muted-foreground);cursor:pointer;display:flex;font:inherit;font-size:12px;font-weight:500;gap:8px;height:28px;line-height:16px;margin:0 8px;outline:none;padding:0 8px;text-align:left;transition:background-color 140ms ease,color 140ms ease;user-select:none;width:calc(100% - 16px);}.group-toggle:hover{background:var(--sidebar-accent,var(--accent));color:var(--sidebar-accent-fg,var(--accent-foreground));}.group-toggle:focus-visible{box-shadow:0 0 0 2px var(--ring-shadow,rgb(24 24 27 / 12%));}.group-title{min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}.chevron{align-items:center;color:var(--muted-foreground);display:inline-flex;flex:0 0 16px;height:16px;justify-content:center;opacity:0;transition:opacity 140ms ease;width:16px;}.group-toggle:hover .chevron,.group-toggle:focus-visible .chevron{opacity:1;}.chevron>*{align-items:center;display:inline-flex;height:16px;justify-content:center;width:16px;}.chevron>*>*{height:16px;width:16px;}.chevron .closed-mark,.group.closed .chevron .open-mark{display:none;}.group.closed .chevron .closed-mark{display:inline-flex;}.project-actions{display:none;gap:2px;margin-left:auto;}.group-toggle:hover .project-actions,.group-toggle:focus-within .project-actions{display:inline-flex;}.project-actions span{background:transparent;border:0;border-radius:4px;color:var(--muted-foreground);cursor:pointer;font-size:12px;height:20px;line-height:20px;padding:0 4px;}.project-actions span:hover{background:var(--accent);color:var(--accent-foreground);}.group ul{border-left:1px solid var(--sidebar-border,var(--border));box-sizing:border-box;display:flex;flex-direction:column;gap:2px;list-style-type:none;margin:4px 8px 0 16px;padding:0 0 0 10px;}.group.closed ul{display:none;}.group ul a{align-items:center;border-radius:6px;box-sizing:border-box;color:var(--sidebar-fg,var(--foreground));display:flex;font-size:14px;font-weight:400;height:28px;line-height:20px;outline:none;overflow:hidden;padding:0 8px;text-align:left;text-decoration:none;transition:background-color 140ms ease,color 140ms ease;user-select:none;white-space:nowrap;width:100%;}.group ul a:hover,.group ul a[aria-current="page"]{background:var(--sidebar-accent,var(--accent));color:var(--sidebar-accent-fg,var(--accent-foreground));}.group ul a[aria-current="page"]{font-weight:500;}.group ul a:focus-visible{box-shadow:0 0 0 2px var(--ring-shadow,rgb(24 24 27 / 12%));}.thread-label{display:flex;align-items:center;gap:7px;min-width:0;overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}.run-pulse{background:var(--primary);border-radius:999px;box-shadow:0 0 0 0 color-mix(in srgb,var(--primary) 35%,transparent);display:inline-block;flex:0 0 6px;height:6px;width:6px;animation:run-pulse 1.6s ease-out infinite;}@keyframes run-pulse{70%{box-shadow:0 0 0 5px transparent;}100%{box-shadow:0 0 0 0 transparent;}}.group ul li{position:relative;}.thread-menu{align-items:center;background:var(--sidebar-bg,var(--secondary));border-radius:4px;color:var(--muted-foreground);cursor:pointer;display:none;font-size:16px;height:22px;justify-content:center;line-height:1;position:absolute;right:4px;top:50%;transform:translateY(-50%);user-select:none;width:22px;}.group ul li:hover .thread-menu,.thread-menu[aria-expanded="true"]{display:inline-flex;}.thread-menu:hover{background:var(--accent);color:var(--accent-foreground);}.group ul li:hover a{padding-right:28px;}@media(max-width:767px){.root{display:none;}:host([hydrated]) .root.open{box-shadow:0 18px 48px rgb(0 0 0 / 22%);display:flex;inset:0 auto 0 0;max-width:320px;position:fixed;width:min(84vw,320px);z-index:50;}.scrim{background:rgb(0 0 0 / 36%);display:block;inset:0;position:fixed;z-index:-1;}.nav-item a,.group-toggle,.group ul a{font-size:16px;height:44px;padding:0 12px;}.nav-item .icon{flex-basis:20px;height:20px;width:20px;}.nav-item .icon>*{height:20px;width:20px;}.group ul{padding-left:8px;}.thread-menu{display:inline-flex;height:28px;width:28px;}.group ul li a{padding-right:32px;}}@media(prefers-reduced-motion:reduce){.root,.nav-item a{transition:none;}.run-pulse{animation:none;}}`;

const toggleStyles = css`:host{display:inline-flex;}icon-panel-left-open,icon-panel-left-close{height:16px;width:16px;}.brand-mark{align-items:center;background:var(--primary);border-radius:8px;color:var(--primary-foreground);display:inline-flex;font-size:13px;font-weight:700;height:32px;justify-content:center;width:32px;}.hover-icon{display:none;}.with-brand:hover .brand-mark,.with-brand:focus-within .brand-mark{display:none;}.with-brand:hover .hover-icon,.with-brand:focus-within .hover-icon{align-items:center;display:inline-flex;height:16px;justify-content:center;width:16px;}`;

interface SidebarThread {
  id: string;
  title: string;
}

interface SidebarProject {
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
    | {type: "thread_created"; thread_id: string; title: string; project_id: string | null}
    | {type: "thread_renamed"; thread_id: string; title: string}
    | {type: "thread_archived" | "thread_deleted"; thread_id: string}
    | {type: "thread_restored" | "resync"; thread_id?: string}
    | {type: "thread_run_status"; thread_id: string; running: boolean}
    | {type: "notification"; notification_id: string; title: string; message: string; thread_id: string | null};
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
    const thread = {id: kind.thread_id, title: kind.title};
    const project = kind.project_id ? host.projects.find((candidate) => candidate.id === kind.project_id) : undefined;
    if (project) {
      host.projects = host.projects.map((candidate) => candidate.id === project.id
        ? {...candidate, threads: [thread, ...candidate.threads]}
        : candidate);
    } else {
      host.threads = [thread, ...host.threads];
    }
    return;
  }
  if (kind.type === "thread_renamed") {
    host.threads = host.threads.map((thread) => thread.id === kind.thread_id ? {...thread, title: kind.title} : thread);
    host.projects = host.projects.map((project) => ({
      ...project,
      threads: project.threads.map((thread) => thread.id === kind.thread_id ? {...thread, title: kind.title} : thread),
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
  if (kind.type === "notification") return;
  void resyncThreads(host);
}

async function resyncThreads(host: SidebarHost): Promise<void> {
  const response = await fetch("/api/threads");
  if (!response.ok) return;
  const threads = await response.json() as {id: string; title: string; project_id: string | null}[];
  host.projects = host.projects.map((project) => ({
    ...project,
    threads: threads.filter((thread) => thread.project_id === project.id).map(({id, title}) => ({id, title})),
  }));
  const projectIds = new Set(host.projects.map((project) => project.id));
  host.threads = threads
    .filter((thread) => !thread.project_id || !projectIds.has(thread.project_id))
    .map(({id, title}) => ({id, title}));
}

const MOBILE_QUERY = "(max-width: 767px)";

function toggleSidebar(): void {
  const mobile = window.matchMedia(MOBILE_QUERY).matches;
  if (mobile) {
    sidebar.status = sidebar.status === "open" ? "hidden" : "open";
  } else {
    sidebar.status = sidebar.status === "collapsed" ? "open" : "collapsed";
  }
}

export function AppSidebar({
  projects = [],
  threads = [],
}: {
  projects?: SidebarProject[];
  threads?: SidebarThread[];
}): Component {
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
    const connect = () => {
      const protocol = location.protocol === "https:" ? "wss:" : "ws:";
      socket = new WebSocket(`${protocol}//${location.host}/api/events`);
      socket.onopen = () => void resyncThreads(host);
      socket.onmessage = (event) => applyUserEvent(host, JSON.parse(event.data as string) as UserEvent);
      socket.onclose = () => {
        if (!stopped) retry = setTimeout(connect, 2000 + Math.random() * 3000);
      };
    };
    connect();
    return () => {
      stopped = true;
      mq.removeEventListener("change", sync);
      if (retry) clearTimeout(retry);
      socket?.close();
    };
  });

  return (
    <>
      <style>{styles}</style>
      <div class={`root ${sidebar.status}`}>
        <button
          class="scrim"
          type="button"
          aria-label="Close sidebar"
          onClick={() => {
            sidebar.status = "hidden";
          }}
        ></button>
        <div class="header">
          <div class="brand">
            <span class="mark">F</span>
            <strong>S.T.R.I.D.E.</strong>
            <AppSidebarToggle brand="F" />
          </div>
        </div>
        <div
          class="main"
          onClick={(event: Event) => {
            const target = event.target as Element;
            // Nav and thread entries are plain <a href> links — let them navigate.
            // Only project mutations (need a dialog + API) and group collapsing
            // are handled here.
            const action = target.closest<HTMLElement>("[data-action]");
            if (action) {
              event.preventDefault();
              const name = action.dataset.action!;
              // Thread actions carry the thread id/title and the trigger element
              // (so a menu can anchor to it); project actions keep their fields.
              const detail = {
                id: action.dataset.threadId ?? action.dataset.projectId ?? "",
                title: action.dataset.threadTitle ?? action.dataset.projectTitle ?? "",
                anchor: action,
              };
              emit(this, name, detail);
              return;
            }
            const toggle = target.closest(".group-toggle");
            if (toggle) {
              toggle.closest(".group")!.classList.toggle("closed");
            }
          }}
        >
          <span class="nav-item">
            <a href="/threads">
              <span class="icon"><IconBotMessageSquare /></span>
              <span class="label">New task</span>
            </a>
          </span>
          <span class="nav-item">
            <a href="/files" aria-current={sidebar.activePage === "files" ? "page" : "false"}>
              <span class="icon"><IconFiles /></span>
              <span class="label">Files</span>
            </a>
          </span>
          <span class="nav-item">
            <a href="/automations" aria-current={sidebar.activePage === "automations" ? "page" : "false"}>
              <span class="icon"><IconWorkflow /></span>
              <span class="label">Automations</span>
            </a>
          </span>
          <span class="nav-item">
            <a href="/archived" aria-current={sidebar.activePage === "archived" ? "page" : "false"}>
              <span class="icon"><IconArchive /></span>
              <span class="label">Archived</span>
            </a>
          </span>
          <span class="nav-item">
            <a href="/settings" aria-current={sidebar.activePage === "settings" ? "page" : "false"}>
              <span class="icon"><IconSettingsHorizontal /></span>
              <span class="label">Settings</span>
            </a>
          </span>
          <div class="groups">
            {projects.map((project) => (
              <div key={project.id} class="group">
                <button class="group-toggle" type="button">
                  <span class="group-title">{project.title}</span>
                  <span class="chevron" aria-hidden="true">
                    <span class="open-mark"><IconChevronDown /></span>
                    <span class="closed-mark"><IconChevronRight /></span>
                  </span>
                  <span class="project-actions">
                    <span
                      role="button"
                      title="New thread"
                      data-action="project-new-thread"
                      data-project-id={project.id}
                      data-project-title={project.title}
                    >+</span>
                    <span
                      role="button"
                      title="Rename"
                      data-action="project-rename"
                      data-project-id={project.id}
                      data-project-title={project.title}
                    >✎</span>
                    <span
                      role="button"
                      title="Delete"
                      data-action="project-delete"
                      data-project-id={project.id}
                    >✕</span>
                  </span>
                </button>
                <ul>
                  {project.threads.map((thread) => (
                    <li key={thread.id}>
                      <a
                        href={`/threads/${thread.id}`}
                        data-thread-id={thread.id}
                        aria-current={thread.id === sidebar.activeThread ? "page" : "false"}
                      >
                        <span class="thread-label">
                          {sidebar.runningThreads.includes(thread.id) && <span class="run-pulse" title="Running" />}
                          {thread.title}
                        </span>
                      </a>
                      <span
                        class="thread-menu"
                        role="button"
                        title="Thread actions"
                        aria-label="Thread actions"
                        data-action="thread-menu"
                        data-thread-id={thread.id}
                        data-thread-title={thread.title}
                      >⋯</span>
                    </li>
                  )).join("")}
                </ul>
              </div>
            )).join("")}
            {threads.length > 0 && <div class="group">
                <button class="group-toggle" type="button">
                  <span class="group-title">Threads</span>
                  <span class="chevron" aria-hidden="true">
                    <span class="open-mark"><IconChevronDown /></span>
                    <span class="closed-mark"><IconChevronRight /></span>
                  </span>
                </button>
                <ul>
                  {threads.map((thread) => (
                    <li key={thread.id}>
                      <a
                        href={`/threads/${thread.id}`}
                        data-thread-id={thread.id}
                        aria-current={thread.id === sidebar.activeThread ? "page" : "false"}
                      >
                        <span class="thread-label">
                          {sidebar.runningThreads.includes(thread.id) && <span class="run-pulse" title="Running" />}
                          {thread.title}
                        </span>
                      </a>
                      <span
                        class="thread-menu"
                        role="button"
                        title="Thread actions"
                        aria-label="Thread actions"
                        data-action="thread-menu"
                        data-thread-id={thread.id}
                        data-thread-title={thread.title}
                      >⋯</span>
                    </li>
                  )).join("")}
                </ul>
              </div>}
          </div>
        </div>
        <div
          class="footer"
          onClick={(event: Event) => {
            const button = (event.target as Element).closest<HTMLElement>("[data-action]");
            if (!button) return;
            emit(this, button.dataset.action!);
          }}
        >
          <AppButton variant="ghost" data-action="new-project">+ New project</AppButton>
          <AppButton variant="secondary" data-action="logout">Log out</AppButton>
        </div>
      </div>
    </>
  );
}

export function AppSidebarToggle({ brand = "" }: { brand?: string }): Component {
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
            <span class="brand-mark">{brand}</span>
            <span class="hover-icon"><IconPanelLeftOpen /></span>
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
