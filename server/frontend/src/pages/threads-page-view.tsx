import { Component, css, onMount, server } from "@frontiers-labs/argon";
import { AppApprovalBar } from "../components/app-approval-bar.js";
import { AppButton } from "../components/app-button.js";
import { AppChatView } from "../components/app-chat-view.js";
import { AppPromptInput } from "../components/app-prompt-input.js";
import { AppQuizBar } from "../components/app-quiz-bar.js";
import { AppSidebar, AppSidebarToggle, type SidebarProject, type SidebarThread } from "../components/app-sidebar.js";
import { type ChatTurn } from "../shared/timeline.js";
import { mountThreadsPage } from "../components/threads-page-controller.js";
import { threadView } from "../stores/thread-view.js";

interface ThreadPageData {
  threadId: string;
  currentTitle: string;
  selectedModel: string;
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
  .files-button { min-width: 72px; }
  .thread-menu-button { margin-left: 4px; }
  .content { flex: 1; min-height: 0; width: 100%; }
  app-chat-view { height: 100%; }
  .error { color: var(--destructive); font-size: 13px; margin: 10px auto 0; max-width: 860px; }
  .error:empty { display: none; }
  @media (max-width: 767px) {
    header app-sidebar-toggle { display: inline-flex; }
    header { justify-content: space-between; }
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

  onMount(() => mountThreadsPage(this));

  return (
    <>
      <style>{styles}</style>
      <div class="page">
        <nav><AppSidebar projects={data.projects} threads={data.threads} /></nav>
        <main>
          <header>
            <AppSidebarToggle />
            <span class="toolbar-spacer"></span>
            <AppButton variant="ghost" size="sm" class="files-button" data-action="files">Files</AppButton>
            <AppButton variant="ghost" size="icon-sm" class="thread-menu-button" title="Thread actions" aria-label="Thread actions" data-action="thread-menu">⋯</AppButton>
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
            models={threadView.models}
            selectedModel={selectedModel}
          />
          <AppApprovalBar style="margin: auto" data-approval hidden={threadView.approvalMessage === ""} message={threadView.approvalMessage} />
          <AppQuizBar style="margin: auto" data-quiz hidden={threadView.quizQuestion === ""} question={threadView.quizQuestion} options={threadView.quizOptions} />
          <div class="error" data-error>{threadView.error}</div>
        </main>
        <app-file-manager data-file-manager data-thread-id={data.threadId}></app-file-manager>
      </div>
    </>
  );
}
