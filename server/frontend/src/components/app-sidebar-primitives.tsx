/*
 * Portions of these components' visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, emit } from "@frontiers-labs/argon";
import { AppButton } from "./app-button.js";
import { AppInput } from "./app-input.js";
import { AppSkeleton } from "./app-skeleton.js";

const providerStyles = css`
  :host { display: block; min-height: 100svh; width: 100%; }
  .wrapper { background: var(--sidebar-bg); display: flex; min-height: 100svh; width: 100%; }
`;

export function AppSidebarProvider(): Component {
  return <><style>{providerStyles}</style><div class="wrapper"><slot></slot></div></>;
}

const panelStyles = css`
  :host { color: var(--sidebar-fg); display: block; flex: 0 0 auto; width: var(--sidebar-width, 16rem); }
  .panel { background: var(--sidebar-bg); border-right: 1px solid var(--sidebar-border); box-sizing: border-box; display: flex; flex-direction: column; height: 100svh; width: 100%; }
  :host([side="right"]) .panel { border-left: 1px solid var(--sidebar-border); border-right: 0; }
  :host([variant="floating"]) { padding: 8px; }
  :host([variant="floating"]) .panel { border: 1px solid var(--sidebar-border); border-radius: var(--radius-lg); box-shadow: 0 2px 8px rgb(0 0 0 / 8%); }
  :host([state="collapsed"]), :host([data-state="collapsed"]) { width: var(--sidebar-width-icon, 3rem); }
  :host([preview]) .panel { height: 320px; }
  @media (max-width: 767px) { :host { display: none; } :host([mobile-open]), :host([data-mobile-open="true"]) { display: block; inset: 0 auto 0 0; max-width: 18rem; position: fixed; width: min(86vw,18rem); z-index: 50; } }
  @media (prefers-reduced-motion: no-preference) { :host { transition: width 200ms ease; } }
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
  :host { box-sizing: border-box; display: flex; flex-direction: column; gap: 8px; padding: 8px; width: 100%; }
`;

export function AppSidebarHeader(): Component { return <><style>{sectionStyles}</style><header><slot></slot></header></>; }
export function AppSidebarFooter(): Component { return <><style>{sectionStyles}</style><footer><slot></slot></footer></>; }

const contentStyles = css`
  :host { display: block; flex: 1; min-height: 0; overflow: auto; }
  .content { display: flex; flex-direction: column; gap: 8px; width: 100%; }
`;

export function AppSidebarContent(): Component { return <><style>{contentStyles}</style><div class="content"><slot></slot></div></>; }

const groupStyles = css`
  :host { display: block; padding: 8px; width: 100%; }
  .group { display: flex; flex-direction: column; min-width: 0; width: 100%; }
`;

export function AppSidebarGroup(): Component { return <><style>{groupStyles}</style><section class="group"><slot></slot></section></>; }

const labelStyles = css`
  :host { align-items: center; color: var(--muted-foreground); display: flex; font-size: 0.75rem; font-weight: 500; height: 32px; padding: 0 8px; }
`;

export function AppSidebarGroupLabel(): Component { return <><style>{labelStyles}</style><slot></slot></>; }

const groupContentStyles = css`:host { display: block; width: 100%; }`;
export function AppSidebarGroupContent(): Component { return <><style>{groupContentStyles}</style><slot></slot></>; }

const menuStyles = css`
  :host { display: block; width: 100%; }
  ul { display: flex; flex-direction: column; gap: 2px; list-style: none; margin: 0; padding: 0; width: 100%; }
`;

export function AppSidebarMenu(): Component { return <><style>{menuStyles}</style><ul><slot></slot></ul></>; }

const itemStyles = css`:host { display: block; min-width: 0; position: relative; width: 100%; } li { list-style: none; }`;
export function AppSidebarMenuItem(): Component { return <><style>{itemStyles}</style><li><slot></slot></li></>; }

const menuButtonStyles = css`
  :host { display: block; width: 100%; }
  app-button { width: 100%; }
  :host([active]) app-button, :host([data-active="true"]) app-button { background: var(--sidebar-accent); color: var(--sidebar-accent-fg); }
`;

export function AppSidebarMenuButton({ disabled = false }: { disabled?: boolean }): Component {
  return <><style>{menuButtonStyles}</style><AppButton variant="ghost" align="start" disabled={disabled} onClick={() => emit(this, "select")}><slot></slot></AppButton></>;
}

const actionStyles = css`
  :host { display: inline-flex; position: absolute; right: 4px; top: 4px; }
`;

export function AppSidebarMenuAction(): Component { return <><style>{actionStyles}</style><AppButton size="icon-xs" variant="ghost"><slot></slot></AppButton></>; }

const badgeStyles = css`
  :host { align-items: center; color: var(--sidebar-fg); display: inline-flex; font-size: 0.75rem; font-variant-numeric: tabular-nums; height: 20px; justify-content: center; min-width: 20px; padding: 0 4px; pointer-events: none; }
`;

export function AppSidebarMenuBadge(): Component { return <><style>{badgeStyles}</style><slot></slot></>; }

const inputStyles = css`:host { display: block; padding: 0 8px; }`;
export function AppSidebarInput({ value = "", placeholder = "" }: { value?: string; placeholder?: string }): Component { return <><style>{inputStyles}</style><AppInput value={value} placeholder={placeholder}></AppInput></>; }

const separatorStyles = css`:host { display: block; padding: 0 8px; } hr { background: var(--sidebar-border); border: 0; height: 1px; margin: 0; }`;
export function AppSidebarSeparator(): Component { return <><style>{separatorStyles}</style><hr /></>; }

const skeletonStyles = css`
  :host { align-items: center; display: flex; gap: 8px; height: 32px; padding: 0 8px; }
  app-skeleton:first-child { flex: 0 0 16px; height: 16px; }
  app-skeleton:last-child { flex: 1; height: 14px; }
`;

export function AppSidebarMenuSkeleton(): Component { return <><style>{skeletonStyles}</style><AppSkeleton></AppSkeleton><AppSkeleton></AppSkeleton></>; }

const railStyles = css`
  :host { bottom: 0; display: block; position: absolute; right: -4px; top: 0; width: 8px; }
  button { background: transparent; border: 0; cursor: ew-resize; height: 100%; padding: 0; width: 100%; }
  button:hover::after { background: var(--sidebar-border); bottom: 0; content: ""; left: 3px; position: absolute; top: 0; width: 1px; }
`;

export function AppSidebarRail(): Component { return <><style>{railStyles}</style><button type="button" aria-label="Toggle sidebar" onClick={() => emit(this, "toggle")}></button></>; }
