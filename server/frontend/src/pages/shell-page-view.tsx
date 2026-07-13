import { Component, css, onMount, server } from "@frontiers-labs/argon";
import { AppSidebar, AppSidebarToggle, type SidebarProject, type SidebarThread } from "../components/app-sidebar.js";
import { AppSidebarProvider } from "../components/app-sidebar-primitives.js";
import { AppSettingsDialog } from "../components/app-settings-dialog.js";
import { mountShellPage } from "../components/shell-page-controller.js";

interface ShellPageData {
  projects: SidebarProject[];
  threads: SidebarThread[];
}

declare function loadShellPage(page: string): ShellPageData;

const styles = css`
  :host { display: block; height: 100%; min-width: 0; width: 100%; }
  .page { display: flex; height: 100%; width: 100%; }
  nav { height: 100%; }
  main { display: flex; flex: 1; flex-direction: column; min-height: 0; min-width: 0; }
  app-file-browser, app-automations, app-archived-threads { flex: 1; min-height: 0; }
  .mobile-bar { display: none; }
  @media (max-width: 767px) {
    .mobile-bar { border-bottom: 1px solid var(--border); display: flex; padding: 8px 12px; }
  }
`;

export function ShellPageView({ page }: { page: string }): Component {
  const data = server(loadShellPage(page));

  onMount(() => mountShellPage(this, page));

  return (
    <>
      <style>{styles}</style>
      <AppSidebarProvider>
        <div class="page">
          <nav><AppSidebar projects={data.projects} threads={data.threads} /></nav>
          <main>
            <div class="mobile-bar"><AppSidebarToggle /></div>
            {page === "files" ? <app-file-browser></app-file-browser>
              : page === "automations" ? <app-automations></app-automations>
              : <app-archived-threads></app-archived-threads>}
          </main>
          <AppSettingsDialog />
        </div>
      </AppSidebarProvider>
    </>
  );
}
