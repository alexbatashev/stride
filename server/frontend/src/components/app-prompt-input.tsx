/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, effect, emit, ref, state } from "@frontiers-labs/argon";
import { transcribeAudio } from "../api/threads.js";
import { IconArrowUp } from "./icons/arrow-up.js";
import { IconMic } from "./icons/mic.js";
import { IconPlus } from "./icons/plus.js";
import { IconSettingsHorizontal } from "./icons/settings-horizontal.js";
import { IconStop } from "./icons/stop.js";

const styles = css`
  :host {
    display: inline-block;
    max-width: 960px;
    width: 100%;
    height: fit-content;
    max-height: 250px;
    padding: 8px;
  }

  :host([hidden]) {
    display: none;
  }

  [hidden] {
    display: none !important;
  }

  form {
    background: var(--prompt-bg, #212121);
    border: 1px solid var(--prompt-border, #333333);
    border-radius: 20px;
    box-shadow: var(--prompt-shadow, none);
    box-sizing: border-box;
    display: grid;
    gap: 10px;
    padding: 12px 12px 10px;
    transition:
      border-color 140ms ease,
      box-shadow 140ms ease;
  }

  form:focus-within {
    border-color: var(--prompt-focus-border, #3c3c3c);
    box-shadow: 0 0 0 3px var(--prompt-ring, rgb(255 255 255 / 7%));
  }

  textarea {
    background: transparent;
    border: 0;
    color: var(--prompt-fg, #d4d4d4);
    font: inherit;
    font-size: 0.95rem;
    line-height: 1.4;
    max-height: 220px;
    min-height: 36px;
    min-width: 0;
    outline: none;
    overflow-y: auto;
    padding: 0;
    resize: none;
    width: 100%;
  }

  textarea::placeholder {
    color: var(--prompt-muted, #747474);
  }

  textarea:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }

  .toolbar {
    align-items: center;
    display: flex;
    gap: 8px;
    justify-content: space-between;
    min-height: 32px;
  }

  .actions {
    align-items: center;
    display: flex;
    gap: 6px;
    min-width: 0;
  }

  .right-actions {
    align-items: center;
    display: flex;
    gap: 6px;
  }

  .tool-button,
  .send {
    align-items: center;
    border-radius: 999px;
    display: inline-flex;
    flex: 0 0 auto;
    justify-content: center;
    outline: none;
    user-select: none;
    transition:
      background-color 140ms ease,
      border-color 140ms ease,
      box-shadow 140ms ease,
      color 140ms ease,
      opacity 140ms ease;
    white-space: nowrap;
  }

  .tool-button {
    background: transparent;
    border: 1px solid var(--prompt-control-border, #343434);
    color: var(--prompt-control-fg, #bdbdbd);
    cursor: pointer;
    font: inherit;
    font-size: 0.875rem;
    font-weight: 500;
    gap: 6px;
    height: 32px;
    padding: 0 12px;
  }

  .tool-button.icon {
    font-size: 1.25rem;
    padding: 0;
    width: 32px;
  }

  .tool-button:hover {
    background: var(--prompt-control-hover-bg, #2d2d2d);
    color: var(--prompt-control-hover-fg, #e4e4e7);
  }

  .send {
    background: var(--prompt-send-bg, #333333);
    border: 1px solid var(--prompt-send-bg, #333333);
    color: var(--prompt-send-fg, #777777);
    cursor: pointer;
    height: 32px;
    width: 32px;
  }

  .send:not(:disabled) {
    background: var(--prompt-send-ready-bg, #f4f4f5);
    border-color: var(--prompt-send-ready-bg, #f4f4f5);
    color: var(--prompt-send-ready-fg, #18181b);
  }

  .send:hover:not(:disabled) {
    opacity: 0.92;
  }

  .tool-button:focus-visible,
  .send:focus-visible {
    box-shadow: 0 0 0 3px var(--prompt-ring, rgb(255 255 255 / 7%));
  }

  .send:disabled {
    cursor: not-allowed;
    opacity: 0.5;
    pointer-events: none;
  }

  .send.stop {
    background: var(--prompt-send-ready-bg, #f4f4f5);
    border-color: var(--prompt-send-ready-bg, #f4f4f5);
    color: var(--prompt-send-ready-fg, #18181b);
  }

  .send.stop:hover {
    opacity: 0.92;
  }

  .tool-button.recording {
    background: var(--prompt-record-bg, #b91c1c);
    border-color: var(--prompt-record-bg, #b91c1c);
    color: #ffffff;
    animation: prompt-pulse 1.2s ease-in-out infinite;
  }

  .tool-button.recording:hover {
    background: var(--prompt-record-hover-bg, #dc2626);
    color: #ffffff;
  }

  .tool-button:disabled {
    cursor: not-allowed;
    opacity: 0.5;
    pointer-events: none;
  }

  @keyframes prompt-pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.6;
    }
  }

  .sr-only {
    border: 0;
    clip: rect(0, 0, 0, 0);
    height: 1px;
    margin: -1px;
    overflow: hidden;
    padding: 0;
    position: absolute;
    white-space: nowrap;
    width: 1px;
  }

  @media (min-width: 768px) {
    textarea {
      font-size: 1rem;
    }
  }

  .model-select select {
    background: transparent;
    border: 1px solid var(--prompt-control-border, #343434);
    border-radius: 999px;
    box-sizing: border-box;
    color: var(--prompt-control-fg, #bdbdbd);
    cursor: pointer;
    font: inherit;
    font-size: 0.8125rem;
    height: 32px;
    max-width: 180px;
    min-width: 120px;
    outline: none;
    padding: 0 28px 0 12px;
  }

  .model-select select:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }

  @media (max-width: 640px) {
    :host {
      max-width: 100%;
    }

    form {
      border-radius: 18px;
      gap: 10px;
      padding: 12px;
      width: 100%;
    }

    .toolbar {
      align-items: flex-end;
      gap: 8px;
    }

    .actions {
      gap: 8px;
      min-width: 0;
    }

    .right-actions {
      gap: 8px;
    }
  }
`;

function submitPrompt(host: HTMLElement, textarea: HTMLTextAreaElement): void {
  const value = textarea.value.trim();
  if (!value || textarea.disabled) return;
  textarea.value = "";
  textarea.style.height = "";
  const picker = host.shadowRoot?.querySelector<HTMLSelectElement>("select.model-picker");
  const model = picker?.value ?? host.getAttribute("data-selected-model") ?? "";
  host.dispatchEvent(
    new CustomEvent("prompt-submit", {
      bubbles: true,
      composed: true,
      detail: { value, model: model || null },
    }),
  );
}

function syncModelSelect(
  select: HTMLSelectElement,
  models: { value: string; label: string }[],
  selectedModel: string,
): void {
  select.replaceChildren(
    ...models.map((model) => {
      const option = document.createElement("option");
      option.value = model.value;
      option.textContent = model.label;
      return option;
    }),
  );
  if (selectedModel && models.some((model) => model.value === selectedModel)) {
    select.value = selectedModel;
  } else if (models.length > 0) {
    select.value = models[0]!.value;
  }
}

function emitModelChange(host: HTMLElement, value: string): void {
  if (value) {
    host.setAttribute("data-selected-model", value);
  } else {
    host.removeAttribute("data-selected-model");
  }
  host.dispatchEvent(
    new CustomEvent("model-change", {
      bubbles: true,
      composed: true,
      detail: { value },
    }),
  );
}

function emitPromptError(host: HTMLElement, message: string): void {
  host.dispatchEvent(
    new CustomEvent("prompt-error", { bubbles: true, composed: true, detail: { message } }),
  );
}

function insertTranscript(root: ShadowRoot, text: string): string {
  const textarea = root.querySelector<HTMLTextAreaElement>("textarea");
  if (!textarea || !text) return textarea?.value ?? "";
  const existing = textarea.value.trim();
  textarea.value = existing ? `${existing} ${text}` : text;
  textarea.style.height = "";
  textarea.style.height = `${Math.min(textarea.scrollHeight, 220)}px`;
  textarea.focus();
  return textarea.value;
}

export function AppPromptInput({
  disabled = false,
  running = false,
  placeholder = "Send a message",
  models = [],
  selectedModel = "",
}: {
  disabled?: boolean;
  running?: boolean;
  placeholder?: string;
  models?: { value: string; label: string }[];
  selectedModel?: string;
}): Component {
  const input = ref<HTMLTextAreaElement>();
  let recording = state(false);
  let transcribing = state(false);
  let draft = state("");
  let recorder: MediaRecorder | false = false;
  const chunks: Blob[] = [];
  const blocked = disabled || running || transcribing;
  const micDisabled = transcribing || (!recording && (disabled || running));

  effect(() => {
    if (selectedModel) {
      this.setAttribute("data-selected-model", selectedModel);
    } else {
      this.removeAttribute("data-selected-model");
    }
    const modelSelect = this.shadowRoot?.querySelector<HTMLSelectElement>("select.model-picker");
    if (modelSelect) {
      syncModelSelect(modelSelect, models, selectedModel);
    }
  });

  return (
    <>
      <style>{styles}</style>
      <form
        onSubmit={(event: SubmitEvent) => {
          event.preventDefault();
          submitPrompt(this, root.querySelector("textarea")!);
          draft = "";
        }}
      >
        <input
          type="file"
          multiple
          style="display:none"
          onChange={(event: Event) => {
            const picker = event.target as HTMLInputElement;
            const files = Array.from(picker.files ?? []);
            picker.value = "";
            if (files.length === 0) return;
            emit(this, "files-attach", { files });
          }}
        />
        <textarea
          ref={input}
          placeholder={placeholder}
          disabled={blocked}
          rows="2"
          onInput={(event: Event) => {
            const textarea = event.currentTarget as HTMLTextAreaElement;
            draft = textarea.value;
            textarea.style.height = "";
            textarea.style.height = `${Math.min(textarea.scrollHeight, 220)}px`;
          }}
          onKeyDown={(event: KeyboardEvent) => {
            if (event.key !== "Enter" || event.shiftKey || event.isComposing) return;
            event.preventDefault();
            submitPrompt(this, event.currentTarget as HTMLTextAreaElement);
            draft = "";
          }}
        ></textarea>
        <div class="toolbar">
          <div class="actions">
            <button
              class="tool-button icon"
              type="button"
              aria-label="Add attachment"
              onClick={() => root.querySelector<HTMLInputElement>('input[type="file"]')!.click()}
            >
              <IconPlus />
            </button>
            <button class="tool-button icon" type="button" aria-label="Tools">
              <IconSettingsHorizontal />
            </button>
            <div class="model-select">
              <select
                class="model-picker"
                disabled={blocked}
                onChange={(event: Event) =>
                  emitModelChange(this, (event.currentTarget as HTMLSelectElement).value)
                }
              ></select>
            </div>
            <button
              class={`tool-button icon${recording ? " recording" : ""}`}
              type="button"
              disabled={micDisabled}
              aria-label={recording ? "Stop recording" : "Record voice message"}
              aria-pressed={recording ? "true" : "false"}
              onClick={() => {
                if (transcribing) return;
                if (recording) {
                  recorder?.stop();
                  return;
                }
                if (disabled || running) return;
                if (
                  !navigator.mediaDevices?.getUserMedia ||
                  typeof MediaRecorder === "undefined"
                ) {
                  emitPromptError(this, "Voice input is not supported in this browser.");
                  return;
                }
                void (async () => {
                  let stream: MediaStream;
                  try {
                    stream = await navigator.mediaDevices.getUserMedia({ audio: true });
                  } catch {
                    emitPromptError(this, "Microphone access was denied.");
                    return;
                  }
                  chunks.length = 0;
                  const mime = MediaRecorder.isTypeSupported("audio/webm") ? "audio/webm" : "";
                  const nextRecorder = mime
                    ? new MediaRecorder(stream, { mimeType: mime })
                    : new MediaRecorder(stream);
                  recorder = nextRecorder;
                  nextRecorder.ondataavailable = (event) => {
                    if (event.data.size > 0) chunks.push(event.data);
                  };
                  nextRecorder.onstop = () => {
                    recording = false;
                    for (const track of stream.getTracks()) track.stop();
                    const type = nextRecorder.mimeType || "audio/webm";
                    recorder = false;
                    const blob = new Blob(chunks, { type });
                    chunks.length = 0;
                    if (blob.size === 0) return;
                    transcribing = true;
                    const ext = type.includes("ogg") ? "ogg" : "webm";
                    void transcribeAudio(blob, `voice.${ext}`)
                      .then((text) => {
                        draft = insertTranscript(root as ShadowRoot, text);
                      })
                      .catch((error) =>
                        emitPromptError(
                          this,
                          error instanceof Error && error.message
                            ? error.message
                            : "Could not transcribe the recording.",
                        ),
                      )
                      .finally(() => {
                        transcribing = false;
                      });
                  };
                  nextRecorder.start();
                  recording = true;
                })();
              }}
            >
              {recording ? <IconStop /> : <IconMic />}
            </button>
          </div>
          <div
            class="right-actions"
            onClick={(event: Event) => {
              if (!(event.target as Element).closest(".stop")) return;
              emit(this, "prompt-stop");
            }}
          >
            <button class="send stop" type="button" hidden={!running}>
              <IconStop />
              <span class="sr-only">Stop</span>
            </button>
            <button class="send" type="submit" hidden={running} disabled={blocked || draft.trim() === ""}>
              <IconArrowUp />
              <span class="sr-only">Send message</span>
            </button>
          </div>
        </div>
      </form>
    </>
  );
}
