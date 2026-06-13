import { bindSidebar } from "./sidebar.js";

const sidebar = document.querySelector<HTMLElement>("#files-page app-sidebar");
if (sidebar) {
	bindSidebar(sidebar);
}
