import { LitElement, css, html } from "lit";
import { customElement, property } from "lit/decorators.js";
import "./app-spoiler.js";
import "./auto-markdown.js";
import { repeat } from "lit/directives/repeat.js";

type MessageType = "agent" | "user" | "tool_call" | "tool_output";

@customElement("app-message")
export class AppMessage extends LitElement {
  @property()
  message_id: string = "";

  @property()
  type: MessageType = "user";

  @property()
  tool_names: Array<string> = new Array();

  @property()
  with_thinking: boolean = false;

  @property()
  text: string = "";

  static styles = css`
    :host {
      width: 100%;
      display: block;
    }

    .bubble {
      display: block;
    }

    .user {
      border-radius: 24px;
      background: var(--secondary, "#fefefe");
      max-width: 800px;
      width: fit-content;
      float: right;
      padding: 24px;
    }

    .agent {
    }
  `;

  connectedCallback() {
    super.connectedCallback();
    // Hydrate text from server-rendered slot content when property is not set
    if (!this.text) {
      const content = this.querySelector("[data-content]");
      if (content?.textContent) {
        this.text = content.textContent;
      }
    }
  }

  render() {
    switch (this.type) {
      case "agent":
      case "user":
        return this.getMessage();
      case "tool_output":
        return this.getToolOutput();
    }
  }

  getToolOutput() {
    return html` <app-spoiler title="${this.tool_names[0]}">
      <slot></slot>
    </app-spoiler>`;
  }

  getMessage() {
    return html`
      <div class="bubble ${this.type}">
        ${this.with_thinking
          ? html`<app-spoiler title="Thinking"
              ><slot name="thinking"></slot
            ></app-spoiler>`
          : null}
        <auto-markdown .text="${this.text}"></auto-markdown>

        ${repeat(
          this.tool_names,
          (item) => item,
          (item, _) => html` <p>Called tool ${item}</p>`,
        )}
      </div>
    `;
  }
}
