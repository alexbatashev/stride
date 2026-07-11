import { Component, css, onMount, server } from "@frontiers-labs/argon";
import { AppApprovalBar } from "../components/app-approval-bar.js";
import { AppButton } from "../components/app-button.js";
import { AppMessage } from "../components/app-message.js";
import { AppPromptInput } from "../components/app-prompt-input.js";
import { AppQuizBar } from "../components/app-quiz-bar.js";
import { AppSidebar, AppSidebarToggle, type SidebarProject, type SidebarThread } from "../components/app-sidebar.js";
import { buildTimeline, type TimelineMessage } from "../shared/timeline.js";
import { mountThreadsPage } from "../components/threads-page-controller.js";
import { threadView } from "../stores/thread-view.js";

interface ThreadPageData {
  threadId: string;
  currentTitle: string;
  selectedModel: string;
  running: boolean;
  projects: SidebarProject[];
  threads: SidebarThread[];
  messages: TimelineMessage[];
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
  .content { display: flex; flex: 1; justify-content: center; overflow: auto; padding: 24px 16px; width: 100%; }
  .wrapper { display: flex; flex-direction: column; max-width: 960px; width: 100%; }
  .empty { align-content: center; display: grid; justify-items: center; min-height: 100%; padding-bottom: 96px; text-align: center; }
  .empty h2 { color: var(--foreground); font-size: clamp(28px, 4vw, 40px); font-weight: 700; line-height: 1.08; margin: 0 0 12px; }
  .empty p { color: var(--muted-foreground); font-size: 15px; line-height: 1.5; margin: 0; max-width: 420px; }
  .error { color: var(--destructive); font-size: 13px; margin: 10px auto 0; max-width: 860px; }
  .error:empty { display: none; }
  @media (max-width: 767px) {
    header app-sidebar-toggle { display: inline-flex; }
    header { justify-content: space-between; }
  }
`;

export function ThreadsPageView({ threadId = "" }: { threadId?: string }): Component {
  const data = server(loadThreadPage(threadId));
  const timeline = buildTimeline(threadView.active ? threadView.messages : data.messages);
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
            <div class="wrapper" data-messages>
              {timeline.length === 0 ? (
                <div class="empty" data-empty>
                  <h2>What are we working on?</h2>
                  <p>Start a thread and S.T.R.I.D.E. will keep the context here.</p>
                </div>
              ) : timeline.map((item) => (
                <AppMessage
                  key={item.id}
                  messageId={item.id}
                  seq={item.seq}
                  role={item.role}
                  kind={item.kind}
                  format={item.format}
                  text={item.text}
                  thinking={item.thinking}
                  toolName={item.toolName}
                />
              )).join("")}
            </div>
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
