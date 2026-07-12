/*
 * Portions of these components' visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, emit } from "@frontiers-labs/argon";
import { AppInput } from "./app-input.js";
import { AppSkeleton } from "./app-skeleton.js";

const providerStyles = css`
  :host { display: block; height: 100%; min-height: 0; width: 100%; }
  .wrapper { display: flex; height: 100%; min-height: 0; width: 100%; }
`;

export function AppSidebarProvider(): Component {
  return <><style>{providerStyles}</style><div class="wrapper"><slot></slot></div></>;
}

const panelStyles = css`
  :host { color: var(--sidebar-fg, var(--foreground)); display: block; flex: 0 0 auto; height: 100%; overflow: hidden; position: relative; width: var(--sidebar-width, 260px); }
  .panel { background: var(--sidebar-bg, var(--secondary)); border-right: 1px solid var(--sidebar-border, var(--border)); box-sizing: border-box; display: flex; flex-direction: column; height: 100%; overflow: hidden; width: 100%; }
  :host([side="right"]) .panel { border-left: 1px solid var(--sidebar-border, var(--border)); border-right: 0; }
  :host([variant="floating"]) { padding: 8px; }
  :host([variant="floating"]) .panel { border: 1px solid var(--sidebar-border, var(--border)); border-radius: var(--radius-lg); box-shadow: 0 2px 8px rgb(0 0 0 / 8%); }
  :host([state="collapsed"]), :host([data-state="collapsed"]) { width: var(--sidebar-width-icon, 48px); }
  :host([state="hidden"]), :host([data-state="hidden"]) { display: none; }
  @media (max-width: 767px) {
    :host { display: none; }
    :host([state="open"]), :host([data-state="open"]) { box-shadow: 0 18px 48px rgb(0 0 0 / 22%); display: block; inset: 0 auto 0 0; max-width: 320px; position: fixed; width: min(84vw, 320px); z-index: 50; }
  }
  @media (prefers-reduced-motion: no-preference) { :host { transition: width 200ms linear; } }
`;

export function AppSidebarPanel(): Component {
  return <><style>{panelStyles}</style><aside class="panel"><slot></slot></aside></>;
}

const insetStyles = css`
  :host { background: var(--background); display: flex; flex: 1; flex-direction: column; min-width: 0; }
  main { display: flex; flex: 1; flex-direction: column; min-width: 0; }
  :host([variant="inset"]) { border-radius: var(--radius-xl); box-shadow: 0 1px 2px rgb(0 0 0 / 6%); margin: 8px 8px 8px 0; overflow: hidden; }
`;

export function AppSidebarInset(): Component { return <><style>{insetStyles}</style><main><slot></slot></main></>; }

const sectionStyles = css`
  :host { box-sizing: border-box; display: flex; flex: 0 0 auto; flex-direction: column; gap: 4px; padding: 8px; width: 100%; }
  header, footer { display: contents; }
`;

export function AppSidebarHeader(): Component { return <><style>{sectionStyles}</style><header><slot></slot></header></>; }
export function AppSidebarFooter(): Component { return <><style>{sectionStyles}</style><footer><slot></slot></footer></>; }

const contentStyles = css`
  :host { display: block; flex: 1; min-height: 0; overflow: auto; width: 100%; }
  :host([state="collapsed"]) { overflow: hidden; }
  .content { display: flex; flex-direction: column; min-width: 0; padding: 8px 0; width: 100%; }
`;

export function AppSidebarContent(): Component { return <><style>{contentStyles}</style><div class="content"><slot></slot></div></>; }

const groupStyles = css`
  :host { box-sizing: border-box; display: block; padding: 8px 0; width: 100%; }
  .group { display: flex; flex-direction: column; min-width: 0; width: 100%; }
`;

export function AppSidebarGroup(): Component { return <><style>{groupStyles}</style><section class="group"><slot></slot></section></>; }

const labelStyles = css`
  :host { display: block; min-width: 0; position: relative; width: 100%; }
  .row { align-items: center; display: flex; margin: 0 8px; min-width: 0; position: relative; width: calc(100% - 16px); }
  button { align-items: center; background: transparent; border: 0; border-radius: 6px; box-sizing: border-box; color: var(--muted-foreground); cursor: pointer; display: flex; flex: 1; font: inherit; font-size: 12px; font-weight: 500; gap: 8px; height: 28px; line-height: 16px; min-width: 0; outline: none; padding: 0 8px; text-align: left; user-select: none; }
  button:hover, button:focus-visible, .row:focus-within button { background: var(--sidebar-accent, var(--accent)); color: var(--sidebar-accent-fg, var(--accent-foreground)); }
  button:focus-visible { box-shadow: 0 0 0 2px var(--ring-shadow, rgb(24 24 27 / 12%)); }
  .actions { align-items: center; display: none; gap: 2px; position: absolute; right: 4px; }
  .row:hover .actions, .row:focus-within .actions { display: inline-flex; }
`;

export function AppSidebarGroupLabel(): Component {
  return <><style>{labelStyles}</style><div class="row"><button type="button" onClick={() => emit(this, "toggle")}><slot></slot></button><span class="actions"><slot name="actions"></slot></span></div></>;
}

const groupContentStyles = css`
  :host { display: block; width: 100%; }
  :host([hidden]) { display: none; }
  .content { border-left: 1px solid var(--sidebar-border, var(--border)); box-sizing: border-box; margin: 4px 8px 0 16px; padding: 0 0 0 10px; }
`;

export function AppSidebarGroupContent(): Component { return <><style>{groupContentStyles}</style><div class="content"><slot></slot></div></>; }

const menuStyles = css`
  :host { display: block; width: 100%; }
  ul { display: flex; flex-direction: column; gap: 2px; list-style: none; margin: 0; padding: 0; width: 100%; }
`;

export function AppSidebarMenu(): Component { return <><style>{menuStyles}</style><ul><slot></slot></ul></>; }

const itemStyles = css`
  :host { display: block; min-width: 0; position: relative; width: 100%; }
  li { list-style: none; min-width: 0; position: relative; }
`;

export function AppSidebarMenuItem(): Component { return <><style>{itemStyles}</style><li><slot></slot></li></>; }

const menuButtonStyles = css`
  :host { box-sizing: border-box; display: block; min-width: 0; padding: 0 8px; width: 100%; }
  .control { align-items: center; background: transparent; border: 0; border-radius: 6px; box-sizing: border-box; color: var(--sidebar-fg, var(--foreground)); cursor: pointer; display: flex; font: inherit; font-size: 14px; font-weight: 400; gap: 8px; height: 32px; line-height: 20px; outline: none; overflow: hidden; padding: 0 8px; text-align: left; text-decoration: none; user-select: none; white-space: nowrap; width: 100%; }
  .control:hover, :host([active]) .control, :host([data-active="true"]) .control { background: var(--sidebar-accent, var(--accent)); color: var(--sidebar-accent-fg, var(--accent-foreground)); }
  :host([active]) .control, :host([data-active="true"]) .control { font-weight: 500; }
  .control:focus-visible { box-shadow: 0 0 0 2px var(--ring-shadow, rgb(24 24 27 / 12%)); }
  :host([compact]) { padding: 0; }
  :host([compact]) .control { height: 28px; }
  :host([data-collapsed="true"]) .control { width: 32px; }
  :host([data-collapsed="true"]) ::slotted(.label) { display: none; }
  :host([disabled]) .control { cursor: default; opacity: .5; pointer-events: none; }
  @media (prefers-reduced-motion: no-preference) { .control { transition: background-color 140ms ease, color 140ms ease, width 200ms linear; } }
  @media (max-width: 767px) { .control { font-size: 16px; height: 44px; padding: 0 12px; } :host([compact]) .control { height: 44px; } }
`;

export function AppSidebarMenuButton({ href = "", active = false, disabled = false, collapsed = false }: { href?: string; active?: boolean; disabled?: boolean; collapsed?: boolean }): Component {
  return <>
    <style>{menuButtonStyles}</style>
    {href !== ""
      ? <a class="control" href={href} aria-current={active ? "page" : "false"}><slot></slot></a>
      : <button class="control" type="button" disabled={disabled} onClick={() => emit(this, "select")}><slot></slot></button>}
  </>;
}

const actionStyles = css`
  :host { align-items: center; display: inline-flex; }
  button { align-items: center; background: transparent; border: 0; border-radius: 4px; color: var(--muted-foreground); cursor: pointer; display: inline-flex; font: inherit; font-size: 16px; height: 22px; justify-content: center; line-height: 1; padding: 0; width: 22px; }
  button:hover, button:focus-visible { background: var(--accent); color: var(--accent-foreground); outline: none; }
  :host([small]) button { font-size: 12px; height: 20px; width: 20px; }
  @media (max-width: 767px) { button { height: 28px; width: 28px; } }
`;

export function AppSidebarMenuAction(): Component { return <><style>{actionStyles}</style><button type="button" onClick={() => emit(this, "select")}><slot></slot></button></>; }

const badgeStyles = css`:host { align-items: center; color: var(--sidebar-fg); display: inline-flex; font-size: 0.75rem; font-variant-numeric: tabular-nums; height: 20px; justify-content: center; min-width: 20px; padding: 0 4px; pointer-events: none; }`;
export function AppSidebarMenuBadge(): Component { return <><style>{badgeStyles}</style><slot></slot></>; }

const inputStyles = css`:host { display: block; padding: 0 8px; }`;
export function AppSidebarInput({ value = "", placeholder = "" }: { value?: string; placeholder?: string }): Component { return <><style>{inputStyles}</style><AppInput value={value} placeholder={placeholder}></AppInput></>; }

const separatorStyles = css`:host { display: block; padding: 0 8px; } hr { background: var(--sidebar-border); border: 0; height: 1px; margin: 0; }`;
export function AppSidebarSeparator(): Component { return <><style>{separatorStyles}</style><hr /></>; }

const skeletonStyles = css`:host { align-items: center; display: flex; gap: 8px; height: 32px; padding: 0 8px; } app-skeleton:first-child { flex: 0 0 16px; height: 16px; } app-skeleton:last-child { flex: 1; height: 14px; }`;
export function AppSidebarMenuSkeleton(): Component { return <><style>{skeletonStyles}</style><AppSkeleton></AppSkeleton><AppSkeleton></AppSkeleton></>; }

const railStyles = css`
  :host { bottom: 0; display: block; position: absolute; right: -8px; top: 0; width: 16px; z-index: 20; }
  button { background: transparent; border: 0; cursor: w-resize; height: 100%; padding: 0; position: relative; width: 100%; }
  button::after { bottom: 0; content: ""; left: 50%; position: absolute; top: 0; width: 2px; }
  button:hover::after { background: var(--sidebar-border, var(--border)); }
  :host([data-collapsed="true"]) button { cursor: e-resize; }
  @media (max-width: 767px) { :host { display: none; } }
`;

export function AppSidebarRail({ collapsed = false }: { collapsed?: boolean }): Component { return <><style>{railStyles}</style><button type="button" aria-label="Toggle sidebar" title="Toggle sidebar" onClick={() => emit(this, "toggle")}></button></>; }
