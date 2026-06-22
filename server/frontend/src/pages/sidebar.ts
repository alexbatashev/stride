import { logout } from "../api/auth.js";
import { createProject, deleteProject, renameProject } from "../api/projects.js";

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
}
