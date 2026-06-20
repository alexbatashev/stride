/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, onMount } from "@frontiers-labs/argon";

const styles = css`
  :host {
    display: inline-flex;
  }

  .avatar {
    align-items: center;
    background: var(--muted, #f4f4f5);
    border-radius: 999px;
    color: var(--muted-foreground, #71717a);
    display: inline-flex;
    flex: 0 0 auto;
    font-size: 0.85rem;
    font-weight: 500;
    height: 40px;
    justify-content: center;
    overflow: hidden;
    user-select: none;
    width: 40px;
  }

  img {
    height: 100%;
    object-fit: cover;
    width: 100%;
  }

  img:not([src]) {
    display: none;
  }
`;

export function AppAvatar({
  src = "",
  alt = "",
  fallback = "",
}: {
  src?: string;
  alt?: string;
  fallback?: string;
}): Component {
  onMount(() => {
    const root = this.shadowRoot;
    if (!root) return;
    const image = root.querySelector<HTMLImageElement>("img");
    const fallbackEl = root.querySelector<HTMLElement>(".fallback");
    if (!image || !fallbackEl) return;
    const showFallback = () => {
      image.style.display = "none";
      fallbackEl.style.display = "inline";
    };
    if (!src) {
      showFallback();
      return;
    }
    image.addEventListener("error", showFallback);
    return () => image.removeEventListener("error", showFallback);
  });
  return (
    <>
      <style>{styles}</style>
      <span class="avatar">
        <img src={src} alt={alt} />
        <span class="fallback" style="display:none">{fallback}</span>
      </span>
    </>
  );
}
