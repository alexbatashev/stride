import { bindSidebar } from "./sidebar.js";
import { openThreadMenu } from "./thread-actions.js";

const root = document.querySelector<HTMLElement>("#archived-page");
const sidebar = root?.querySelector<HTMLElement>("app-sidebar");
if (sidebar) {
	bindSidebar(sidebar);
}

const list = root?.querySelector<HTMLElement>("app-archived-threads");
list?.addEventListener("thread-menu", (event) => {
	const { id, title, anchor } = (event as CustomEvent<{ id: string; title: string; anchor: HTMLElement }>).detail;
	if (!id || !anchor) return;
	openThreadMenu(anchor, { id, title, archived: true }, () => window.location.reload());
});
