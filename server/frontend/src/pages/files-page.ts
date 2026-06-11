import {logout} from "../api/auth.js";
import {createProject} from "../api/projects.js";

const sidebar = document.querySelector<HTMLElement>("#files-page app-sidebar");

sidebar?.addEventListener("logout", () => {
	void logout().then(() => {
		window.location.href = "/auth/login";
	});
});

sidebar?.addEventListener("new-project", () => {
	const title = window.prompt("Project name:")?.trim();
	if (!title) return;
	void createProject(title).then(() => {
		window.location.href = "/threads";
	});
});

sidebar?.addEventListener("new-thread", () => {
	window.location.href = "/threads";
});

sidebar?.addEventListener("thread-select", (event) => {
	const id = (event as CustomEvent<{id: string}>).detail.id;
	window.location.href = `/threads/${id}`;
});
