/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, effect, emit, ref, state } from "@frontiers-labs/argon";
import { transcribeAudio } from "../api/threads.js";
import type { ModelOption } from "../shared/model-option.js";
import { AppButton } from "./app-button.js";
import { AppModelPicker } from "./app-model-picker.js";
import { IconArrowUp } from "./icons/arrow-up.js";
import { IconMic } from "./icons/mic.js";
import { IconPlus } from "./icons/plus.js";
import { IconStop } from "./icons/stop.js";

const styles = css`
  :host {
    display: inline-block;
    max-width: 870px;
    width: 100%;
    height: fit-content;
    max-height: none;
    padding: 8px 0 12px;
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
    border-radius: 24px;
    box-shadow: var(--prompt-shadow, none);
    box-sizing: border-box;
    display: grid;
    gap: 8px;
    min-height: 112px;
    padding: 12px 20px 10px;
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
    font-size: 1rem;
    line-height: 1.4;
    max-height: 220px;
    min-height: 44px;
    min-width: 0;
    outline: none;
    overflow-y: auto;
    padding: 0 2px;
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
    display: grid;
    gap: 16px;
    grid-template-columns: 1fr auto auto;
    min-height: 32px;
  }

  .right-actions {
    align-items: center;
    display: flex;
  }

  .attachment { --secondary: transparent; --secondary-foreground: var(--prompt-control-fg, #efefef); --secondary-hover: var(--prompt-control-hover-bg, #303030); }
  .primary-action { --primary: var(--accent); --primary-foreground: var(--accent-foreground); --primary-hover: color-mix(in oklab, var(--accent) 82%, black); }
  .primary-action.ready { --primary: var(--accent); --primary-foreground: var(--accent-foreground); --primary-hover: color-mix(in oklab, var(--accent) 82%, black); }
  .primary-action.recording { --primary: var(--prompt-record-bg, #b91c1c); --primary-foreground: #ffffff; --primary-hover: var(--prompt-record-hover-bg, #dc2626); animation: prompt-pulse 1.2s ease-in-out infinite; }
  .primary-action:has(app-button[disabled]) { opacity: 0.5; pointer-events: none; }

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

  @media (max-width: 640px) {
    :host {
      max-width: 100%;
    }

    form {
      border-radius: 28px;
      gap: 14px;
      min-height: 108px;
      padding: 14px 16px 12px;
      width: 100%;
    }

    .toolbar {
      gap: 12px;
    }

    textarea { font-size: 1rem; min-height: 44px; }
  }
`;

function submitPrompt(host: HTMLElement, textarea: HTMLTextAreaElement): void {
  const value = textarea.value.trim();
  if (!value || textarea.disabled) return;
  textarea.value = "";
  textarea.style.height = "";
  const model = host.getAttribute("data-selected-model") ?? "";
  host.dispatchEvent(
    new CustomEvent("prompt-submit", {
      bubbles: true,
      composed: true,
      detail: { value, model: model || null },
    }),
  );
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
  selectedModelLabel = "Choose model",
  selectedModelReasoningEffort = "",
}: {
  disabled?: boolean;
  running?: boolean;
  placeholder?: string;
  models?: ModelOption[];
  selectedModel?: string;
  selectedModelLabel?: string;
  selectedModelReasoningEffort?: string;
}): Component {
  const input = ref<HTMLTextAreaElement>();
  let recording = state(false);
  let transcribing = state(false);
  let draft = state("");
  let recorder: MediaRecorder | false = false;
  const chunks: Blob[] = [];
  const blocked = disabled || running || transcribing;
  const actionDisabled = transcribing || (!recording && disabled);
  const hasDraft = draft.trim() !== "";

  effect(() => {
    if (selectedModel) {
      this.setAttribute("data-selected-model", selectedModel);
    } else {
      this.removeAttribute("data-selected-model");
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
          <AppButton class="attachment" size="icon-lg" variant="secondary" aria-label="Add attachment" title="Add attachment" onClick={() => root.querySelector<HTMLInputElement>('input[type="file"]')!.click()}>
            <IconPlus />
          </AppButton>
          <AppModelPicker models={models} value={selectedModel} label={selectedModelLabel} reasoningEffort={selectedModelReasoningEffort} disabled={blocked} on:value-change={(event: CustomEvent<{ value: string }>) => emitModelChange(this, event.detail.value)} />
          <div class={`right-actions primary-action${recording ? " recording" : ""}${hasDraft && !running && !recording ? " ready" : ""}`}>
            <AppButton
              size="icon-lg"
              disabled={actionDisabled}
              aria-label={running ? "Stop response" : recording ? "Stop recording" : hasDraft ? "Send message" : "Record voice message"}
              aria-pressed={recording ? "true" : "false"}
              onClick={() => {
                if (running) {
                  emit(this, "prompt-stop");
                  return;
                }
                if (hasDraft) {
                  submitPrompt(this, root.querySelector("textarea")!);
                  draft = "";
                  return;
                }
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
              {running || recording ? <IconStop /> : hasDraft ? <IconArrowUp /> : <IconMic />}
            </AppButton>
          </div>
        </div>
      </form>
    </>
  );
}
