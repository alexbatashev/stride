/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css } from "@frontiers-labs/argon";
import { IconChevronLeft } from "./icons/chevron-left.js";
import { IconChevronRight } from "./icons/chevron-right.js";

const styles = css`
  :host {
    display: block;
  }

  nav {
    align-items: center;
    display: flex;
    gap: 4px;
    justify-content: center;
  }

  button {
    align-items: center;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 8px;
    color: var(--foreground, #18181b);
    cursor: pointer;
    display: inline-flex;
    font: inherit;
    font-size: 0.875rem;
    gap: 4px;
    height: 32px;
    justify-content: center;
    min-width: 32px;
    outline: none;
    padding: 0 8px;
    transition:
      background-color 140ms ease,
      border-color 140ms ease,
      box-shadow 140ms ease;
  }

  button:hover:not(:disabled) {
    background: var(--muted, #f4f4f5);
  }

  button:focus-visible {
    border-color: var(--ring, #18181b);
    box-shadow: 0 0 0 3px var(--ring-shadow, rgb(24 24 27 / 12%));
  }

  button[aria-current="page"] {
    border-color: var(--border, #e4e4e7);
    font-weight: 600;
  }

  button:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }

  .icon {
    height: 16px;
    width: 16px;
  }

  .ellipsis {
    align-items: center;
    color: var(--muted-foreground, #71717a);
    display: inline-flex;
    justify-content: center;
    min-width: 32px;
  }
`;

function pageList(total: number, current: number): (number | "...")[] {
  if (total <= 7) return Array.from({ length: total }, (_, index) => index + 1);
  const pages: (number | "...")[] = [1];
  const start = Math.max(2, current - 1);
  const end = Math.min(total - 1, current + 1);
  if (start > 2) pages.push("...");
  for (let page = start; page <= end; page += 1) pages.push(page);
  if (end < total - 1) pages.push("...");
  pages.push(total);
  return pages;
}

function emitPageChange(host: HTMLElement, totalAttr: string, pageAttr: string, request: string): void {
  const totalPages = Math.max(1, Number(totalAttr) || 1);
  const current = Math.min(totalPages, Math.max(1, Number(pageAttr) || 1));
  const target = request === "prev" ? current - 1 : request === "next" ? current + 1 : Number(request);
  if (target < 1 || target > totalPages || target === current) return;
  host.setAttribute("page", String(target));
  host.dispatchEvent(
    new CustomEvent("page-change", { bubbles: true, composed: true, detail: { page: target } }),
  );
}

export function AppPagination({ total = "1", page = "1" }: { total?: string; page?: string }): Component {
  const totalPages = Math.max(1, Number(total) || 1);
  const current = Math.min(totalPages, Math.max(1, Number(page) || 1));
  return (
    <>
      <style>{styles}</style>
      <nav
        aria-label="Pagination"
        onClick={(event: Event) => {
          const target = (event.target as Element).closest("[data-page]");
          if (!target) return;
          emitPageChange(this, total, page, target.getAttribute("data-page") ?? "");
        }}
      >
        <button
          type="button"
          class="prev"
          aria-label="Previous page"
          disabled={current === 1}
          onClick={() => emitPageChange(this, total, page, "prev")}
        >
          <span class="icon">
            <IconChevronLeft />
          </span>
        </button>
        {pageList(totalPages, current)
          .map((entry) =>
            entry === "..."
              ? '<span class="ellipsis">&hellip;</span>'
              : `<button type="button" data-page="${entry}"${
                  entry === current ? ' aria-current="page"' : ""
                }>${entry}</button>`,
          )
          .join("")}
        <button
          type="button"
          class="next"
          aria-label="Next page"
          disabled={current === totalPages}
          onClick={() => emitPageChange(this, total, page, "next")}
        >
          <span class="icon">
            <IconChevronRight />
          </span>
        </button>
      </nav>
    </>
  );
}
