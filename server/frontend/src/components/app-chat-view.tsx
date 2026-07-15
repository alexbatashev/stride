import { Component, css } from "@frontiers-labs/argon";
import { ChatTurn } from "../shared/timeline.js";
import { AppMessageScroller } from "./app-message-scroller.js";
import { AppMessage } from "./app-message.js";
import { AppWorkGroup } from "./app-work-group.js";

const styles = css`
  :host { display: block; height: 100%; min-height: 0; width: 100%; }
  app-message-scroller { height: 100%; }
  .transcript { box-sizing: border-box; margin: 0 auto; max-width: 768px; min-height: 100%; padding: 12px 16px 120px; width: 100%; }
  .turn { min-width: 0; }
  .turn + .turn { margin-top: 4px; }
  .user-row { padding-bottom: 16px; }
  .work-row { padding-bottom: 16px; }
  .answer-row { padding-bottom: 16px; }
  .empty { align-content: center; display: grid; flex: 1; justify-items: center; min-height: 100%; padding-bottom: 96px; text-align: center; }
  .empty h2 { color: var(--foreground); font-size: clamp(28px, 4vw, 40px); font-weight: 700; letter-spacing: -0.03em; line-height: 1.08; margin: 0 0 12px; }
  .empty p { color: var(--muted-foreground); font-size: 0.9375rem; line-height: 1.5; margin: 0; max-width: 420px; }
  @media (max-width: 767px) { .transcript { padding: 10px 12px 112px; } }
`;

export function AppChatView({ turns = [], emptyTitle = "What are we working on?", emptyDescription = "Start a thread and S.T.R.I.D.E. will keep the context here." }: { turns?: ChatTurn[]; emptyTitle?: string; emptyDescription?: string }): Component {
  return <><style>{styles}</style><AppMessageScroller><div class="transcript" data-messages>{turns.length === 0 ? <div class="empty" data-empty><h2>{emptyTitle}</h2><p>{emptyDescription}</p></div> : turns.map((turn) => <section class="turn" key={turn.id}>{turn.hasUser && <div class="user-row"><AppMessage messageId={turn.user.id} seq={turn.user.seq} role={turn.user.role} kind={turn.user.kind} format={turn.user.format} text={turn.user.text} pending={turn.user.pending} /></div>}{turn.hasWork && <div class="work-row"><AppWorkGroup label={turn.workLabel} segments={turn.segments} running={turn.running} startedAt={turn.startedAt} /></div>}{turn.hasAnswer && <div class="answer-row"><AppMessage messageId={turn.answer.id} seq={turn.answer.seq} role={turn.answer.role} kind={turn.answer.kind} format={turn.answer.format} text={turn.answer.text} pending={turn.answer.pending} /></div>}</section>).join("")}</div></AppMessageScroller></>;
}
