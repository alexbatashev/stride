/*
 * Design and functionality adapted from shadcn/ui sidebar component.
 * shadcn/ui — MIT License — Copyright (c) 2023 shadcn
 * https://ui.shadcn.com/docs/components/sidebar
 */

import { LitElement, css, html } from "lit";
import { customElement, state, property } from "lit/decorators.js";

type SidebarStatus = "open" | "collapsed" | "hidden";

const SIDEBAR_TOGGLE_EVENT = "app-sidebar-toggle";
const SIDEBAR_STATE_EVENT = "app-sidebar-state";
const MOBILE_QUERY = "(max-width: 767px)";
const SIDEBAR_WIDTH = "260px";
const SIDEBAR_COLLAPSED_WIDTH = "48px";

@customElement("app-sidebar")
export class AppSidebar extends LitElement {
  @state()
  status: SidebarStatus = "open";

  private mediaQuery?: MediaQueryList;
  private originalParentStyle?: {
    display: string;
    minWidth: string;
    maxWidth: string;
    width: string;
    position: string;
    inset: string;
    zIndex: string;
  };

  private readonly toggleSidebar = () => {
    if (this.isMobile()) {
      this.status = this.status === "open" ? "hidden" : "open";
    } else {
      this.status = this.status === "collapsed" ? "open" : "collapsed";
    }
  };

  private readonly syncToViewport = () => {
    this.status = this.isMobile() ? "hidden" : "open";
  };

  static styles = css`
    :host {
      background: var(--sidebar-bg, var(--secondary));
      border-right: 1px solid var(--sidebar-border, var(--border));
      box-sizing: border-box;
      display: flex;
      flex-direction: column;
      align-items: stretch;
      height: 100%;
      overflow: hidden;
      position: relative;
      transition: width 180ms ease;
      width: 100%;
    }

    :host([status="collapsed"]) {
      width: 48px;
    }

    :host([status="hidden"]) {
      display: none;
    }

    .scrim {
      border: 0;
      display: none;
      padding: 0;
    }

    .header {
      height: 48px;
      width: 100%;
      flex: 0 0 auto;
    }

    .main {
      flex: 1;
      overflow: auto;
      padding: 8px 0;
      width: 100%;
    }

    .footer {
      height: 64px;
      flex: 0 0 auto;
      width: 100%;
    }

    :host([status="collapsed"]) .footer,
    :host([status="collapsed"]) ::slotted(app-sidebar-group) {
      display: none;
    }

    @media (max-width: 767px) {
      :host {
        box-shadow: 0 18px 48px rgb(0 0 0 / 22%);
        max-width: 320px;
        width: min(84vw, 320px);
      }

      .scrim {
        background: rgb(0 0 0 / 36%);
        display: block;
        inset: 0;
        position: fixed;
        z-index: -1;
      }
    }
  `;

  connectedCallback() {
    super.connectedCallback();
    this.mediaQuery = window.matchMedia(MOBILE_QUERY);
    this.mediaQuery.addEventListener("change", this.syncToViewport);
    window.addEventListener(SIDEBAR_TOGGLE_EVENT, this.toggleSidebar);
    this.syncToViewport();
  }

  disconnectedCallback() {
    window.removeEventListener(SIDEBAR_TOGGLE_EVENT, this.toggleSidebar);
    this.mediaQuery?.removeEventListener("change", this.syncToViewport);
    this.restoreParentStyle();
    super.disconnectedCallback();
  }

  protected updated() {
    this.setAttribute("status", this.status);
    this.syncSlottedItems();
    this.syncParentStyle();
    window.dispatchEvent(
      new CustomEvent(SIDEBAR_STATE_EVENT, { detail: { status: this.status } }),
    );
  }

  private isMobile() {
    return this.mediaQuery?.matches ?? window.matchMedia(MOBILE_QUERY).matches;
  }

  private syncSlottedItems() {
    const collapsed = this.status === "collapsed";

    for (const item of this.querySelectorAll("app-sidebar-nav-item")) {
      item.toggleAttribute("collapsed", collapsed);
    }
  }

  private syncParentStyle() {
    const parent = this.parentElement;
    if (!parent) {
      return;
    }

    if (!this.originalParentStyle) {
      this.originalParentStyle = {
        display: parent.style.display,
        minWidth: parent.style.minWidth,
        maxWidth: parent.style.maxWidth,
        width: parent.style.width,
        position: parent.style.position,
        inset: parent.style.inset,
        zIndex: parent.style.zIndex,
      };
    }

    if (this.status === "hidden") {
      parent.style.display = "none";
      return;
    }

    parent.style.display = this.originalParentStyle.display || "block";
    parent.style.width =
      this.status === "collapsed" ? SIDEBAR_COLLAPSED_WIDTH : SIDEBAR_WIDTH;
    parent.style.minWidth = parent.style.width;
    parent.style.maxWidth = parent.style.width;

    if (this.isMobile()) {
      parent.style.position = "fixed";
      parent.style.inset = "0 auto 0 0";
      parent.style.zIndex = "50";
    } else {
      parent.style.position = this.originalParentStyle.position;
      parent.style.inset = this.originalParentStyle.inset;
      parent.style.zIndex = this.originalParentStyle.zIndex;
    }
  }

  private restoreParentStyle() {
    const parent = this.parentElement;
    if (!parent || !this.originalParentStyle) {
      return;
    }

    parent.style.display = this.originalParentStyle.display;
    parent.style.minWidth = this.originalParentStyle.minWidth;
    parent.style.maxWidth = this.originalParentStyle.maxWidth;
    parent.style.width = this.originalParentStyle.width;
    parent.style.position = this.originalParentStyle.position;
    parent.style.inset = this.originalParentStyle.inset;
    parent.style.zIndex = this.originalParentStyle.zIndex;
  }

  render() {
    return html`<button
        class="scrim"
        type="button"
        aria-label="Close sidebar"
        @click=${this.toggleSidebar}
      ></button>
      <div class="header">
        <slot name="header"></slot>
      </div>
      <div class="main">
        <slot @slotchange=${this.syncSlottedItems}></slot>
      </div>
      <div class="footer">
        <slot name="footer"></slot>
      </div>`;
  }
}

@customElement("app-sidebar-nav-item")
export class AppSidebarNavItem extends LitElement {
  @property()
  target: string = "/";

  @property({ type: Boolean, reflect: true })
  active: boolean = false;

  @property({ type: Boolean, reflect: true })
  collapsed: boolean = false;

  @state()
  private hasIcon: boolean = false;

  @state()
  private labelText: string = "";

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

    :host([collapsed]) {
      padding: 2px 4px;
    }

    :host([collapsed]) a {
      height: 40px;
      justify-content: center;
      padding: 0;
    }

    :host([collapsed]) .label {
      display: none;
    }

    :host([collapsed]) .icon {
      flex-basis: 32px;
      height: 32px;
      width: 32px;
    }

    :host([collapsed]) .icon ::slotted(*) {
      height: 32px;
      width: 32px;
    }

    @media (max-width: 767px) {
      a {
        font-size: 16px;
        height: 44px;
        padding: 0 12px;
      }

      .icon {
        flex-basis: 20px;
        height: 20px;
        width: 20px;
      }

      .icon ::slotted(*) {
        height: 20px;
        width: 20px;
      }
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
  }

  private onLabelSlotChange(event: Event) {
    const slot = event.target as HTMLSlotElement;
    this.labelText = slot
      .assignedNodes({ flatten: true })
      .map((node) => node.textContent?.trim() ?? "")
      .filter(Boolean)
      .join(" ");
  }

  render() {
    return html`<a
      href="${this.target}"
      aria-current=${this.active ? "page" : "false"}
      aria-label=${this.labelText}
      title=${this.collapsed ? this.labelText : ""}
    >
      <span class="icon" ?hidden=${!this.hasIcon}
        ><slot name="icon" @slotchange=${this.onIconSlotChange}></slot
      ></span>
      <span class="label"
        ><slot @slotchange=${this.onLabelSlotChange}></slot
      ></span>
    </a>`;
  }
}

@customElement("app-sidebar-toggle")
export class AppSidebarToggle extends LitElement {
  @property()
  brand: string = "";

  @state()
  private isClosed: boolean = false;

  private readonly onSidebarState = (event: Event) => {
    const state = event as CustomEvent<{ status: SidebarStatus }>;
    this.isClosed = state.detail.status !== "open";
  };

  static styles = css`
    :host {
      display: inline-flex;
      height: 24px;
      width: 24px;
    }

    :host([brand]) {
      height: 32px;
      width: 32px;
    }

    .brand-mark {
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

    .hover-icon {
      display: none;
    }

    :host([brand]:hover) .brand-mark,
    :host([brand]:focus-within) .brand-mark {
      display: none;
    }

    :host([brand]:hover) .hover-icon,
    :host([brand]:focus-within) .hover-icon {
      align-items: center;
      display: inline-flex;
      height: 24px;
      justify-content: center;
      width: 24px;
    }
  `;

  connectedCallback() {
    super.connectedCallback();
    this.isClosed = window.matchMedia(MOBILE_QUERY).matches;
    window.addEventListener(SIDEBAR_STATE_EVENT, this.onSidebarState);
  }

  disconnectedCallback() {
    window.removeEventListener(SIDEBAR_STATE_EVENT, this.onSidebarState);
    super.disconnectedCallback();
  }

  private toggle() {
    window.dispatchEvent(new CustomEvent(SIDEBAR_TOGGLE_EVENT));
  }

  render() {
    const content =
      this.brand && this.isClosed
        ? html`<span class="brand-mark">${this.brand}</span>
            <span class="hover-icon"><icon-panel-left-open></icon-panel-left-open></span>`
        : this.isClosed
          ? html`<icon-panel-left-open></icon-panel-left-open>`
          : html`<icon-panel-left-close></icon-panel-left-close>`;

    return html`<app-button
      variant="ghost"
      size=${this.brand ? "icon" : "icon-xs"}
      title=${this.isClosed ? "Open sidebar" : "Close sidebar"}
      @click=${this.toggle}
      >${content}</app-button
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

    .chevron > * {
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

    @media (max-width: 767px) {
      button {
        font-size: 16px;
        height: 44px;
        padding: 0 12px;
      }

      ul {
        padding-left: 8px;
      }

      ::slotted(a) {
        font-size: 16px;
        height: 44px;
        padding: 0 12px;
      }
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
        <span class="chevron"
          >${this.is_open ? html`<icon-chevron-down></icon-chevron-down>` : html`<icon-chevron-right></icon-chevron-right>`}</span
        >
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

    @media (max-width: 767px) {
      a {
        font-size: 16px;
        height: 44px;
        padding: 0 12px;
      }
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
