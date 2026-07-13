import { Component, css, onMount, server } from "@frontiers-labs/argon";
import { AppApprovalBar } from "../components/app-approval-bar.js";
import { AppButton } from "../components/app-button.js";
import { AppChatView } from "../components/app-chat-view.js";
import { AppPromptInput } from "../components/app-prompt-input.js";
import { AppQuizBar } from "../components/app-quiz-bar.js";
import { AppDialog } from "../components/app-dialog.js";
import { IconPanelRight } from "../components/icons/panel-right.js";
import { AppSidebar, AppSidebarToggle, type SidebarProject, type SidebarThread } from "../components/app-sidebar.js";
import { AppSidebarProvider } from "../components/app-sidebar-primitives.js";
import { type ChatTurn } from "../shared/timeline.js";
import { type ModelOption } from "../shared/model-option.js";
import { mountThreadsPage } from "../components/threads-page-controller.js";
import { threadView } from "../stores/thread-view.js";
import { sidePanel } from "../stores/side-panel.js";

interface ThreadPageData {
  threadId: string;
  currentTitle: string;
  selectedModel: string;
  models: ModelOption[];
  selectedModelLabel: string;
  selectedModelReasoningEffort: string;
  running: boolean;
  projects: SidebarProject[];
  threads: SidebarThread[];
  turns: ChatTurn[];
}

declare function loadThreadPage(threadId: string): ThreadPageData;

const styles = css`
  :host {
    display: block;
    height: 100%;
    min-width: 0;
    width: 100%;
  }
  .page { display: flex; height: 100%; width: 100%; }
  nav { height: 100%; }
  main { align-items: stretch; display: flex; flex: 1; flex-direction: column; height: 100%; min-width: 0; }
  header { align-items: center; border-bottom: 1px solid var(--border); box-sizing: border-box; display: flex; height: 48px; padding: 8px; width: 100%; }
  header app-sidebar-toggle { display: none; }
  .toolbar-spacer { flex: 1; }
  .panel-button { color: var(--muted-foreground); }
  .panel-button[hidden] { display: none; }
  .thread-menu-button { margin-left: 4px; }
  .content { flex: 1; min-height: 0; width: 100%; }
  app-side-panel:not([open]) { display: none; }
  app-chat-view { height: 100%; }
  .error { color: var(--destructive); font-size: 13px; margin: 10px auto 0; max-width: 860px; }
  .error:empty { display: none; }
  @media (max-width: 767px) {
    header app-sidebar-toggle { display: inline-flex; }
    header { justify-content: space-between; }
    .panel-button { display: none; }
  }
  @media (min-width: 768px) {
    .thread-menu-button { display: none; }
  }
`;

export function ThreadsPageView({ threadId = "" }: { threadId?: string }): Component {
  const data = server(loadThreadPage(threadId));
  const turns = threadView.active ? threadView.turns : data.turns;
  const running = threadView.active ? threadView.running : data.running;
  const placeholder = threadView.active
    ? threadView.placeholder
    : data.threadId === "" ? "Ask S.T.R.I.D.E. anything" : "Message S.T.R.I.D.E.";
  const selectedModel = threadView.active ? threadView.selectedModel : data.selectedModel;
  const models = threadView.active ? threadView.models : data.models;
  const selectedModelLabel = threadView.active ? threadView.selectedModelLabel : data.selectedModelLabel;
  const selectedModelReasoningEffort = threadView.active ? threadView.selectedModelReasoningEffort : data.selectedModelReasoningEffort;

  onMount(() => mountThreadsPage(this));

  return (
    <>
      <style>{styles}</style>
      <AppSidebarProvider>
        <div class="page">
          <nav><AppSidebar projects={data.projects} threads={data.threads} /></nav>
          <main>
          <header>
            <AppSidebarToggle />
            <span class="toolbar-spacer"></span>
            <AppButton variant="ghost" size="icon-sm" class="panel-button" title="Open side panel" aria-label="Open side panel" data-action="side-panel-open"><IconPanelRight /></AppButton>
            <AppButton variant="ghost" size="icon-sm" class="thread-menu-button" title="Thread actions" aria-label="Thread actions" data-action="thread-menu" hidden>⋯</AppButton>
            <span data-current-title hidden>{data.currentTitle}</span>
          </header>
          <section class="content">
            <AppChatView turns={turns} />
          </section>
          <AppPromptInput
            style="margin: auto"
            data-prompt
            hidden={threadView.approvalMessage !== "" || threadView.quizQuestion !== ""}
            running={running}
            placeholder={placeholder}
            models={models}
            selectedModel={selectedModel}
            selectedModelLabel={selectedModelLabel}
            selectedModelReasoningEffort={selectedModelReasoningEffort}
          />
          <AppApprovalBar style="margin: auto" data-approval hidden={threadView.approvalMessage === ""} message={threadView.approvalMessage} />
          <AppQuizBar style="margin: auto" data-quiz hidden={threadView.quizQuestion === ""} question={threadView.quizQuestion} options={threadView.quizOptions} />
          <div class="error" data-error>{threadView.error}</div>
        </main>
        <app-side-panel open={sidePanel.open} tabs='[{"value":"files","label":"Files"},{"value":"subagents","label":"Subagents"}]' data-active-tab={sidePanel.tab} data-side-panel>
          <AppButton slot="header-action" variant="ghost" size="icon-sm" title="Close side panel" aria-label="Close side panel" data-action="side-panel-close"><IconPanelRight /></AppButton>
          <app-file-explorer slot="files" data-thread-id={data.threadId} data-pane-active={sidePanel.open && sidePanel.tab === "files"}></app-file-explorer>
          <app-subagent-view slot="subagents" data-thread-id={data.threadId} data-active={sidePanel.open && sidePanel.tab === "subagents"}></app-subagent-view>
        </app-side-panel>
        <AppDialog open={false} size="fullscreen" title="Files" data-mobile-panel>
          <app-file-explorer data-thread-id={data.threadId} data-pane-active={false} data-mobile-files></app-file-explorer>
          <app-subagent-view data-thread-id={data.threadId} data-active={false} data-mobile-subagents></app-subagent-view>
          </AppDialog>
        </div>
      </AppSidebarProvider>
    </>
  );
}
