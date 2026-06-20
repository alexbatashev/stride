+++
name = "report-writing"
title = "Report Writing"
description = "Write polished reports and documents with Typst: file layout, compiling, and core syntax."
+++
# Report Writing

For reports and formatted documents, author in Typst rather than Markdown or
LaTeX. Save the source as a `.typ` file in the workspace, then compile it in the
shell:

```
typst compile report.typ          # -> report.pdf
typst compile report.typ out.svg --format svg
```

## Typst syntax basics

- Headings: `= Title`, `== Section`, `=== Subsection` (more `=` is deeper).
- Emphasis: `*bold*`, `_italic_`. Blank line starts a new paragraph.
- Bullet list with `-`, numbered list with `+`; indent lines to nest them.
- Images: `#image("chart.png", width: 70%)`. With a caption and label:
  `#figure(image("chart.png"), caption: [Revenue]) <rev>`, referenced as `@rev`.
- Math: wrap in `$ ... $`, e.g. `$ Q = rho A v $`; `_` subscript, `^` superscript, `/` fraction.
- Call any function from markup with `#`, e.g. `#set text(size: 11pt)`,
  `#set page(margin: 2cm)`, `#pagebreak()`.

Put `#set` styling rules at the top for document-wide formatting, keep one idea
per section, and compile to PDF for the final deliverable.
