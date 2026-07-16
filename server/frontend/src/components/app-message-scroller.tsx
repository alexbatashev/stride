/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, emit, onMount, ref, state } from "@frontiers-labs/argon";
import { IconArrowDown } from "./icons/arrow-down.js";

const styles = css`
  :host { display: block; height: 100%; min-height: 0; width: 100%; }
  .root { display: flex; height: 100%; min-height: 0; overflow: hidden; position: relative; width: 100%; }
  .viewport { contain: content; height: 100%; min-height: 0; min-width: 0; overscroll-behavior: contain; overflow-y: auto; scrollbar-gutter: stable; width: 100%; }
  .content { display: flex; flex-direction: column; gap: 32px; min-height: 100%; }
  .to-end {
    align-items: center;
    background: var(--background, #fff);
    border: 1px solid var(--border, #e4e4e7);
    border-radius: 16px;
    bottom: var(--composer-clearance, 160px);
    box-sizing: border-box;
    color: var(--foreground, #18181b);
    cursor: pointer;
    display: flex;
    height: 28px;
    justify-content: center;
    left: 50%;
    opacity: 1;
    padding: 0;
    position: absolute;
    transform: translate(-50%, 0) scale(1);
    transition: background-color 140ms ease, border-color 140ms ease, box-shadow 140ms ease, color 140ms ease, opacity 200ms ease, transform 200ms cubic-bezier(0.23, 1, 0.32, 1);
    width: 28px;
  }
  .to-end:hover { background: var(--muted, #f4f4f5); }
  .to-end:focus-visible { border-color: var(--ring, #18181b); box-shadow: 0 0 0 3px var(--ring-shadow, rgb(24 24 27 / 12%)); outline: none; }
  .to-end.hidden { opacity: 0; pointer-events: none; transform: translate(-50%, 100%) scale(0.95); transition-duration: 400ms; transition-timing-function: cubic-bezier(0.7, 0, 0.84, 0); }
  .to-end-icon { height: 16px; width: 16px; }
  .sr-only { height: 1px; margin: -1px; overflow: hidden; padding: 0; position: absolute; width: 1px; clip: rect(0, 0, 0, 0); white-space: nowrap; }
  @media (prefers-reduced-motion: reduce) { .to-end { transition: none; } }
`;

export function AppMessageScroller(): Component {
  const viewport = ref<HTMLDivElement>();
  let atEnd = state(true);
  onMount(() => {
    const el = viewport.current;
    if (!el) return;
    const update = () => { atEnd = el.scrollHeight - el.scrollTop - el.clientHeight < 8; emit(this, "scroll-state-change", { atEnd: atEnd }); };
    const observer = new ResizeObserver(() => { if (atEnd) el.scrollTo({ top: el.scrollHeight }); update(); });
    observer.observe(el);
    const content = el.querySelector(".content");
    if (content) observer.observe(content);
    el.addEventListener("scroll", update, { passive: true });
    (this as HTMLElement & { scrollToEnd: (behavior?: ScrollBehavior) => void }).scrollToEnd = (behavior = "smooth") => el.scrollTo({ top: el.scrollHeight, behavior: behavior });
    update();
    return () => { observer.disconnect(); el.removeEventListener("scroll", update); };
  });
  return <><style>{styles}</style><div class="root"><div ref={viewport} class="viewport"><div class="content"><slot></slot></div></div><button type="button" class={`to-end ${atEnd ? "hidden" : ""}`} tabIndex={atEnd ? -1 : 0} onClick={() => viewport.current?.scrollTo({ top: viewport.current.scrollHeight, behavior: "smooth" })}><IconArrowDown class="to-end-icon" /><span class="sr-only">Scroll to end</span></button></div></>;
}
