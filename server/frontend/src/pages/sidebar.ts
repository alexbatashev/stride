import { logout } from "../api/auth.js";
import { createProject, deleteProject, renameProject } from "../api/projects.js";
import { openThreadMenu, type ThreadMutation, type ThreadRef } from "./thread-actions.js";

// After a sidebar-triggered mutation, reload so the list re-renders. If the
// affected thread is the one currently open, an archive/delete makes its page
// invalid, so navigate to the new-thread view instead.
function reloadAfterThreadMutation(mutation: ThreadMutation, thread: ThreadRef): void {
	const currentId = document.querySelector<HTMLElement>("#threads-page")?.dataset.threadId;
	if (currentId && currentId === thread.id && (mutation === "delete" || mutation === "archive")) {
		window.location.href = "/threads";
		return;
	}
	window.location.reload();
}

// Sidebar navigation is plain <a href> links, so every page gets it for free.
// Only the actions that need JS (auth + project mutations) are wired here, once,
// so a new page can never silently drop them.
export function bindSidebar(sidebar: HTMLElement): void {
	sidebar.addEventListener("logout", () => {
		void logout().then(() => {
			window.location.href = "/auth/login";
		});
	});

	sidebar.addEventListener("new-project", () => {
		const title = window.prompt("Project name:")?.trim();
		if (!title) return;
		void createProject(title).then(() => window.location.reload());
	});

	sidebar.addEventListener("project-new-thread", (event) => {
		const { id } = (event as CustomEvent<{ id: string }>).detail;
		if (!id) return;
		window.location.href = `/threads?project=${encodeURIComponent(id)}`;
	});

	sidebar.addEventListener("project-rename", (event) => {
		const { id, title } = (event as CustomEvent<{ id: string; title: string }>).detail;
		const next = window.prompt("Project name:", title)?.trim();
		if (!next || next === title) return;
		void renameProject(id, next).then(() => window.location.reload());
	});

	sidebar.addEventListener("project-delete", (event) => {
		const { id } = (event as CustomEvent<{ id: string }>).detail;
		if (!window.confirm("Delete this project? Threads will be kept but unlinked.")) return;
		void deleteProject(id).then(() => window.location.reload());
	});

	sidebar.addEventListener("thread-menu", (event) => {
		const { id, title, anchor } = (event as CustomEvent<{ id: string; title: string; anchor: HTMLElement }>).detail;
		if (!id || !anchor) return;
		openThreadMenu(anchor, { id, title, archived: false }, reloadAfterThreadMutation);
	});
}
