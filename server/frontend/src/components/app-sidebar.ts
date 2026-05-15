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
import { PANEL_LEFT_CLOSE, PANEL_LEFT_OPEN } from "./icons";

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

  static styles = css`
    :host {
      height: 24px;
      width: 100%;
      padding: 8px;
      display: block;
    }

    a:hover {
      background-color: var(--secondary-hover);
    }

    a {
      width: 100%;
      height: 100%;
      display: block;
      border-radius: 8px;
      text-decoration: none;
      color: var(--foreground);
    }
  `;

  render() {
    return html`<a href="${this.target}"><slot></slot></a>`;
  }
}

@customElement("app-sidebar-toggle")
export class AppSidebarToggle extends LitElement {
  @state()
  is_closed: boolean = false;

  render() {
    return html`<app-button size="icon"
      >${this.is_closed ? PANEL_LEFT_OPEN : PANEL_LEFT_CLOSE}</app-button
    >`;
  }
}
