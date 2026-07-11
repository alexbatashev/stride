/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, emit, onMount, ref, state } from "@frontiers-labs/argon";
import { IconChevronDown } from "./icons/chevron-down.js";
import { AppButton } from "./app-button.js";

const styles = css`
  :host { display: block; height: 100%; min-height: 0; width: 100%; }
  .root { display: flex; height: 100%; min-height: 0; overflow: hidden; position: relative; width: 100%; }
  .viewport { contain: content; height: 100%; min-height: 0; min-width: 0; overscroll-behavior: contain; overflow-y: auto; scrollbar-gutter: stable; width: 100%; }
  .content { display: flex; flex-direction: column; gap: 32px; min-height: 100%; }
  .to-end { bottom: 16px; left: 50%; opacity: 1; position: absolute; transform: translate(-50%, 0); transition: opacity 200ms ease, transform 200ms ease; }
  .to-end.hidden { opacity: 0; pointer-events: none; transform: translate(-50%, 100%); }
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
  return <><style>{styles}</style><div class="root"><div ref={viewport} class="viewport"><div class="content"><slot></slot></div></div><AppButton class={`to-end ${atEnd ? "hidden" : ""}`} size="icon-sm" variant="outline" onClick={() => viewport.current?.scrollTo({ top: viewport.current.scrollHeight, behavior: "smooth" })}><IconChevronDown /><span class="sr-only">Scroll to end</span></AppButton></div></>;
}
