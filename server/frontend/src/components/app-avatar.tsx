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
    font-size: 0.875rem;
    font-weight: 500;
    height: 32px;
    justify-content: center;
    overflow: hidden;
    user-select: none;
    width: 32px;
  }

  :host([size="sm"]) .avatar { font-size: 0.75rem; height: 24px; width: 24px; }
  :host([size="lg"]) .avatar { height: 40px; width: 40px; }

  ::slotted([slot="badge"]) { bottom: 0; position: absolute; right: 0; z-index: 1; }

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
      <span class="avatar" style="position:relative">
        <img src={src} alt={alt} style={src !== "" ? "" : "display:none"} />
        <span class="fallback" style={src !== "" ? "display:none" : ""}>{fallback}</span>
        <slot name="badge"></slot>
      </span>
    </>
  );
}

const groupStyles = css`
  :host { align-items: center; display: flex; }
  ::slotted(app-avatar) { margin-left: -8px; outline: 2px solid var(--background); }
  ::slotted(app-avatar:first-child) { margin-left: 0; }
`;

export function AppAvatarGroup(): Component { return <><style>{groupStyles}</style><slot></slot></>; }

const countStyles = css`
  :host { align-items: center; background: var(--muted); border: 2px solid var(--background); border-radius: 999px; color: var(--muted-foreground); display: inline-flex; font-size: 0.875rem; height: 32px; justify-content: center; width: 32px; }
`;

export function AppAvatarGroupCount(): Component { return <><style>{countStyles}</style><slot></slot></>; }
