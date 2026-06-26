/*
 * Portions of this component's visual styling are adapted from shadcn/ui.
 * Copyright (c) 2023 shadcn. Licensed under the MIT License.
 */
import { Component, css, effect, ref, state } from "@frontiers-labs/argon";
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
  host.dispatchEvent(
    new CustomEvent("prompt-submit", { bubbles: true, composed: true, detail: { value } }),
  );
}

function syncSendButton(root: ShadowRoot): void {
  const textarea = root.querySelector<HTMLTextAreaElement>("textarea");
  const send = root.querySelector<HTMLButtonElement>('button[type="submit"]');
  if (send && textarea) send.disabled = textarea.disabled || !textarea.value.trim();
}

// MediaRecorder state lives outside the component body because the SSR pass
// cannot evaluate `null`/MediaRecorder initializers; keying by host keeps it
// alive across re-renders.
type RecordingState = { recorder: MediaRecorder | null; chunks: Blob[] };
const recordingStates = new WeakMap<EventTarget, RecordingState>();

function recordingStateFor(host: EventTarget): RecordingState {
  let entry = recordingStates.get(host);
  if (!entry) {
    entry = { recorder: null, chunks: [] };
    recordingStates.set(host, entry);
  }
  return entry;
}

function emitPromptError(host: HTMLElement, message: string): void {
  host.dispatchEvent(
    new CustomEvent("prompt-error", { bubbles: true, composed: true, detail: { message } }),
  );
}

function insertTranscript(root: ShadowRoot, text: string): void {
  const textarea = root.querySelector<HTMLTextAreaElement>("textarea");
  if (!textarea || !text) return;
  const existing = textarea.value.trim();
  textarea.value = existing ? `${existing} ${text}` : text;
  textarea.style.height = "";
  textarea.style.height = `${Math.min(textarea.scrollHeight, 220)}px`;
  syncSendButton(root);
  textarea.focus();
}

export function AppPromptInput({
  disabled = false,
  running = false,
  placeholder = "Send a message",
}: {
  disabled?: boolean;
  running?: boolean;
  placeholder?: string;
}): Component {
  const input = ref<HTMLTextAreaElement>();
  const micButton = ref<HTMLButtonElement>();
  let recording = state(false);
  let transcribing = state(false);

  effect(() => {
    // Boolean attributes can't be driven by JSX bindings here: server-side
    // render emits a present `disabled` attribute even for `false`, so the
    // control would stay greyed until a state change re-renders it. Toggle the
    // disabled state imperatively instead, the way the textarea and send button
    // are handled.
    const mic = micButton.current;
    if (mic) mic.disabled = transcribing;
    const textarea = input.current;
    if (!textarea) return;
    textarea.toggleAttribute("disabled", disabled || running || transcribing);
    syncSendButton(this.shadowRoot!);
  });

  return (
    <>
      <style>{styles}</style>
      <form
        onSubmit={(event: SubmitEvent) => {
          event.preventDefault();
          submitPrompt(this, root.querySelector("textarea")!);
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
            this.dispatchEvent(
              new CustomEvent("files-attach", { bubbles: true, composed: true, detail: { files } }),
            );
          }}
        />
        <textarea
          ref={input}
          placeholder={placeholder}
          rows="2"
          onInput={(event: Event) => {
            const textarea = event.currentTarget as HTMLTextAreaElement;
            textarea.style.height = "";
            textarea.style.height = `${Math.min(textarea.scrollHeight, 220)}px`;
            syncSendButton(root as ShadowRoot);
          }}
          onKeyDown={(event: KeyboardEvent) => {
            if (event.key !== "Enter" || event.shiftKey || event.isComposing) return;
            event.preventDefault();
            submitPrompt(this, event.currentTarget as HTMLTextAreaElement);
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
            <button
              ref={micButton}
              class={`tool-button icon${recording ? " recording" : ""}`}
              type="button"
              aria-label={recording ? "Stop recording" : "Record voice message"}
              aria-pressed={recording ? "true" : "false"}
              onClick={() => {
                const rec = recordingStateFor(this);
                if (transcribing) return;
                if (recording) {
                  rec.recorder?.stop();
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
                  rec.chunks.length = 0;
                  const mime = MediaRecorder.isTypeSupported("audio/webm") ? "audio/webm" : "";
                  const recorder = mime
                    ? new MediaRecorder(stream, { mimeType: mime })
                    : new MediaRecorder(stream);
                  rec.recorder = recorder;
                  recorder.ondataavailable = (event) => {
                    if (event.data.size > 0) rec.chunks.push(event.data);
                  };
                  recorder.onstop = () => {
                    recording = false;
                    for (const track of stream.getTracks()) track.stop();
                    const type = recorder.mimeType || "audio/webm";
                    rec.recorder = null;
                    const blob = new Blob(rec.chunks, { type });
                    rec.chunks.length = 0;
                    if (blob.size === 0) return;
                    transcribing = true;
                    const ext = type.includes("ogg") ? "ogg" : "webm";
                    void transcribeAudio(blob, `voice.${ext}`)
                      .then((text) => insertTranscript(root as ShadowRoot, text))
                      .catch(() => emitPromptError(this, "Could not transcribe the recording."))
                      .finally(() => {
                        transcribing = false;
                      });
                  };
                  recorder.start();
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
              this.dispatchEvent(new CustomEvent("prompt-stop", { bubbles: true, composed: true }));
            }}
          >
            {running ? (
              <button class="send stop" type="button">
                <IconStop />
                <span class="sr-only">Stop</span>
              </button>
            ) : (
              <button class="send" type="submit" disabled>
                <IconArrowUp />
                <span class="sr-only">Send message</span>
              </button>
            )}
          </div>
        </div>
      </form>
    </>
  );
}
