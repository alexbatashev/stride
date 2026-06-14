import { bindSidebar } from "./sidebar.js";

const sidebar = document.querySelector<HTMLElement>("#automations-page app-sidebar");
if (sidebar) {
	bindSidebar(sidebar);
}
