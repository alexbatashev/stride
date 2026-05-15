import { LitElement, css, html } from "lit";
import { customElement, property } from "lit/decorators.js";
import "./app-spoiler.js";
import "./auto-markdown.js";

type MessageType = "agent" | "user" | "tool_call" | "tool_output";

@customElement("app-message")
export class AppMessage extends LitElement {
  @property()
  message_id: string = "";

  @property()
  type: MessageType = "user";

  @property()
  tool_name?: string;

  @property()
  with_thinking: boolean = false;

  @property()
  text: string = "";

  static styles = css`
    .bubble {
      display: block;
      margin: 8px;
      padding: 24px;
      max-width: 100%;
      min-width: 60px;
    }

    .user {
      border-radius: 24px;
      background: var(--secondary, "#fefefe");
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
      case "tool_call":
        return this.getToolCall();
      case "tool_output":
        return this.getToolOutput();
    }
  }

  getToolCall() {
    return html`<p>Called tool ${this.tool_name}</p>`;
  }

  getToolOutput() {
    return html` <app-spoiler title="${this.tool_name}">
      <slot></slot>
    </app-spoiler>`;
  }

  getMessage() {
    return html`
      ${this.with_thinking
        ? html`<app-spoiler title="Thinking"
            ><slot name="thinking"></slot
          ></app-spoiler>`
        : null}
      <div class="bubble ${this.type}">
        ${this.type === "agent"
          ? html`<auto-markdown .text="${this.text}"></auto-markdown>`
          : this.text}
      </div>
    `;
  }
}
