/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, effect, emit, ref, state } from "@frontiers-labs/argon";
import { transcribeAudio } from "../api/threads.js";
import type { ModelOption } from "../shared/model-option.js";
import type { PromptAttachment } from "../shared/prompt-attachment.js";
import { AppAttachment } from "./app-attachment.js";
import { AppButton } from "./app-button.js";
import { AppModelPicker } from "./app-model-picker.js";
import { IconArrowUp } from "./icons/arrow-up.js";
import { IconFile } from "./icons/file.js";
import { IconMic } from "./icons/mic.js";
import { IconPlus } from "./icons/plus.js";
import { IconStop } from "./icons/stop.js";
import { IconX } from "./icons/x.js";

const styles = css`
  :host {
    display: flex;
    flex-direction: column;
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

  .attachments {
    display: flex;
    gap: 8px;
    margin: 0 0 8px;
    max-width: 100%;
    overflow-x: auto;
    padding: 2px 20px;
    scroll-snap-type: x proximity;
    scrollbar-width: thin;
  }

  .attachments app-attachment {
    flex: 0 0 auto;
    max-width: min(240px, calc(100vw - 80px));
    scroll-snap-align: start;
  }

  .attachment-file-icon { color: var(--muted-foreground); height: 18px; width: 18px; }
  .attachment-remove { --foreground: var(--muted-foreground); }
  .attachment-remove:hover { --foreground: var(--foreground); }

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

    .attachments { padding-inline: 16px; }

    .toolbar {
      gap: 12px;
    }

    textarea { font-size: 1rem; min-height: 44px; }
  }
`;

function submitPrompt(host: HTMLElement, textarea: HTMLTextAreaElement, uploadPending = false): boolean {
  const value = textarea.value.trim();
  if (!value || textarea.disabled || uploadPending) return false;
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
  return true;
}

function formatFileSize(size: number): string {
  if (size < 1024) return `${size} B`;
  if (size < 1024 * 1024) return `${Math.round(size / 1024)} KB`;
  if (size < 10 * 1024 * 1024) return `${Math.round(size / (1024 * 1024) * 10) / 10} MB`;
  return `${Math.round(size / (1024 * 1024))} MB`;
}

function attachmentType(name: string): string {
  const normalized = name.toLowerCase();
  if (normalized.endsWith(".pdf")) return "PDF";
  if (normalized.endsWith(".csv")) return "CSV";
  if (normalized.endsWith(".zip")) return "ZIP";
  if (normalized.endsWith(".txt")) return "TXT";
  if (normalized.endsWith(".md")) return "MD";
  if (normalized.endsWith(".json")) return "JSON";
  if (normalized.endsWith(".png")) return "PNG";
  if (normalized.endsWith(".jpg") || normalized.endsWith(".jpeg")) return "JPG";
  if (normalized.endsWith(".webp")) return "WEBP";
  return "File";
}

function attachmentDescription(attachment: PromptAttachment): string {
  if (attachment.state === "uploading") return `Uploading · ${formatFileSize(attachment.size)}`;
  if (attachment.state === "error") return "Upload failed. Remove and try again.";
  return `${attachmentType(attachment.name)} · ${formatFileSize(attachment.size)}`;
}

function hasPendingUpload(attachments: PromptAttachment[]): boolean {
  return attachments.some((attachment) => attachment.state === "uploading");
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
  attachments = [],
}: {
  disabled?: boolean;
  running?: boolean;
  placeholder?: string;
  models?: ModelOption[];
  selectedModel?: string;
  selectedModelLabel?: string;
  selectedModelReasoningEffort?: string;
  attachments?: PromptAttachment[];
}): Component {
  const input = ref<HTMLTextAreaElement>();
  let recording = state(false);
  let transcribing = state(false);
  let draft = state("");
  let recorder = state(false as MediaRecorder | false);
  let chunks = state([] as Blob[]);
  const blocked = disabled || running || transcribing;
  const hasDraft = draft.trim() !== "";
  const actionDisabled = transcribing || (!recording && (disabled || hasPendingUpload(attachments)));

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
      {attachments.length > 0 && (
        <div class="attachments" role="group" aria-label="Attached files" tabIndex="0">
          {attachments.map((attachment) => (
            <AppAttachment
              key={attachment.key}
              state={attachment.state}
              size="sm"
              title={attachment.name}
              description={attachmentDescription(attachment)}
            >
              <IconFile slot="media" class="attachment-file-icon" />
              <AppButton
                slot="actions"
                class="attachment-remove"
                variant="ghost"
                size="icon-xs"
                aria-label={`Remove ${attachment.name}`}
                title={`Remove ${attachment.name}`}
                onClick={() => emit(this, "attachment-remove", { key: attachment.key })}
              >
                <IconX />
              </AppButton>
            </AppAttachment>
          )).join("")}
        </div>
      )}
      <form
        onSubmit={(event: SubmitEvent) => {
          event.preventDefault();
          if (submitPrompt(this, root.querySelector("textarea")!, hasPendingUpload(attachments))) draft = "";
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
            if (submitPrompt(this, event.currentTarget as HTMLTextAreaElement, hasPendingUpload(attachments))) draft = "";
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
                if (draft.trim() !== "") {
                  if (submitPrompt(this, root.querySelector("textarea")!, hasPendingUpload(attachments))) draft = "";
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
