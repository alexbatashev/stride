import {logout} from "../api/auth.js";
import {createProject} from "../api/projects.js";
import "../components/app-file-browser.js";

const root = document.querySelector<HTMLElement>("#files-page");

root
	?.querySelector<HTMLElement>('[data-action="logout"]')
	?.addEventListener("click", () => {
		void logout().then(() => {
			window.location.href = "/auth/login";
		});
	});

root
	?.querySelector<HTMLElement>('[data-action="new-project"]')
	?.addEventListener("click", () => {
		const title = window.prompt("Project name:")?.trim();
		if (!title) return;
		void createProject(title).then(() => {
			window.location.href = "/threads";
		});
	});
