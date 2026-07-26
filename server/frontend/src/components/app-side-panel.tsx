/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, effect, emit, onMount, state } from "@frontiers-labs/argon";
import { AppTabs } from "./app-tabs.js";

interface Tab {
	value: string;
	label: string;
}

const MIN_WIDTH = 320;
const DEFAULT_WIDTH = 400;
const STORAGE_KEY = "stride.sidePanel.width";
const KEYBOARD_STEP = 16;

function clampWidth(width: number): number {
	const max = Math.max(MIN_WIDTH, Math.round(window.innerWidth * 0.6));
	return Math.min(max, Math.max(MIN_WIDTH, width));
}

function storedWidth(): number {
	const raw = Number(localStorage.getItem(STORAGE_KEY));
	return clampWidth(Number.isFinite(raw) && raw > 0 ? raw : DEFAULT_WIDTH);
}

const styles = css`
	:host {
		display: flex;
		flex: 0 0 auto;
		height: 100%;
		min-height: 0;
	}

	:host(:not([open])) {
		display: none;
	}

	@media (max-width: 767px) {
		:host {
			display: none !important;
		}
	}

	.resizer {
		align-self: stretch;
		background: transparent;
		border: 0;
		cursor: col-resize;
		flex: 0 0 auto;
		padding: 0;
		position: relative;
		touch-action: none;
		width: 8px;
	}

	.resizer::after {
		background: var(--border, #e4e4e7);
		content: "";
		inset-block: 0;
		left: 50%;
		position: absolute;
		transform: translateX(-50%);
		transition: background-color 140ms ease;
		width: 1px;
	}

	.resizer:hover::after,
	.resizer:focus-visible::after {
		background: var(--primary, #18181b);
		width: 2px;
	}

	.resizer:focus-visible {
		outline: none;
	}

	.panel {
		background: var(--background, #ffffff);
		box-sizing: border-box;
		color: var(--foreground, #09090b);
		display: flex;
		flex: 1;
		flex-direction: column;
		min-height: 0;
		min-width: 0;
	}

	.header {
		align-items: center;
		border-bottom: 1px solid var(--border, #e4e4e7);
		display: flex;
		gap: 8px;
		justify-content: flex-start;
		height: 48px;
		padding: 0 12px;
	}

	.header-action {
		align-items: center;
		display: flex;
		margin-left: auto;
	}

	.body {
		flex: 1;
		min-height: 0;
		min-width: 0;
		overflow: auto;
	}
`;

export function AppSidePanel({
	open = false,
	tabs = [],
	activeTab = "",
	title = "",
}: {
	open?: boolean;
	tabs?: Tab[];
	activeTab?: string;
	title?: string;
}): Component {
	let width = state(DEFAULT_WIDTH);
	let selectedTab = state(activeTab || tabs[0]?.value || "");

	onMount(() => {
		width = storedWidth();
		this.style.width = `${width}px`;
		const onResize = () => {
			width = clampWidth(width);
			this.style.width = `${width}px`;
		};
		window.addEventListener("resize", onResize);

		const onSelectTab = (event: Event) => {
			selectedTab = (event as CustomEvent<{ value: string }>).detail.value;
		};
		const onNestedTabChange = (event: Event) => {
			if (event.target === this) return;
			event.stopPropagation();
			selectedTab = (event as CustomEvent<{ value: string }>).detail.value;
			emit(this, "tab-change", { value: selectedTab });
		};
		this.addEventListener("select-tab", onSelectTab);
		this.addEventListener("tab-change", onNestedTabChange);

		return () => {
			window.removeEventListener("resize", onResize);
			this.removeEventListener("select-tab", onSelectTab);
			this.removeEventListener("tab-change", onNestedTabChange);
		};
	});

	effect(() => {
		this.toggleAttribute("open", open);
	});

	const startDrag = (event: PointerEvent) => {
		event.preventDefault();
		const handle = event.currentTarget as HTMLElement;
		handle.setPointerCapture(event.pointerId);
		const startX = event.clientX;
		const startWidth = width;
		document.body.style.userSelect = "none";
		const move = (moveEvent: PointerEvent) => {
			width = clampWidth(startWidth + (startX - moveEvent.clientX));
			this.style.width = `${width}px`;
		};
		const finish = () => {
			handle.releasePointerCapture(event.pointerId);
			handle.removeEventListener("pointermove", move);
			handle.removeEventListener("pointerup", finish);
			handle.removeEventListener("pointercancel", finish);
			document.body.style.userSelect = "";
			localStorage.setItem(STORAGE_KEY, String(width));
		};
		handle.addEventListener("pointermove", move);
		handle.addEventListener("pointerup", finish);
		handle.addEventListener("pointercancel", finish);
	};

	const onResizerKey = (event: KeyboardEvent) => {
		if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
		event.preventDefault();
		width = clampWidth(width + (event.key === "ArrowLeft" ? KEYBOARD_STEP : -KEYBOARD_STEP));
		this.style.width = `${width}px`;
		localStorage.setItem(STORAGE_KEY, String(width));
	};

	return (
		<>
			<style>{styles}</style>
			<div
				class="resizer"
				role="separator"
				aria-orientation="vertical"
				aria-label="Resize panel"
				tabindex="0"
				onPointerDown={startDrag}
				onKeyDown={onResizerKey}
			></div>
			<div class="panel">
				<div class="header">
					<div style="min-width:0;height:100%;display:flex;align-items:center">
						<AppTabs tabs={tabs} value={selectedTab} variant="line"></AppTabs>
					</div>
					<div class="header-action"><slot name="header-action"></slot></div>
				</div>
				<div class="body">
					{tabs
						.map((tab) => (
							<slot
								name={tab.value}
								style={selectedTab === tab.value ? "" : "display:none"}
							></slot>
						))
						.join("")}
				</div>
			</div>
		</>
	);
}
