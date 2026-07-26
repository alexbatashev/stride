/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, effect, ref } from "@frontiers-labs/argon";

const styles = css`
  :host {
    display: block;
  }

  .frame {
    overflow: hidden;
    position: relative;
    width: 100%;
  }

  ::slotted(*) {
    height: 100%;
    inset: 0;
    object-fit: cover;
    position: absolute;
    width: 100%;
  }
`;

export function AppAspectRatio({ ratio = "1" }: { ratio?: string }): Component {
  const frame = ref<HTMLDivElement>();
  effect(() => {
    const el = frame.current;
    if (!el) return;
    const parsed = Number(ratio);
    el.style.aspectRatio = Number.isFinite(parsed) && parsed > 0 ? String(parsed) : "1";
  });
  return (
    <>
      <style>{styles}</style>
      <div class="frame" ref={frame} style={`aspect-ratio:${ratio}`}>
        <slot></slot>
      </div>
    </>
  );
}
