import { bindSidebar } from "./sidebar.js";

const sidebar = document.querySelector<HTMLElement>("#settings-page app-sidebar");
if (sidebar) {
	bindSidebar(sidebar);
}
