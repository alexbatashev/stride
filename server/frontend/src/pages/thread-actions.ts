import {
	archiveThread,
	deleteThread,
	renameThread,
	unarchiveThread,
} from "../api/threads.js";

// Shared client controller for the thread three-dot menu. It owns singleton
// menu/dialog elements reused across the sidebar, the thread view, and the
// archived-threads page, and turns a menu selection into an API call.

export type ThreadRef = { id: string; title: string; archived: boolean };
export type ThreadMutation = "rename" | "archive" | "unarchive" | "delete";
export type OnMutated = (mutation: ThreadMutation, thread: ThreadRef) => void;

type MenuItem = { label: string; action: string; variant?: string };
type MenuEl = HTMLElement & { open: boolean; items: MenuItem[] };
type DialogEl = HTMLElement & { open: boolean; title: string };
type AlertEl = HTMLElement & {
	open: boolean;
	title: string;
	description: string;
	actionLabel: string;
	variant: string;
};
type TextInputEl = HTMLElement & { value: string };

let menu: MenuEl | null = null;
let menuContext: { thread: ThreadRef; onMutated: OnMutated } | null = null;
let activeTrigger: HTMLElement | null = null;
let dismissClick: ((event: Event) => void) | null = null;
let dismissKey: ((event: KeyboardEvent) => void) | null = null;

function ensureMenu(): MenuEl {
	if (menu) return menu;
	const el = document.createElement("app-dropdown-menu") as MenuEl;
	document.body.appendChild(el);
	el.addEventListener("select", (event) => {
		const action = (event as CustomEvent<{ action: string }>).detail.action as ThreadMutation;
		const context = menuContext;
		closeMenu();
		if (context) runAction(action, context.thread, context.onMutated);
	});
	menu = el;
	return el;
}

function closeMenu(): void {
	if (menu) menu.open = false;
	if (dismissClick) document.removeEventListener("click", dismissClick, true);
	if (dismissKey) document.removeEventListener("keydown", dismissKey, true);
	dismissClick = null;
	dismissKey = null;
	activeTrigger?.removeAttribute("aria-expanded");
	activeTrigger = null;
}

export function openThreadMenu(trigger: HTMLElement, thread: ThreadRef, onMutated: OnMutated): void {
	const el = ensureMenu();
	menuContext = { thread, onMutated };
	activeTrigger = trigger;
	trigger.setAttribute("aria-expanded", "true");
	el.items = thread.archived
		? [
				{ label: "Unarchive", action: "unarchive" },
				{ label: "Delete", action: "delete", variant: "destructive" },
			]
		: [
				{ label: "Rename", action: "rename" },
				{ label: "Archive", action: "archive" },
				{ label: "Delete", action: "delete", variant: "destructive" },
			];
	el.open = true;
	requestAnimationFrame(() => positionMenu(el, trigger));

	// Bind dismissal on the next frame so the click that opened the menu is not
	// the click that closes it.
	requestAnimationFrame(() => {
		dismissClick = (event: Event) => {
			if (!event.composedPath().includes(el)) closeMenu();
		};
		dismissKey = (event: KeyboardEvent) => {
			if (event.key === "Escape") closeMenu();
		};
		document.addEventListener("click", dismissClick, true);
		document.addEventListener("keydown", dismissKey, true);
	});
}

function positionMenu(el: HTMLElement, trigger: HTMLElement): void {
	const rect = trigger.getBoundingClientRect();
	const menuRect = el.getBoundingClientRect();
	const width = menuRect.width || 176;
	const height = menuRect.height || 120;
	const margin = 8;

	let left = rect.right - width;
	left = Math.max(margin, Math.min(left, window.innerWidth - width - margin));

	let top = rect.bottom + 4;
	if (top + height > window.innerHeight - margin) {
		top = Math.max(margin, rect.top - height - 4);
	}

	el.style.left = `${left}px`;
	el.style.top = `${top}px`;
}

function runAction(action: ThreadMutation, thread: ThreadRef, onMutated: OnMutated): void {
	switch (action) {
		case "rename":
			openRenameDialog(thread, onMutated);
			return;
		case "archive":
			void mutate(() => archiveThread(thread.id), "archive", thread, onMutated);
			return;
		case "unarchive":
			void mutate(() => unarchiveThread(thread.id), "unarchive", thread, onMutated);
			return;
		case "delete":
			openDeleteDialog(thread, onMutated);
			return;
	}
}

async function mutate(
	call: () => Promise<void>,
	mutation: ThreadMutation,
	thread: ThreadRef,
	onMutated: OnMutated,
): Promise<void> {
	try {
		await call();
		onMutated(mutation, thread);
	} catch {
		window.alert("Something went wrong. Please try again.");
	}
}

// --- Rename dialog ---------------------------------------------------------

let renameDialog: { dialog: DialogEl; input: TextInputEl } | null = null;
let renameContext: { thread: ThreadRef; onMutated: OnMutated } | null = null;

function ensureRenameDialog(): { dialog: DialogEl; input: TextInputEl } {
	if (renameDialog) return renameDialog;

	const dialog = document.createElement("app-dialog") as DialogEl;
	dialog.title = "Rename thread";

	const input = document.createElement("app-text-input") as TextInputEl;
	input.setAttribute("label", "Title");

	const footer = document.createElement("div");
	footer.setAttribute("slot", "footer");
	const cancel = document.createElement("app-button");
	cancel.setAttribute("variant", "outline");
	cancel.textContent = "Cancel";
	const save = document.createElement("app-button");
	save.textContent = "Save";
	footer.append(cancel, save);

	dialog.append(input, footer);
	document.body.appendChild(dialog);

	dialog.addEventListener("close", () => {
		dialog.open = false;
	});
	cancel.addEventListener("click", () => {
		dialog.open = false;
	});
	save.addEventListener("click", () => void submitRename());
	input.addEventListener("commit", () => void submitRename());

	renameDialog = { dialog, input };
	return renameDialog;
}

function openRenameDialog(thread: ThreadRef, onMutated: OnMutated): void {
	const { dialog, input } = ensureRenameDialog();
	renameContext = { thread, onMutated };
	input.value = thread.title;
	dialog.open = true;
	requestAnimationFrame(() => {
		input.shadowRoot?.querySelector("input")?.focus();
	});
}

async function submitRename(): Promise<void> {
	if (!renameContext || !renameDialog) return;
	const { thread, onMutated } = renameContext;
	const title = renameDialog.input.value.trim();
	if (!title || title === thread.title) {
		renameDialog.dialog.open = false;
		return;
	}
	renameDialog.dialog.open = false;
	await mutate(() => renameThread(thread.id, title), "rename", { ...thread, title }, onMutated);
}

// --- Delete confirmation ---------------------------------------------------

let deleteDialog: AlertEl | null = null;
let deleteContext: { thread: ThreadRef; onMutated: OnMutated } | null = null;

function ensureDeleteDialog(): AlertEl {
	if (deleteDialog) return deleteDialog;

	const dialog = document.createElement("app-alert-dialog") as AlertEl;
	dialog.setAttribute("variant", "destructive");
	dialog.actionLabel = "Delete";
	document.body.appendChild(dialog);

	dialog.addEventListener("response", (event) => {
		const confirmed = (event as CustomEvent<{ confirmed: boolean }>).detail.confirmed;
		dialog.open = false;
		if (confirmed && deleteContext) {
			const { thread, onMutated } = deleteContext;
			void mutate(() => deleteThread(thread.id), "delete", thread, onMutated);
		}
	});

	deleteDialog = dialog;
	return dialog;
}

function openDeleteDialog(thread: ThreadRef, onMutated: OnMutated): void {
	const dialog = ensureDeleteDialog();
	deleteContext = { thread, onMutated };
	dialog.title = `Delete "${thread.title}"?`;
	dialog.description =
		"This permanently removes the thread, its messages, and its workspace files and history. This cannot be undone.";
	dialog.open = true;
}
