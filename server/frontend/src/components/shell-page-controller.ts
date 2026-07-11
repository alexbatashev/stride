import { bindSidebar } from "../pages/sidebar.js";
import { openThreadMenu } from "../pages/thread-actions.js";

export function mountShellPage(root: HTMLElement, page: string): void {
  const sidebar = root.shadowRoot?.querySelector<HTMLElement>("app-sidebar");
  if (sidebar) bindSidebar(sidebar);
  if (page !== "archived") return;

  const list = root.shadowRoot?.querySelector<HTMLElement>("app-archived-threads");
  list?.addEventListener("thread-menu", (event) => {
    const { id, title, anchor } = (event as CustomEvent<{ id: string; title: string; anchor: HTMLElement }>).detail;
    if (!id || !anchor) return;
    openThreadMenu(anchor, { id, title, archived: true }, () => window.location.reload());
  });
}
