import { Component, css, effect, onMount, ref } from "@frontiers-labs/argon";

// Matches the server-side cap; an oversized artifact shows a notice instead of
// loading a huge document into the sandbox.
const MAX_ARTIFACT_BYTES = 256 * 1024;

// Policy applied inside every artifact document. The frame already runs in an
// opaque origin (sandbox without allow-same-origin), so it cannot reach the host
// page; the CSP is defense in depth within the frame: no network egress
// (connect-src none blocks exfiltration and external loads), inline script/style
// only since the artifact is self-contained, and images limited to inline data.
const CSP = [
  "default-src 'none'",
  "script-src 'unsafe-inline'",
  "style-src 'unsafe-inline'",
  "img-src data: blob:",
  "font-src data:",
  "connect-src 'none'",
  "form-action 'none'",
  "base-uri 'none'",
  "frame-src 'none'",
].join("; ");

// Posts the rendered height to the host so the frame sizes to its contents; the
// opaque origin stops the parent from measuring the document directly.
const HEIGHT_BRIDGE = `
(function () {
  function report() {
    parent.postMessage(
      { strideArtifactHeight: document.documentElement.scrollHeight },
      "*"
    );
  }
  if (window.ResizeObserver) {
    new ResizeObserver(report).observe(document.documentElement);
  }
  window.addEventListener("load", report);
  report();
})();
`;

function buildDocument(html: string): string {
  return (
    '<!doctype html><html><head><meta charset="utf-8">' +
    `<meta http-equiv="Content-Security-Policy" content="${CSP}">` +
    '<meta name="viewport" content="width=device-width, initial-scale=1">' +
    "<style>html,body{margin:0;padding:0;}" +
    "body{font-family:system-ui,-apple-system,sans-serif;color:inherit;}</style>" +
    "</head><body>" +
    html +
    "<scr" +
    "ipt>" +
    HEIGHT_BRIDGE +
    "</scr" +
    "ipt>" +
    "</body></html>"
  );
}

function clampHeight(height: number): number {
  return Math.min(Math.max(height, 60), 4000);
}

const styles = css`
  :host {
    display: block;
    margin: 0.75em 0;
  }

  :host(:last-child) {
    margin-bottom: 0;
  }

  iframe {
    background: var(--secondary, #fff);
    border: 1px solid var(--border, #d0d0d0);
    border-radius: 12px;
    display: block;
    height: 240px;
    width: 100%;
  }

  .fallback {
    border: 1px solid var(--border, #d0d0d0);
    border-radius: 12px;
    color: var(--muted-foreground, #666);
    font-size: 0.95em;
    padding: 12px;
  }
`;

function buildFrame(source: string): HTMLIFrameElement {
  const frame = document.createElement("iframe");
  frame.setAttribute("sandbox", "allow-scripts");
  frame.setAttribute("referrerpolicy", "no-referrer");
  frame.title = "Interactive view";
  frame.srcdoc = buildDocument(source);
  return frame;
}

function buildFallback(): HTMLDivElement {
  const div = document.createElement("div");
  div.className = "fallback";
  div.textContent = "This interactive view is too large to display.";
  return div;
}

export function StrideArtifact({ source = "" }: { source?: string }): Component {
  const host = ref<HTMLDivElement>();

  onMount(() => {
    function onMessage(event: MessageEvent) {
      const el = host.current?.querySelector("iframe");
      if (!el || event.source !== el.contentWindow) {
        return;
      }
      const height = (event.data as { strideArtifactHeight?: unknown })?.strideArtifactHeight;
      if (typeof height === "number" && Number.isFinite(height)) {
        el.style.height = `${clampHeight(height)}px`;
      }
    }
    window.addEventListener("message", onMessage);
    return () => window.removeEventListener("message", onMessage);
  });

  effect(() => {
    const container = host.current;
    if (!container) {
      return;
    }
    const child = source.length > MAX_ARTIFACT_BYTES ? buildFallback() : buildFrame(source);
    container.replaceChildren(child);
  });

  return (
    <>
      <style>{styles}</style>
      <div ref={host}></div>
    </>
  );
}
