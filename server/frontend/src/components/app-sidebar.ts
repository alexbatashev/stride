/*
 * Design and functionality adapted from shadcn/ui sidebar component.
 * shadcn/ui — MIT License — Copyright (c) 2023 shadcn
 * https://ui.shadcn.com/docs/components/sidebar
 */

import { LitElement, css, html } from "lit";
import {
  customElement,
  state,
  property,
  eventOptions,
  query,
} from "lit/decorators.js";
import {
  CHEVRON_DOWN,
  CHEVRON_RIGHT,
  PANEL_LEFT_CLOSE,
  PANEL_LEFT_OPEN,
} from "./icons";

type SidebarStatus = "open" | "collapsed" | "hidden";

@customElement("app-sidebar")
export class AppSidebar extends LitElement {
  @state()
  status: SidebarStatus = "open";

  static styles = css`
    :host {
      width: 100%;
      height: 100%;
      display: flex;
      flex-direction: column;
      align-items: stretch;
    }
    .header {
      height: 48px;
      width: 100%;
    }

    .main {
      flex: 1;
      width: 100%;
    }

    .footer {
      height: 64px;
      width: 100%;
    }
  `;

  render() {
    return html`<div class="header"></div>
      <div class="main">
        <slot></slot>
      </div>
      <div class="footer"></div>`;
  }
}

@customElement("app-sidebar-nav-item")
export class AppSidebarNavItem extends LitElement {
  @property()
  target: string = "/";

  @property({ type: Boolean, reflect: true })
  active: boolean = false;

  @state()
  private hasIcon: boolean = false;

  static styles = css`
    :host {
      box-sizing: border-box;
      display: block;
      padding: 0 8px;
      width: 100%;
    }

    a {
      align-items: center;
      border-radius: 8px;
      box-sizing: border-box;
      color: var(--sidebar-fg, var(--foreground));
      display: flex;
      font-size: 14px;
      font-weight: 400;
      gap: 8px;
      height: 32px;
      line-height: 20px;
      outline: none;
      overflow: hidden;
      padding: 0 8px;
      text-align: left;
      text-decoration: none;
      transition:
        background-color 140ms ease,
        color 140ms ease;
      user-select: none;
      white-space: nowrap;
      width: 100%;
    }

    a:hover,
    a[aria-current="page"] {
      background: var(--sidebar-accent, var(--accent));
      color: var(--sidebar-accent-fg, var(--accent-foreground));
    }

    a[aria-current="page"] {
      font-weight: 500;
    }

    a:focus-visible {
      box-shadow: 0 0 0 2px var(--ring-shadow, rgb(24 24 27 / 12%));
    }

    .icon {
      align-items: center;
      display: inline-flex;
      flex: 0 0 16px;
      height: 16px;
      justify-content: center;
      width: 16px;
    }

    .icon[hidden] {
      display: none;
    }

    .icon ::slotted(*) {
      align-items: center;
      display: inline-flex;
      flex-shrink: 0;
      height: 16px;
      justify-content: center;
      width: 16px;
    }

    .label {
      flex: 1;
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
    }
  `;

  private onIconSlotChange(event: Event) {
    const slot = event.target as HTMLSlotElement;
    const nodes = slot.assignedNodes({ flatten: true });
    this.hasIcon = nodes.some((node) => {
      return (
        node.nodeType === Node.ELEMENT_NODE ||
        (node.nodeType === Node.TEXT_NODE && !!node.textContent?.trim())
      );
    });

    for (const element of slot.assignedElements({ flatten: true })) {
      const svgs =
        element instanceof SVGSVGElement
          ? [element]
          : Array.from(element.querySelectorAll("svg"));

      for (const svg of svgs) {
        svg.style.width = "16px";
        svg.style.height = "16px";
        svg.style.flexShrink = "0";
      }
    }
  }

  render() {
    return html`<a
      href="${this.target}"
      aria-current=${this.active ? "page" : "false"}
    >
      <span class="icon" ?hidden=${!this.hasIcon}
        ><slot name="icon" @slotchange=${this.onIconSlotChange}></slot
      ></span>
      <span class="label"><slot></slot></span>
    </a>`;
  }
}

@customElement("app-sidebar-toggle")
export class AppSidebarToggle extends LitElement {
  @state()
  is_closed: boolean = false;

  static styles = css`
    :host {
      display: inline-flex;
      height: 24px;
      width: 24px;
    }
  `;

  render() {
    return html`<app-button variant="ghost" size="icon-xs"
      >${this.is_closed ? PANEL_LEFT_OPEN : PANEL_LEFT_CLOSE}</app-button
    >`;
  }
}

@customElement("app-sidebar-group")
export class AppSidebarGroup extends LitElement {
  @state()
  is_open: boolean = true;

  @property()
  title: string = "Group";

  static styles = css`
    :host {
      width: 100%;
      padding: 8px 0;
    }

    button {
      align-items: center;
      background: transparent;
      border: 0;
      border-radius: 8px;
      box-sizing: border-box;
      color: var(--sidebar-fg, var(--foreground));
      cursor: pointer;
      display: flex;
      font: inherit;
      font-size: 14px;
      font-weight: 500;
      gap: 8px;
      height: 32px;
      line-height: 20px;
      margin: 0 8px;
      outline: none;
      padding: 0 8px;
      text-align: left;
      transition:
        background-color 140ms ease,
        color 140ms ease;
      user-select: none;
      width: calc(100% - 16px);
    }

    button:hover {
      background: var(--sidebar-accent, var(--accent));
      color: var(--sidebar-accent-fg, var(--accent-foreground));
    }

    button:focus-visible {
      box-shadow: 0 0 0 2px var(--ring-shadow, rgb(24 24 27 / 12%));
    }

    .title {
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }

    .chevron {
      align-items: center;
      display: inline-flex;
      flex: 0 0 1em;
      height: 1em;
      justify-content: center;
      opacity: 0;
      transition: opacity 140ms ease;
      width: 1em;
    }

    button:hover .chevron,
    button:focus-visible .chevron {
      opacity: 1;
    }

    .chevron svg {
      height: 1em;
      width: 1em;
    }

    ul {
      box-sizing: border-box;
      display: flex;
      flex-direction: column;
      gap: 2px;
      list-style-type: none;
      margin: 4px 0 0;
      padding: 0 8px 0 20px;
      width: 100%;
    }

    ul[hidden] {
      display: none;
    }

    ::slotted(li) {
      display: block;
      width: 100%;
    }

    ::slotted(a) {
      align-items: center;
      border-radius: 8px;
      box-sizing: border-box;
      color: var(--sidebar-fg, var(--foreground));
      display: flex;
      font-size: 14px;
      font-weight: 400;
      height: 32px;
      line-height: 20px;
      outline: none;
      overflow: hidden;
      padding: 0 8px;
      text-align: left;
      text-decoration: none;
      transition:
        background-color 140ms ease,
        color 140ms ease;
      user-select: none;
      white-space: nowrap;
      width: 100%;
    }

    ::slotted(a:hover),
    ::slotted(a[aria-current="page"]) {
      background: var(--sidebar-accent, var(--accent));
      color: var(--sidebar-accent-fg, var(--accent-foreground));
    }
  `;

  private toggle() {
    this.is_open = !this.is_open;
  }

  render() {
    return html`
      <button
        type="button"
        aria-expanded=${this.is_open ? "true" : "false"}
        @click=${this.toggle}
      >
        <span class="title">${this.title}</span>
        <span class="chevron">${this.is_open ? CHEVRON_DOWN : CHEVRON_RIGHT}</span>
      </button>
      <ul ?hidden=${!this.is_open}>
        <slot></slot>
      </ul>
    `;
  }
}

@customElement("app-sidebar-group-item")
export class AppSidebarGroupItem extends LitElement {
  @property()
  target: string = "/";

  @property({ type: Boolean, reflect: true })
  active: boolean = false;

  static styles = css`
    :host {
      box-sizing: border-box;
      display: block;
      width: 100%;
    }

    a {
      align-items: center;
      border-radius: 8px;
      box-sizing: border-box;
      color: var(--sidebar-fg, var(--foreground));
      display: flex;
      font-size: 14px;
      font-weight: 400;
      gap: 8px;
      height: 32px;
      line-height: 20px;
      outline: none;
      overflow: hidden;
      padding: 0 8px;
      text-align: left;
      text-decoration: none;
      transition:
        background-color 140ms ease,
        color 140ms ease;
      user-select: none;
      white-space: nowrap;
      width: 100%;
    }

    a:hover,
    a[aria-current="page"] {
      background: var(--sidebar-accent, var(--accent));
      color: var(--sidebar-accent-fg, var(--accent-foreground));
    }

    a[aria-current="page"] {
      font-weight: 500;
    }

    a:focus-visible {
      box-shadow: 0 0 0 2px var(--ring-shadow, rgb(24 24 27 / 12%));
    }

    .label {
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
    }
  `;

  render() {
    return html`<a
      href=${this.target}
      aria-current=${this.active ? "page" : "false"}
    >
      <span class="label"><slot></slot></span>
    </a>`;
  }
}
