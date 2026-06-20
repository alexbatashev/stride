/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, effect, ref } from "@frontiers-labs/argon";

const styles = css`
  :host {
    display: block;
  }

  .track {
    background: var(--muted, #f4f4f5);
    border-radius: 999px;
    height: 8px;
    overflow: hidden;
    width: 100%;
  }

  .indicator {
    background: var(--primary, #18181b);
    border-radius: inherit;
    height: 100%;
    transition: width 220ms ease;
    width: 0;
  }
`;

function clampPercent(value: string): number {
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) return 0;
  return Math.max(0, Math.min(100, parsed));
}

export function AppProgress({ value = "0" }: { value?: string }): Component {
  const indicator = ref<HTMLDivElement>();
  const percent = clampPercent(value);
  effect(() => {
    const el = indicator.current;
    if (el) el.style.width = `${clampPercent(value)}%`;
  });
  return (
    <>
      <style>{styles}</style>
      <div
        class="track"
        role="progressbar"
        aria-valuemin="0"
        aria-valuemax="100"
        aria-valuenow={String(percent)}
      >
        <div class="indicator" ref={indicator}></div>
      </div>
    </>
  );
}
