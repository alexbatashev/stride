/*
 * Design and functionality adapted from shadcn/ui sidebar component.
 * shadcn/ui — MIT License — Copyright (c) 2023 shadcn
 * https://ui.shadcn.com/docs/components/sidebar
 */
import { Component, css, emit, onMount, state } from "@frontiers-labs/argon";
import { sidebar } from "../stores/ui.js";
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
import { IconBotMessageSquare } from "./icons/bot-message-square.js";
import { IconChevronDown } from "./icons/chevron-down.js";
import { IconChevronRight } from "./icons/chevron-right.js";
import { IconFiles } from "./icons/files.js";
import { IconPanelLeftClose } from "./icons/panel-left-close.js";
import { IconPanelLeftOpen } from "./icons/panel-left-open.js";
import { IconSettingsHorizontal } from "./icons/settings-horizontal.js";
import { IconWorkflow } from "./icons/workflow.js";

const styles = css`
  :host { --sidebar-width: 260px; --sidebar-width-icon: 48px; display: block; height: 100%; width: fit-content; }
  app-sidebar-panel { --sidebar-width: 260px; --sidebar-width-icon: 48px; }
  .scrim { border: 0; display: none; padding: 0; }
  .brand { align-items: center; display: flex; gap: 8px; min-width: 0; }
  .mark { align-items: center; background: var(--primary); border-radius: 8px; color: var(--primary-foreground); display: inline-flex; flex: 0 0 auto; font-size: 13px; font-weight: 700; height: 32px; justify-content: center; width: 32px; }
  .brand strong { color: var(--foreground); flex: 1; font-size: 14px; font-weight: 600; min-width: 0; }
  app-sidebar-panel[state="collapsed"] .mark,
  app-sidebar-panel[state="collapsed"] .brand strong,
  app-sidebar-panel[state="collapsed"] app-sidebar-footer,
  app-sidebar-panel[state="collapsed"] .groups { display: none; }
  .icon { align-items: center; display: inline-flex; flex: 0 0 16px; height: 16px; justify-content: center; width: 16px; }
  .icon > * { height: 16px; width: 16px; }
  .label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .groups { width: 100%; }
  .group-title { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chevron { align-items: center; color: var(--muted-foreground); display: inline-flex; flex: 0 0 16px; height: 16px; justify-content: center; opacity: 0; transition: opacity 140ms ease; width: 16px; }
  app-sidebar-group-label:hover .chevron,
  app-sidebar-group-label:focus-within .chevron { opacity: 1; }
  .chevron > *, .chevron > * > * { align-items: center; display: inline-flex; height: 16px; justify-content: center; width: 16px; }
  .chevron .closed-mark, app-sidebar-group.closed .chevron .open-mark { display: none; }
  app-sidebar-group.closed .chevron .closed-mark { display: inline-flex; }
  .thread-label { align-items: center; display: flex; gap: 7px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .run-pulse { animation: run-pulse 1.6s ease-out infinite; background: var(--primary); border-radius: 999px; box-shadow: 0 0 0 0 color-mix(in srgb, var(--primary) 35%, transparent); display: inline-block; flex: 0 0 6px; height: 6px; width: 6px; }
  .thread-menu { background: var(--sidebar-bg, var(--secondary)); display: none; position: absolute; right: 4px; top: 3px; }
  app-sidebar-menu-item:hover .thread-menu,
  .thread-menu[aria-expanded="true"] { display: inline-flex; }
  @keyframes run-pulse { 70% { box-shadow: 0 0 0 5px transparent; } 100% { box-shadow: 0 0 0 0 transparent; } }
  @media (max-width: 767px) {
    :host([hydrated]) app-sidebar-panel[state="open"] .scrim { background: rgb(0 0 0 / 36%); display: block; inset: 0; position: fixed; z-index: -1; }
    .icon { flex-basis: 20px; height: 20px; width: 20px; }
    .icon > * { height: 20px; width: 20px; }
    .thread-menu { display: inline-flex; top: 8px; }
  }
  @media (prefers-reduced-motion: reduce) { .chevron { transition: none; } .run-pulse { animation: none; } }
`;

const toggleStyles = css`
  :host { display: inline-flex; }
  icon-panel-left-open, icon-panel-left-close { height: 16px; width: 16px; }
  .brand-mark { align-items: center; background: var(--primary); border-radius: 8px; color: var(--primary-foreground); display: inline-flex; font-size: 13px; font-weight: 700; height: 32px; justify-content: center; width: 32px; }
  .hover-icon { display: none; }
  .with-brand:hover .brand-mark, .with-brand:focus-within .brand-mark { display: none; }
  .with-brand:hover .hover-icon, .with-brand:focus-within .hover-icon { align-items: center; display: inline-flex; height: 16px; justify-content: center; width: 16px; }
`;

const navigationItemStyles = css`
  :host { display: block; width: 100%; }
  .icon { align-items: center; display: inline-flex; flex: 0 0 16px; height: 16px; justify-content: center; width: 16px; }
  .icon > * { height: 16px; width: 16px; }
  .label { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  @media (max-width: 767px) { .icon { flex-basis: 20px; height: 20px; width: 20px; } .icon > * { height: 20px; width: 20px; } }
`;

const threadGroupStyles = css`
  :host { display: block; width: 100%; }
  .group-title { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .chevron { align-items: center; color: var(--muted-foreground); display: inline-flex; flex: 0 0 16px; height: 16px; justify-content: center; opacity: 0; transition: opacity 140ms ease; width: 16px; }
  app-sidebar-group-label:hover .chevron, app-sidebar-group-label:focus-within .chevron { opacity: 1; }
  .chevron > *, .chevron > * > * { align-items: center; display: inline-flex; height: 16px; justify-content: center; width: 16px; }
  .chevron .closed-mark, app-sidebar-group.closed .chevron .open-mark { display: none; }
  app-sidebar-group.closed .chevron .closed-mark { display: inline-flex; }
  .thread-label { align-items: center; display: flex; gap: 7px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .run-pulse { animation: run-pulse 1.6s ease-out infinite; background: var(--primary); border-radius: 999px; box-shadow: 0 0 0 0 color-mix(in srgb, var(--primary) 35%, transparent); display: inline-block; flex: 0 0 6px; height: 6px; width: 6px; }
  .thread-menu-wrap { display: none; position: absolute; right: 4px; top: 3px; }
  app-sidebar-menu-item:hover .thread-menu-wrap, .thread-menu-wrap:focus-within { display: inline-flex; }
  @keyframes run-pulse { 70% { box-shadow: 0 0 0 5px transparent; } 100% { box-shadow: 0 0 0 0 transparent; } }
  @media (max-width: 767px) { .thread-menu-wrap { display: inline-flex; top: 8px; } }
  @media (prefers-reduced-motion: reduce) { .chevron { transition: none; } .run-pulse { animation: none; } }
`;

export interface SidebarThread { id: string; title: string; }
export interface SidebarProject { id: string; title: string; threads: SidebarThread[]; }

interface SidebarHost extends HTMLElement { projects: SidebarProject[]; threads: SidebarThread[]; }

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
  host.projects = host.projects.map((project) => ({...project, threads: project.threads.filter((thread) => thread.id !== threadId)}));
}

function applyUserEvent(host: SidebarHost, event: UserEvent): void {
  const kind = event.kind;
  if (kind.type === "thread_created") {
    removeThread(host, kind.thread_id);
    const thread = {id: kind.thread_id, title: kind.title};
    const project = kind.project_id ? host.projects.find((candidate) => candidate.id === kind.project_id) : undefined;
    if (project) host.projects = host.projects.map((candidate) => candidate.id === project.id ? {...candidate, threads: [thread, ...candidate.threads]} : candidate);
    else host.threads = [thread, ...host.threads];
    return;
  }
  if (kind.type === "thread_renamed") {
    host.threads = host.threads.map((thread) => thread.id === kind.thread_id ? {...thread, title: kind.title} : thread);
    host.projects = host.projects.map((project) => ({...project, threads: project.threads.map((thread) => thread.id === kind.thread_id ? {...thread, title: kind.title} : thread)}));
    return;
  }
  if (kind.type === "thread_archived" || kind.type === "thread_deleted") { removeThread(host, kind.thread_id); return; }
  if (kind.type === "thread_run_status") {
    sidebar.runningThreads = kind.running ? [...new Set([...sidebar.runningThreads, kind.thread_id])] : sidebar.runningThreads.filter((id) => id !== kind.thread_id);
    return;
  }
  if (kind.type !== "notification") void resyncThreads(host);
}

async function resyncThreads(host: SidebarHost): Promise<void> {
  const response = await fetch("/api/threads");
  if (!response.ok) return;
  const threads = await response.json() as {id: string; title: string; project_id: string | null}[];
  host.projects = host.projects.map((project) => ({...project, threads: threads.filter((thread) => thread.project_id === project.id).map(({id, title}) => ({id, title}))}));
  const projectIds = new Set(host.projects.map((project) => project.id));
  host.threads = threads.filter((thread) => !thread.project_id || !projectIds.has(thread.project_id)).map(({id, title}) => ({id, title}));
}

const MOBILE_QUERY = "(max-width: 767px)";

function toggleSidebar(): void {
  if (window.matchMedia(MOBILE_QUERY).matches) sidebar.status = sidebar.status === "open" ? "hidden" : "open";
  else sidebar.status = sidebar.status === "collapsed" ? "open" : "collapsed";
}

export function SidebarNavigationItem({ href, label, kind, active = false, collapsed = false }: { href: string; label: string; kind: string; active?: boolean; collapsed?: boolean }): Component {
  return (
    <><style>{navigationItemStyles}</style><AppSidebarMenuItem>
      <AppSidebarMenuButton href={href} active={active} collapsed={collapsed}>
        <span class="icon">{kind === "tasks" ? <IconBotMessageSquare /> : kind === "files" ? <IconFiles /> : kind === "automations" ? <IconWorkflow /> : kind === "archived" ? <IconArchive /> : <IconSettingsHorizontal />}</span>
        <span class="label">{label}</span>
      </AppSidebarMenuButton>
    </AppSidebarMenuItem></>
  );
}

export function SidebarThreadGroup({ title, threads = [], projectId = "", projectTitle = "" }: { title: string; threads?: SidebarThread[]; projectId?: string; projectTitle?: string }): Component {
  let closed = state(false);
  return (
    <><style>{threadGroupStyles}</style><AppSidebarGroup class={closed ? "closed" : ""}>
      <AppSidebarGroupLabel on:toggle={() => closed = !closed}>
        <span class="group-title">{title}</span>
        <span class="chevron" aria-hidden="true"><span class="open-mark"><IconChevronDown /></span><span class="closed-mark"><IconChevronRight /></span></span>
        {projectId !== "" && <>
          <AppSidebarMenuAction slot="actions" small title="New thread" on:select={() => emit(this, "project-new-thread", { id: projectId, title: projectTitle })}>+</AppSidebarMenuAction>
          <AppSidebarMenuAction slot="actions" small title="Rename" on:select={() => emit(this, "project-rename", { id: projectId, title: projectTitle })}>✎</AppSidebarMenuAction>
          <AppSidebarMenuAction slot="actions" small title="Delete" on:select={() => emit(this, "project-delete", { id: projectId })}>✕</AppSidebarMenuAction>
        </>}
      </AppSidebarGroupLabel>
      <AppSidebarGroupContent hidden={closed}>
        <AppSidebarMenu>
          {threads.map((thread) => (
            <AppSidebarMenuItem key={thread.id}>
              <AppSidebarMenuButton compact href={`/threads/${thread.id}`} active={thread.id === sidebar.activeThread}>
                <span class="thread-label">{sidebar.runningThreads.includes(thread.id) && <span class="run-pulse" title="Running" />}{thread.title}</span>
              </AppSidebarMenuButton>
              <span class="thread-menu-wrap"><AppSidebarMenuAction title="Thread actions" aria-label="Thread actions" on:select={(event: Event) => emit(this, "thread-menu", { id: thread.id, title: thread.title, anchor: event.currentTarget as HTMLElement })}>⋯</AppSidebarMenuAction></span>
            </AppSidebarMenuItem>
          ))}
        </AppSidebarMenu>
      </AppSidebarGroupContent>
    </AppSidebarGroup></>
  );
}

export function AppSidebar({ projects = [], threads = [] }: { projects?: SidebarProject[]; threads?: SidebarThread[] }): Component {
  onMount(() => {
    const mq = window.matchMedia(MOBILE_QUERY);
    const sync = () => { sidebar.status = mq.matches ? "hidden" : "open"; };
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
      socket.onclose = () => { if (!stopped) retry = setTimeout(connect, 2000 + Math.random() * 3000); };
    };
    connect();
    return () => { stopped = true; mq.removeEventListener("change", sync); if (retry) clearTimeout(retry); socket?.close(); };
  });

  const collapsed = sidebar.status === "collapsed";
  return <>
    <style>{styles}</style>
    <AppSidebarPanel state={sidebar.status}>
      <button class="scrim" type="button" aria-label="Close sidebar" onClick={() => sidebar.status = "hidden"}></button>
      <AppSidebarHeader><div class="brand"><span class="mark">F</span><strong>S.T.R.I.D.E.</strong><AppSidebarToggle brand="F" /></div></AppSidebarHeader>
      <AppSidebarContent state={sidebar.status}>
        <AppSidebarMenu>
          <SidebarNavigationItem href="/threads" label="New task" kind="tasks" collapsed={collapsed} />
          <SidebarNavigationItem href="/files" label="Files" kind="files" active={sidebar.activePage === "files"} collapsed={collapsed} />
          <SidebarNavigationItem href="/automations" label="Automations" kind="automations" active={sidebar.activePage === "automations"} collapsed={collapsed} />
          <SidebarNavigationItem href="/archived" label="Archived" kind="archived" active={sidebar.activePage === "archived"} collapsed={collapsed} />
          <SidebarNavigationItem href="/settings" label="Settings" kind="settings" active={sidebar.activePage === "settings"} collapsed={collapsed} />
        </AppSidebarMenu>
        <div class="groups">
          {projects.map((project) => <SidebarThreadGroup key={project.id} title={project.title} threads={project.threads} projectId={project.id} projectTitle={project.title} />)}
          {threads.length > 0 && <SidebarThreadGroup title="Threads" threads={threads} />}
        </div>
      </AppSidebarContent>
      <AppSidebarFooter>
        <AppButton variant="ghost" onClick={() => emit(this, "new-project")}>+ New project</AppButton>
        <AppButton variant="secondary" onClick={() => emit(this, "logout")}>Log out</AppButton>
      </AppSidebarFooter>
      <AppSidebarRail collapsed={collapsed} on:toggle={() => toggleSidebar()} />
    </AppSidebarPanel>
  </>;
}

export function AppSidebarToggle({ brand = "" }: { brand?: string }): Component {
  const closed = sidebar.status !== "open";
  return <><style>{toggleStyles}</style><AppButton variant="ghost" size={brand !== "" ? "icon" : "icon-xs"} title={closed ? "Open sidebar" : "Close sidebar"} class={brand !== "" ? "with-brand" : ""} onClick={() => toggleSidebar()}>
    {brand !== "" && closed ? <span class="with-brand"><span class="brand-mark">{brand}</span><span class="hover-icon"><IconPanelLeftOpen /></span></span> : closed ? <IconPanelLeftOpen /> : <IconPanelLeftClose />}
  </AppButton></>;
}
