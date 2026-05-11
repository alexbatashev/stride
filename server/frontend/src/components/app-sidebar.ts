/*
 * Design and functionality adapted from shadcn/ui sidebar component.
 * shadcn/ui — MIT License — Copyright (c) 2023 shadcn
 * https://ui.shadcn.com/docs/components/sidebar
 *
 * Reimplemented as Lit web components. Not a direct port — reimplemented from scratch
 * in a different framework while preserving the design system, API shape, and UX behaviour.
 */

import {LitElement, css, html, nothing} from 'lit';

const COOKIE = 'sidebar_state';
const COOKIE_MAX_AGE = 60 * 60 * 24 * 7;
const SHORTCUT = 'b'; // Cmd+B / Ctrl+B
const MOBILE_MAX_PX = 767;

const PANEL_LEFT_ICON = html`
	<svg
		xmlns="http://www.w3.org/2000/svg"
		width="16"
		height="16"
		viewBox="0 0 24 24"
		fill="none"
		stroke="currentColor"
		stroke-width="2"
		stroke-linecap="round"
		stroke-linejoin="round"
		aria-hidden="true"
	>
		<rect width="18" height="18" x="3" y="3" rx="2" />
		<path d="M9 3v18" />
	</svg>
`;

// ─── Provider ─────────────────────────────────────────────────────────────────
//
// Manages open/collapsed state. Broadcasts to child app-sidebar elements
// and persists state in a cookie. Wires the Cmd+B keyboard shortcut.

export class AppSidebarProvider extends LitElement {
	static properties = {
		open: {type: Boolean, reflect: true},
		defaultOpen: {type: Boolean, attribute: 'default-open'},
	};

	open = true;
	defaultOpen = true;

	private _isMobile = false;
	private _mq?: MediaQueryList;

	private _mqListener = (e: MediaQueryListEvent) => {
		this._isMobile = e.matches;
		this._broadcast();
	};

	private _keyListener = (e: KeyboardEvent) => {
		if (e.key === SHORTCUT && (e.metaKey || e.ctrlKey)) {
			e.preventDefault();
			this.toggle();
		}
	};

	connectedCallback() {
		super.connectedCallback();
		const m = document.cookie.match(new RegExp(`${COOKIE}=(true|false)`));
		this.open = m ? m[1] === 'true' : this.defaultOpen;

		this._mq = window.matchMedia(`(max-width: ${MOBILE_MAX_PX}px)`);
		this._isMobile = this._mq.matches;
		this._mq.addEventListener('change', this._mqListener);
		window.addEventListener('keydown', this._keyListener);

		// Defer so slotted children have time to connect.
		queueMicrotask(() => this._broadcast());
	}

	disconnectedCallback() {
		super.disconnectedCallback();
		this._mq?.removeEventListener('change', this._mqListener);
		window.removeEventListener('keydown', this._keyListener);
	}

	toggle() {
		this.open = !this.open;
		document.cookie = `${COOKIE}=${this.open}; path=/; max-age=${COOKIE_MAX_AGE}`;
		this._broadcast();
		this.dispatchEvent(
			new CustomEvent('sidebar-toggle', {
				bubbles: true,
				composed: true,
				detail: {open: this.open},
			})
		);
	}

	private _broadcast() {
		this.querySelectorAll('app-sidebar').forEach((el) => {
			(el as AppSidebar)._sync(this.open, this._isMobile);
		});
	}

	static styles = css`
		:host {
			/* Design tokens — override these to theme the sidebar */
			--sidebar-width: 16rem;
			--sidebar-width-icon: 3rem;
			--sidebar-bg: hsl(0 0% 98%);
			--sidebar-fg: hsl(240 5.3% 26.1%);
			--sidebar-accent: hsl(240 4.8% 95.9%);
			--sidebar-accent-fg: hsl(240 5.9% 10%);
			--sidebar-border: hsl(220 13% 91%);
			--sidebar-ring: hsl(217.2 91.2% 59.8%);

			display: flex;
			min-height: 100svh;
			width: 100%;
		}
	`;

	render() {
		return html`<slot></slot>`;
	}
}

customElements.define('app-sidebar-provider', AppSidebarProvider);

// ─── Sidebar ──────────────────────────────────────────────────────────────────
//
// The sidebar panel. Syncs with the provider, handles desktop gap + fixed
// panel layout, mobile sheet overlay, and icon-collapse sub-component updates.

export class AppSidebar extends LitElement {
	static properties = {
		side: {type: String},
		variant: {type: String},
		collapsible: {type: String},
		_state: {state: true},
		_mobile: {state: true},
		_mobileOpen: {state: true},
	};

	side: 'left' | 'right' = 'left';
	variant: 'sidebar' | 'floating' | 'inset' = 'sidebar';
	collapsible: 'offcanvas' | 'icon' | 'none' = 'offcanvas';

	private _state: 'expanded' | 'collapsed' = 'expanded';
	private _mobile = false;
	private _mobileOpen = false;

	/** Called by AppSidebarProvider when open/mobile state changes. */
	_sync(open: boolean, isMobile: boolean) {
		this._mobile = isMobile;
		if (isMobile) {
			this._mobileOpen = open;
		} else {
			this._state = open ? 'expanded' : 'collapsed';
		}
		this._updateIconCollapsed();
	}

	// Propagate icon-collapsed state to sub-components that need to hide.
	private _updateIconCollapsed() {
		const iconCollapsed =
			!this._mobile && this._state === 'collapsed' && this.collapsible === 'icon';

		const targets = [
			'app-sidebar-group-label',
			'app-sidebar-menu-badge',
			'app-sidebar-menu-action',
			'app-sidebar-menu-sub',
		];
		targets.forEach((tag) => {
			this.querySelectorAll(tag).forEach((el) => {
				if (iconCollapsed) {
					el.setAttribute('data-icon-collapsed', '');
				} else {
					el.removeAttribute('data-icon-collapsed');
				}
			});
		});
	}

	private _closeOverlay() {
		const provider = this.closest('app-sidebar-provider');
		if (provider && this._mobileOpen) (provider as AppSidebarProvider).toggle();
	}

	static styles = css`
		:host {
			display: contents;
		}

		/* ── static (collapsible=none) ── */
		.static {
			display: flex;
			flex-direction: column;
			height: 100%;
			width: var(--sidebar-width);
			background: var(--sidebar-bg);
			color: var(--sidebar-fg);
			overflow: hidden;
		}

		/* ── desktop gap: reserves space next to the fixed panel ── */
		.gap {
			flex-shrink: 0;
			width: var(--sidebar-width);
			background: transparent;
			transition: width 200ms ease-linear;
		}

		.gap.offcanvas-collapsed {
			width: 0;
		}

		.gap.icon-collapsed {
			width: var(--sidebar-width-icon);
		}

		/* ── desktop fixed panel ── */
		.panel {
			position: fixed;
			inset-block: 0;
			z-index: 10;
			display: none;
			height: 100svh;
			width: var(--sidebar-width);
			transition:
				left 200ms ease-linear,
				right 200ms ease-linear,
				width 200ms ease-linear;
		}

		@media (min-width: 768px) {
			.panel {
				display: flex;
			}
		}

		.panel.left {
			left: 0;
		}

		.panel.right {
			right: 0;
		}

		/* sidebar variant: show border on the inner edge */
		.panel.sidebar.left {
			border-right: 1px solid var(--sidebar-border);
		}

		.panel.sidebar.right {
			border-left: 1px solid var(--sidebar-border);
		}

		/* floating/inset variants: padding, no border (border is on .inner) */
		.panel.floating,
		.panel.inset {
			padding: 0.5rem;
		}

		/* offcanvas collapse: slide out */
		.panel.left.offcanvas-collapsed {
			left: calc(var(--sidebar-width) * -1);
		}

		.panel.right.offcanvas-collapsed {
			right: calc(var(--sidebar-width) * -1);
		}

		/* icon collapse: shrink to icon width */
		.panel.icon-collapsed {
			width: var(--sidebar-width-icon);
		}

		.panel.floating.icon-collapsed,
		.panel.inset.icon-collapsed {
			width: calc(var(--sidebar-width-icon) + 1rem + 2px);
		}

		/* inner flex container */
		.inner {
			display: flex;
			flex-direction: column;
			height: 100%;
			width: 100%;
			background: var(--sidebar-bg);
			color: var(--sidebar-fg);
			overflow: hidden;
		}

		.floating .inner {
			border-radius: 0.5rem;
			border: 1px solid var(--sidebar-border);
			box-shadow: 0 1px 2px 0 rgb(0 0 0 / 0.05);
		}

		/* ── mobile sheet overlay ── */
		.backdrop {
			position: fixed;
			inset: 0;
			z-index: 49;
			background: rgb(0 0 0 / 0.5);
			cursor: pointer;
		}

		.sheet {
			position: fixed;
			inset-block: 0;
			z-index: 50;
			display: flex;
			flex-direction: column;
			width: 18rem;
			background: var(--sidebar-bg);
			color: var(--sidebar-fg);
			transition: transform 200ms ease-in-out;
			overflow: hidden;
		}

		.sheet.left {
			left: 0;
			transform: translateX(-100%);
		}

		.sheet.right {
			right: 0;
			transform: translateX(100%);
		}

		.sheet.open {
			transform: translateX(0);
		}
	`;

	render() {
		if (this.collapsible === 'none') {
			return html`
				<div class="static" part="sidebar">
					<slot></slot>
				</div>
			`;
		}

		if (this._mobile) {
			return html`
				${this._mobileOpen
					? html`<div class="backdrop" @click=${this._closeOverlay}></div>`
					: nothing}
				<div
					class=${`sheet ${this.side}${this._mobileOpen ? ' open' : ''}`}
					part="sidebar"
					data-mobile="true"
				>
					<slot></slot>
				</div>
			`;
		}

		const collapsed = this._state === 'collapsed';
		const offcanvas = collapsed && this.collapsible === 'offcanvas';
		const icon = collapsed && this.collapsible === 'icon';

		const panelCls = [
			'panel',
			this.side,
			this.variant,
			offcanvas ? 'offcanvas-collapsed' : '',
			icon ? 'icon-collapsed' : '',
		]
			.filter(Boolean)
			.join(' ');

		const gapCls = [
			'gap',
			offcanvas ? 'offcanvas-collapsed' : '',
			icon ? 'icon-collapsed' : '',
		]
			.filter(Boolean)
			.join(' ');

		return html`
			<div class=${gapCls} aria-hidden="true"></div>
			<div class=${panelCls} part="sidebar">
				<div class="inner">
					<slot></slot>
				</div>
			</div>
		`;
	}
}

customElements.define('app-sidebar', AppSidebar);

// ─── Trigger ──────────────────────────────────────────────────────────────────

export class AppSidebarTrigger extends LitElement {
	private _toggle() {
		(this.closest('app-sidebar-provider') as AppSidebarProvider | null)?.toggle();
	}

	static styles = css`
		:host {
			display: inline-flex;
		}

		button {
			display: inline-flex;
			align-items: center;
			justify-content: center;
			width: 1.75rem;
			height: 1.75rem;
			padding: 0;
			background: transparent;
			border: none;
			border-radius: 0.375rem;
			cursor: pointer;
			color: inherit;
			outline: none;
		}

		button:hover {
			background: var(--sidebar-accent, hsl(240 4.8% 95.9%));
			color: var(--sidebar-accent-fg, hsl(240 5.9% 10%));
		}

		button:focus-visible {
			outline: 2px solid var(--sidebar-ring, hsl(217.2 91.2% 59.8%));
			outline-offset: 2px;
		}

		.sr-only {
			position: absolute;
			width: 1px;
			height: 1px;
			padding: 0;
			margin: -1px;
			overflow: hidden;
			clip: rect(0, 0, 0, 0);
			white-space: nowrap;
			border-width: 0;
		}
	`;

	render() {
		return html`
			<button @click=${this._toggle} aria-label="Toggle sidebar" title="Toggle sidebar (Ctrl+B / ⌘B)">
				${PANEL_LEFT_ICON}
				<span class="sr-only">Toggle sidebar</span>
			</button>
		`;
	}
}

customElements.define('app-sidebar-trigger', AppSidebarTrigger);

// ─── Rail ─────────────────────────────────────────────────────────────────────
//
// Thin clickable strip on the sidebar's inner edge. Click to toggle.

export class AppSidebarRail extends LitElement {
	connectedCallback() {
		super.connectedCallback();
		const sidebar = this.closest('app-sidebar');
		if (sidebar && (sidebar as AppSidebar).side === 'right') {
			this.setAttribute('data-side', 'right');
		} else {
			this.setAttribute('data-side', 'left');
		}
	}

	private _toggle() {
		(this.closest('app-sidebar-provider') as AppSidebarProvider | null)?.toggle();
	}

	static styles = css`
		:host {
			position: absolute;
			inset-block: 0;
			z-index: 20;
			width: 1rem;
			display: none;
		}

		@media (min-width: 640px) {
			:host {
				display: flex;
				align-items: center;
			}
		}

		:host([data-side='left']) {
			right: -0.5rem;
			cursor: w-resize;
		}

		:host([data-side='right']) {
			left: -0.5rem;
			cursor: e-resize;
		}

		button {
			position: absolute;
			inset-block: 0;
			left: 50%;
			width: 2px;
			padding: 0;
			background: transparent;
			border: none;
			cursor: inherit;
			transition: background 200ms ease-linear;
		}

		:host(:hover) button {
			background: var(--sidebar-border, hsl(220 13% 91%));
		}
	`;

	render() {
		return html`
			<button @click=${this._toggle} aria-label="Toggle sidebar" tabindex="-1" title="Toggle sidebar"></button>
		`;
	}
}

customElements.define('app-sidebar-rail', AppSidebarRail);

// ─── Inset ────────────────────────────────────────────────────────────────────
//
// The main content area rendered beside the sidebar.

export class AppSidebarInset extends LitElement {
	static styles = css`
		:host {
			display: flex;
			flex-direction: column;
			flex: 1 1 0%;
			width: 100%;
			min-height: 0;
			background: hsl(0 0% 100%);
			position: relative;
			overflow: hidden;
		}
	`;

	render() {
		return html`<slot></slot>`;
	}
}

customElements.define('app-sidebar-inset', AppSidebarInset);

// ─── Header / Content / Footer / Separator ────────────────────────────────────

export class AppSidebarHeader extends LitElement {
	static styles = css`
		:host {
			display: flex;
			flex-direction: column;
			gap: 0.5rem;
			padding: 0.5rem;
		}
	`;

	render() {
		return html`<slot></slot>`;
	}
}

customElements.define('app-sidebar-header', AppSidebarHeader);

export class AppSidebarContent extends LitElement {
	static styles = css`
		:host {
			display: flex;
			flex-direction: column;
			gap: 0.5rem;
			flex: 1 1 0%;
			min-height: 0;
			overflow-y: auto;
			overflow-x: hidden;
		}
	`;

	render() {
		return html`<slot></slot>`;
	}
}

customElements.define('app-sidebar-content', AppSidebarContent);

export class AppSidebarFooter extends LitElement {
	static styles = css`
		:host {
			display: flex;
			flex-direction: column;
			gap: 0.5rem;
			padding: 0.5rem;
		}
	`;

	render() {
		return html`<slot></slot>`;
	}
}

customElements.define('app-sidebar-footer', AppSidebarFooter);

export class AppSidebarSeparator extends LitElement {
	static styles = css`
		:host {
			display: block;
			margin: 0 0.5rem;
			height: 1px;
			background: var(--sidebar-border, hsl(220 13% 91%));
		}
	`;

	render() {
		return html``;
	}
}

customElements.define('app-sidebar-separator', AppSidebarSeparator);

// ─── Group ────────────────────────────────────────────────────────────────────

export class AppSidebarGroup extends LitElement {
	static styles = css`
		:host {
			display: flex;
			flex-direction: column;
			width: 100%;
			min-width: 0;
			padding: 0.5rem;
			position: relative;
		}
	`;

	render() {
		return html`<slot></slot>`;
	}
}

customElements.define('app-sidebar-group', AppSidebarGroup);

export class AppSidebarGroupLabel extends LitElement {
	static styles = css`
		:host {
			display: flex;
			align-items: center;
			height: 2rem;
			padding: 0 0.5rem;
			font-size: 0.75rem;
			font-weight: 500;
			color: hsl(240 5.3% 26.1% / 0.7);
			border-radius: 0.375rem;
			flex-shrink: 0;
			overflow: hidden;
			white-space: nowrap;
			transition:
				margin-top 200ms ease-linear,
				opacity 200ms ease-linear;
		}

		/* Slide up and fade out when sidebar is in icon-collapsed mode */
		:host([data-icon-collapsed]) {
			margin-top: -2rem;
			opacity: 0;
			pointer-events: none;
		}

		::slotted(svg) {
			width: 1rem;
			height: 1rem;
			flex-shrink: 0;
		}
	`;

	render() {
		return html`<slot></slot>`;
	}
}

customElements.define('app-sidebar-group-label', AppSidebarGroupLabel);

export class AppSidebarGroupContent extends LitElement {
	static styles = css`
		:host {
			display: block;
			width: 100%;
			font-size: 0.875rem;
		}
	`;

	render() {
		return html`<slot></slot>`;
	}
}

customElements.define('app-sidebar-group-content', AppSidebarGroupContent);

// ─── Menu ─────────────────────────────────────────────────────────────────────

export class AppSidebarMenu extends LitElement {
	static styles = css`
		:host {
			display: flex;
			flex-direction: column;
			gap: 0.25rem;
			width: 100%;
			min-width: 0;
			list-style: none;
			margin: 0;
			padding: 0;
		}
	`;

	render() {
		return html`<slot></slot>`;
	}
}

customElements.define('app-sidebar-menu', AppSidebarMenu);

export class AppSidebarMenuItem extends LitElement {
	static styles = css`
		:host {
			display: block;
			position: relative;
		}
	`;

	render() {
		return html`<slot></slot>`;
	}
}

customElements.define('app-sidebar-menu-item', AppSidebarMenuItem);

// ─── Menu Button ──────────────────────────────────────────────────────────────
//
// Renders as a <button> by default, or <a> when `href` is set.
// Use the `tooltip` attribute to show a label in icon-collapsed mode.

export class AppSidebarMenuButton extends LitElement {
	static properties = {
		active: {type: Boolean, reflect: true},
		size: {type: String},
		variant: {type: String},
		tooltip: {type: String},
		href: {type: String},
	};

	active = false;
	size: 'sm' | 'default' | 'lg' = 'default';
	variant: 'default' | 'outline' = 'default';
	tooltip = '';
	href = '';

	static styles = css`
		:host {
			display: block;
		}

		button,
		a {
			display: flex;
			align-items: center;
			gap: 0.5rem;
			width: 100%;
			min-width: 0;
			padding: 0.5rem;
			background: transparent;
			border: none;
			border-radius: 0.375rem;
			cursor: pointer;
			font: inherit;
			font-size: 0.875rem;
			text-align: left;
			text-decoration: none;
			color: inherit;
			overflow: hidden;
			white-space: nowrap;
			outline: none;
			transition:
				background 150ms,
				color 150ms,
				width 200ms ease-linear,
				height 200ms ease-linear,
				padding 200ms ease-linear;
		}

		button:hover,
		a:hover {
			background: var(--sidebar-accent, hsl(240 4.8% 95.9%));
			color: var(--sidebar-accent-fg, hsl(240 5.9% 10%));
		}

		button:focus-visible,
		a:focus-visible {
			outline: 2px solid var(--sidebar-ring, hsl(217.2 91.2% 59.8%));
			outline-offset: -2px;
		}

		:host([active]) button,
		:host([active]) a {
			background: var(--sidebar-accent, hsl(240 4.8% 95.9%));
			color: var(--sidebar-accent-fg, hsl(240 5.9% 10%));
			font-weight: 500;
		}

		/* sizes */
		.sm {
			height: 1.75rem;
			font-size: 0.75rem;
		}

		.default {
			height: 2rem;
		}

		.lg {
			height: 3rem;
		}

		/* outline variant */
		.outline {
			background: hsl(0 0% 100%);
			box-shadow: 0 0 0 1px var(--sidebar-border, hsl(220 13% 91%));
		}

		.outline:hover {
			box-shadow: 0 0 0 1px var(--sidebar-accent, hsl(240 4.8% 95.9%));
		}

		::slotted(svg) {
			width: 1rem;
			height: 1rem;
			flex-shrink: 0;
		}
	`;

	render() {
		const cls = [this.size, this.variant !== 'default' ? this.variant : '']
			.filter(Boolean)
			.join(' ');

		// Native title acts as a tooltip in icon-collapsed mode.
		const title = this.tooltip || nothing;

		if (this.href) {
			return html`
				<a href=${this.href} class=${cls} title=${title} ?aria-current=${this.active}>
					<slot></slot>
				</a>
			`;
		}
		return html`
			<button class=${cls} title=${title} type="button" ?aria-pressed=${this.active}>
				<slot></slot>
			</button>
		`;
	}
}

customElements.define('app-sidebar-menu-button', AppSidebarMenuButton);

// ─── Menu Action ──────────────────────────────────────────────────────────────
//
// Secondary action button overlaid on a menu item (e.g. "…" menu).

export class AppSidebarMenuAction extends LitElement {
	static styles = css`
		:host {
			position: absolute;
			top: 0.375rem;
			right: 0.25rem;
			display: flex;
			align-items: center;
			justify-content: center;
			transition: opacity 150ms;
		}

		:host([data-icon-collapsed]) {
			display: none;
		}

		button {
			display: flex;
			align-items: center;
			justify-content: center;
			width: 1.25rem;
			height: 1.25rem;
			padding: 0;
			background: transparent;
			border: none;
			border-radius: 0.375rem;
			cursor: pointer;
			color: var(--sidebar-fg, hsl(240 5.3% 26.1%));
			outline: none;
		}

		button:hover {
			background: var(--sidebar-accent, hsl(240 4.8% 95.9%));
			color: var(--sidebar-accent-fg, hsl(240 5.9% 10%));
		}

		button:focus-visible {
			outline: 2px solid var(--sidebar-ring, hsl(217.2 91.2% 59.8%));
		}

		::slotted(svg) {
			width: 1rem;
			height: 1rem;
			flex-shrink: 0;
		}
	`;

	render() {
		return html`<button type="button"><slot></slot></button>`;
	}
}

customElements.define('app-sidebar-menu-action', AppSidebarMenuAction);

// ─── Menu Badge ───────────────────────────────────────────────────────────────

export class AppSidebarMenuBadge extends LitElement {
	static styles = css`
		:host {
			position: absolute;
			right: 0.25rem;
			top: 50%;
			transform: translateY(-50%);
			display: flex;
			align-items: center;
			justify-content: center;
			min-width: 1.25rem;
			height: 1.25rem;
			padding: 0 0.25rem;
			border-radius: 0.375rem;
			font-size: 0.75rem;
			font-weight: 500;
			pointer-events: none;
			user-select: none;
			font-variant-numeric: tabular-nums;
			color: var(--sidebar-fg, hsl(240 5.3% 26.1%));
			transition: opacity 200ms ease-linear;
		}

		:host([data-icon-collapsed]) {
			opacity: 0;
			pointer-events: none;
		}
	`;

	render() {
		return html`<slot></slot>`;
	}
}

customElements.define('app-sidebar-menu-badge', AppSidebarMenuBadge);

// ─── Sub-menu ─────────────────────────────────────────────────────────────────

export class AppSidebarMenuSub extends LitElement {
	static styles = css`
		:host {
			display: flex;
			flex-direction: column;
			gap: 0.25rem;
			min-width: 0;
			margin-left: 0.875rem;
			padding: 0.125rem 0.625rem;
			border-left: 1px solid var(--sidebar-border, hsl(220 13% 91%));
			list-style: none;
			overflow: hidden;
			transition:
				height 200ms ease-linear,
				opacity 200ms ease-linear;
		}

		:host([data-icon-collapsed]) {
			display: none;
		}
	`;

	render() {
		return html`<slot></slot>`;
	}
}

customElements.define('app-sidebar-menu-sub', AppSidebarMenuSub);

export class AppSidebarMenuSubItem extends LitElement {
	static styles = css`
		:host {
			display: block;
			position: relative;
		}
	`;

	render() {
		return html`<slot></slot>`;
	}
}

customElements.define('app-sidebar-menu-sub-item', AppSidebarMenuSubItem);

export class AppSidebarMenuSubButton extends LitElement {
	static properties = {
		active: {type: Boolean, reflect: true},
		size: {type: String},
		href: {type: String},
	};

	active = false;
	size: 'sm' | 'md' = 'md';
	href = '';

	static styles = css`
		:host {
			display: block;
		}

		a,
		button {
			display: flex;
			align-items: center;
			gap: 0.5rem;
			height: 1.75rem;
			min-width: 0;
			/* Slight negative offset aligns with the parent left border */
			margin-left: -1px;
			padding: 0 0.5rem;
			border-radius: 0.375rem;
			text-decoration: none;
			font: inherit;
			font-size: 0.875rem;
			color: var(--sidebar-fg, hsl(240 5.3% 26.1%));
			background: transparent;
			border: none;
			cursor: pointer;
			width: 100%;
			text-align: left;
			overflow: hidden;
			white-space: nowrap;
			outline: none;
			transition: background 150ms, color 150ms;
		}

		a:hover,
		button:hover {
			background: var(--sidebar-accent, hsl(240 4.8% 95.9%));
			color: var(--sidebar-accent-fg, hsl(240 5.9% 10%));
		}

		a:focus-visible,
		button:focus-visible {
			outline: 2px solid var(--sidebar-ring, hsl(217.2 91.2% 59.8%));
		}

		:host([active]) a,
		:host([active]) button {
			background: var(--sidebar-accent, hsl(240 4.8% 95.9%));
			color: var(--sidebar-accent-fg, hsl(240 5.9% 10%));
		}

		.sm {
			font-size: 0.75rem;
		}

		::slotted(svg) {
			width: 1rem;
			height: 1rem;
			flex-shrink: 0;
			color: var(--sidebar-accent-fg, hsl(240 5.9% 10%));
		}
	`;

	render() {
		if (this.href) {
			return html`
				<a href=${this.href} class=${this.size} ?aria-current=${this.active}><slot></slot></a>
			`;
		}
		return html`
			<button type="button" class=${this.size} ?aria-pressed=${this.active}><slot></slot></button>
		`;
	}
}

customElements.define('app-sidebar-menu-sub-button', AppSidebarMenuSubButton);
