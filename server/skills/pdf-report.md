+++
name = "pdf-report"
title = "Write a PDF report"
description = "Write polished reports and documents in PDF format: file layout, compiling, and core syntax."
+++
Write a PDF report

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

## Common Typst pitfalls

## 1. Page header/footer content blocks

**Don't** use the `align: <direction> + <anchor)[ ... ]` shorthand inside
`#set page(header: ...)`.

```typst
// BROKEN — "unexpected colon"
header: align: right + horizon)[ ... ],
footer: align: center + bottom)[ ... ],
```

**Do** use plain content blocks and place `#align(...)` inside them.

```typst
header: [
  #align(right)[Header text]
],
footer: context [
  #align(center)[#counter(page).display() / #counter(page).final().first()]
]
```

---

## 2. `context` is required for dynamic values in headers/footers

`counter(page).final()` returns a value that depends on document state.
Any use of it must be wrapped in `context`.

```typst
// BROKEN — "can only be used when context is known"
footer: [
  #counter(page).display() / #counter(page).final().first()
]

// WORKS
footer: context [
  #counter(page).display() / #counter(page).final().first()
]
```

The `context` keyword can prefix a content block: `context [ ... ]`.

---

## 3. `(*)` in body text starts bold markup

A line that begins with `(*)` is parsed as the opening of a `*bold*` span
that is never closed → "unclosed delimiter".

```typst
// BROKEN — unclosed delimiter
(*) On Blackwell, matrix A can also reside in TMEM.
```

**Do** avoid leading `*` in prose. Use a different marker.

```typst
(Note) On Blackwell, matrix A can also reside in TMEM.
```

This also applies to `(*` anywhere Typst sees it as emphasis markup, e.g.
inside table cell content `[Shared Memory (*), TMEM]`.

---

## 4. Backtick code spans inside table cells

Single-backtick `` `code` `` spans work in regular markup paragraphs but
caused issues inside `#table()` cell content `[…]` in this Typst version.

**Workaround:** use plain text without backticks, or `#raw("…")` if raw
formatting is essential.

```typst
// Risky inside table cells
[Ampere (sm80)], [`mma`], [Registers]

// Safe
[Ampere (sm80)], [mma], [Registers]
```

If you do use `#raw("…")`, strip it back to plain text afterward — it adds
visual noise and the plain text compiles reliably.

---

## 5. Unicode arrows in prose

`→` (U+2192) rendered fine in body paragraphs and figure captions, but
the safe choice is to replace it in sensitive locations (table cells,
footnotes) with `to` or `->` to avoid any parser ambiguity.

---

## 6. Table column count must match content

When editing an existing `#table()` to add a column, update **both** the
`columns: (…)` tuple, every `align:` entry, and **every data row**.
A row with fewer cells than columns produces silent misalignment or, in
some cases, an unclosed-delimiter error that points nowhere useful.

---

## 7. Debugging strategy: bisect with `sed`

Typst's error messages in this version are minimal — often just
"unclosed delimiter" with no line number.

**Bisect procedure:**

1. `head -N report.typ > test.typ && echo '= End' >> test.typ && typst compile test.typ`
2. If it compiles, increase N; if not, decrease N.
3. Narrow until the failing range is 10–15 lines.
4. Inspect those lines for the patterns above (`*`, backticks, `(*)`,
   stray `$`, unmatched `[` / `]` / `(` / `)`).

This is far faster than eyeballing a 600-line file.

---

## 8. `#footnote([…])` with URLs containing special chars

Footnote content `[ … ]` is a content block. URLs with `/`, `:`, and `?`
are fine, but ensure the closing `])` is balanced — a missing `]` or `)`
propagates as "unclosed delimiter" far from the actual site.

---

## Summary cheat-sheet

| Pattern | Problem | Fix |
|---|---|---|
| `header: align: X)[…]` | unexpected colon | `header: [#align(X)[…]]` |
| `counter(page).final()` in footer | context unknown | wrap footer in `context […]` |
| `(*)` at line start | unclosed delimiter (bold start) | use `(Note)` or remove `*` |
| `` `code` `` in table cells | parse ambiguity | use plain text |
| 5-col table, 6-col header row | unclosed delimiter | match all rows to column count |
| No line numbers in errors | hard to locate | bisect with `sed -n 'A,Bp'` |
