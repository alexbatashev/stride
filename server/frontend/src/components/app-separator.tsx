/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css } from "@frontiers-labs/argon";

const styles = css`
  :host {
    display: block;
  }

  .separator {
    background: var(--border, #e4e4e7);
    flex: 0 0 auto;
  }

  :host(:not([orientation="vertical"])) .separator {
    height: 1px;
    width: 100%;
  }

  :host([orientation="vertical"]) {
    display: inline-block;
    height: 100%;
  }

  :host([orientation="vertical"]) .separator {
    height: 100%;
    width: 1px;
  }
`;

export function AppSeparator({ orientation = "horizontal" }: { orientation?: string }): Component {
  return (
    <>
      <style>{styles}</style>
      <div class="separator" role="separator" aria-orientation={orientation}></div>
    </>
  );
}
