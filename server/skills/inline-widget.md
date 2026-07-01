+++
name = "inline-widget"
title = "Create inline HTML widgets"
description = "Build small interactive inline experiences for rich responses. Use this skill to provide interactive widgets by embedding an iframe with a small HTML page."
+++
Create inline HTML widgets

Use this skill when a user would benefit from a small interactive explanation,
simulation, chart, calculator, or visual aid inside the chat response.

## When to use an iframe widget

Good fits:

- Explaining dynamic concepts: sorting algorithms, derivatives, optimization,
  graph traversal, probability, physics, or timelines.
- Letting the user drag sliders, press step/play buttons, inspect data points,
  or switch between modes.
- Showing a compact visualization that is easier to understand by interacting
  with it than by reading prose.

Do not use a widget for ordinary text answers, simple static tables, or anything
that needs network access to work.

## Output flow

1. Create a standalone `.html` file in the writable workspace. Use a URL-safe
   ASCII filename with no spaces, e.g. `sorting-widget.html`.
2. Link `/static/common.css` from the page.
3. Include `/static/widget-frame.js` so the parent iframe resizes to the page
   content height instead of showing internal scrollbars.
4. Use only local code, inline data, and libraries served from `/static`.
5. In the final answer, include a short prose explanation and an iframe whose
   `src` is the created file's absolute public download URL.

If no configured public URL is available, do not emit an iframe. Link the HTML
file instead.

## iframe constraints

The chat renderer displays iframes inline at 100% of the message width. The
frame uses a sandbox that allows scripts, so the widget must be self-contained
and must not rely on parent-window access.

Generated widget files are not served from `/static`. `/static` is only for
built-in assets such as `/static/common.css`, `/static/widget-frame.js`, and
bundled libraries in `/static/vendor/`.

The final message iframe must use an absolute `src` beginning with the configured
public URL and pointing at the thread file-download route:

```text
<configured-public-url>/api/threads/<thread-id>/files/<relative-widget-path>
```

If you created `/~workspace/sorting-widget.html`, the `<relative-widget-path>` is
`sorting-widget.html`; drop the writable directory prefix. If you created
`/~workspace/widgets/sorting-widget.html`, the path is
`widgets/sorting-widget.html`.

Correct:

```html
<iframe src="https://stride.example.com/api/threads/<thread-id>/files/sorting.html"></iframe>
```

Wrong:

```html
<iframe src="/api/threads/<thread-id>/files/sorting.html"></iframe>
<iframe src="/static/sorting.html"></iframe>
<iframe src="/~workspace/sorting.html"></iframe>
```

Do not add inline styles, classes, ids, event handlers, scripts, or other custom
attributes to the iframe in the chat message. The sanitizer keeps only the `src`
and adds the sandbox.

## Network and data rules

- Do not request scripts, styles, fonts, images, APIs, or data from external
  hosts.
- Do not use CDN URLs.
- Do not call `fetch()` unless the URL starts with the same base URL as the
  iframe page.
- Prefer embedding small datasets directly in the HTML.
- If you need a bundled library, use the global builds:

```html
<script src="/static/vendor/d3.global.js"></script>
<script>
  const x = d3.scaleLinear([0, 10], [0, 100]);
</script>
```

Available bundled libraries:

- `/static/vendor/d3.global.js` exposes `d3`. Use it for scales, axes, shapes,
  force layouts, transitions, data joins, hierarchy, geo rendering, and lower
  level visualization work.
- `/static/vendor/plot.global.js` exposes `Plot`. Use it for concise
  Observable Plot charts when a standard chart should take less code than raw
  D3.
- `/static/vendor/decimal.global.js` exposes `Decimal`. Use it for calculators
  where decimal precision matters, especially money, percentages, rates, and
  unit conversions.
- `/static/vendor/dagre.global.js` exposes `dagre`. Use it for directed graph
  layout such as workflows, state machines, dependency graphs, and execution
  plans.

Common library patterns:

```html
<script src="/static/vendor/plot.global.js"></script>
<script>
  const data = [
    { label: "A", value: 4 },
    { label: "B", value: 9 },
  ];
  document.querySelector("#vis").append(
    Plot.plot({
      width: 640,
      height: 260,
      marginLeft: 36,
      x: { label: null },
      y: { grid: true },
      marks: [Plot.barY(data, { x: "label", y: "value" })],
    }),
  );
</script>
```

```html
<script src="/static/vendor/decimal.global.js"></script>
<script>
  const total = new Decimal("19.99").times("1.0825").toFixed(2);
</script>
```

```html
<script src="/static/vendor/dagre.global.js"></script>
<script>
  const graph = new dagre.graphlib.Graph();
  graph.setGraph({ rankdir: "LR", nodesep: 32, ranksep: 56 });
  graph.setDefaultEdgeLabel(() => ({}));
  graph.setNode("start", { width: 96, height: 40 });
  graph.setNode("finish", { width: 96, height: 40 });
  graph.setEdge("start", "finish");
  dagre.layout(graph);
</script>
```

Avoid ES module imports inside widgets unless you have a specific reason. The
iframe is sandboxed without same-origin privileges, and classic scripts are the
most reliable way to load bundled widget libraries.

Use native browser APIs first. Use bundled libraries when they materially reduce
complexity or improve correctness.

## Page skeleton

Use this structure as the default starting point:

```html
<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <link rel="stylesheet" href="/static/common.css">
  <script src="/static/widget-frame.js" defer></script>
  <title>Interactive widget</title>
  <style>
    .bars {
      display: grid;
      grid-template-columns: repeat(var(--count), minmax(0, 1fr));
      align-items: end;
      gap: 4px;
      min-height: 220px;
      padding: 12px;
    }
  </style>
</head>
<body class="widget-page">
  <main class="widget">
    <header class="widget-header">
      <div>
        <h1 class="widget-title">Interactive title</h1>
        <p class="widget-subtitle">One sentence that explains the control.</p>
      </div>
      <div class="widget-toolbar">
        <label class="widget-control">
          Speed
          <input id="speed" type="range" min="1" max="10" value="5">
        </label>
        <button class="widget-button" id="play">Play</button>
      </div>
    </header>

    <section class="widget-panel">
      <div class="widget-vis" id="vis"></div>
    </section>
  </main>

  <script type="module">
    const vis = document.querySelector("#vis");
    const resize = () => {
      const rect = vis.getBoundingClientRect();
      // Render from rect.width and rect.height so the widget adapts to iframe size.
    };
    new ResizeObserver(resize).observe(vis);
    resize();
  </script>
</body>
</html>
```

## common.css environment

`/static/common.css` provides the app typography, light/dark theme variables, and
widget utility classes. `/static/widget-frame.js` reports the document height to
the chat iframe host whenever the widget layout changes; include it in every
widget so the iframe expands inline and avoids internal scrolling.

Important theme variables:

- `--background`, `--foreground`: page background and body text.
- `--card`, `--card-foreground`: panel surfaces and panel text.
- `--primary`, `--primary-foreground`, `--primary-hover`: primary controls.
- `--secondary`, `--secondary-foreground`, `--secondary-hover`: secondary controls.
- `--muted`, `--muted-foreground`: subdued surfaces and helper text.
- `--border`, `--input`, `--ring`: borders, form controls, and focus treatment.
- `--destructive`: errors or dangerous states.

Important widget classes:

- `widget-page`: put this on `<body>` for standalone iframe pages. It resets the
  app shell layout, applies theme colors, and makes the page fill the iframe.
- `widget`: outer flexible column. It fills the viewport height and keeps spacing
  consistent.
- `widget-header`, `widget-title`, `widget-subtitle`: compact title area.
- `widget-toolbar`: responsive wrapping control row.
- `widget-control`: label plus input/select layout.
- `widget-button`, `widget-button secondary`: primary and secondary buttons.
- `widget-panel`: bordered card-like surface for controls or explanations.
- `widget-grid`, `widget-stat`: responsive metric/summary layouts.
- `widget-vis`: flexible visualization viewport for SVG or canvas.
- `widget-muted`: subdued helper text.

You may add a small page-local `<style>` block for the visualization-specific
layout, but keep colors tied to the theme variables above. The widget must work
in both light and dark mode.

## Responsive and interaction requirements

- Design for the iframe width, not a fixed desktop canvas.
- Use `ResizeObserver`, SVG `viewBox`, CSS grid/flex, or canvas redraws so the
  widget adapts when the message column changes width.
- Always include `<script src="/static/widget-frame.js" defer></script>`. Do not
  hand-roll iframe height messaging unless the helper is insufficient.
- Keep controls usable on narrow screens; wrap toolbars and avoid horizontal
  overflow.
- Keep all text inside its container. Prefer short labels.
- Respect `prefers-reduced-motion` for animations or provide a manual step mode.
- Keep state in memory or URL hash only; do not use external persistence.
- Make the initial state useful before the user clicks anything.

## Final response pattern

After creating the HTML file, respond with concise prose plus the iframe:

```html
<p>Here is an interactive view of how the algorithm moves values into place.</p>
<iframe src="https://stride.example.com/api/threads/<thread-id>/files/sorting.html"></iframe>
```
