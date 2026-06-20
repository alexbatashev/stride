/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css } from "@frontiers-labs/argon";

interface Crumb {
  label: string;
  href?: string;
}

const styles = css`
  :host {
    display: block;
  }

  nav {
    align-items: center;
    color: var(--muted-foreground, #71717a);
    display: flex;
    flex-wrap: wrap;
    font-size: 0.875rem;
    gap: 8px;
  }

  a {
    color: inherit;
    text-decoration: none;
    transition: color 140ms ease;
  }

  a:hover {
    color: var(--foreground, #18181b);
  }

  .current {
    color: var(--foreground, #18181b);
    font-weight: 500;
  }

  .sep {
    color: var(--muted-foreground, #71717a);
    user-select: none;
  }
`;

export function AppBreadcrumb({ items = [] }: { items?: Crumb[] }): Component {
  return (
    <>
      <style>{styles}</style>
      <nav aria-label="Breadcrumb">
        {items
          .map((item, index) => {
            const isLast = index === items.length - 1;
            const node =
              item.href && !isLast
                ? `<a href="${item.href}">${item.label}</a>`
                : `<span class="current" aria-current="page">${item.label}</span>`;
            const sep = isLast ? "" : '<span class="sep" aria-hidden="true">/</span>';
            return node + sep;
          })
          .join("")}
      </nav>
    </>
  );
}
