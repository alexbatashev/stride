# Typst notes

Use this reference when authoring or debugging a non-trivial PDF in Typst.

## Core syntax

- Use `= Title`, `== Section`, and `=== Subsection` for headings.
- Use `*bold*`, `_italic_`, `-` for bullets, and `+` for numbered items.
- Insert images with `#image("chart.png", width: 70%)`.
- Add a caption and label with
  `#figure(image("chart.png"), caption: [Revenue]) <revenue>`, then cite
  `@revenue`.
- Put math in `$ ... $`. Use `_` for subscripts, `^` for superscripts, and `/`
  for fractions.
- Put document-wide `#set` rules at the top.

## Headers and footers

Put alignment inside a content block:

```typst
header: [
  #align(right)[Header text]
],
footer: context [
  #align(center)[#counter(page).display() / #counter(page).final().first()]
]
```

Dynamic values such as `counter(page).final()` require `context`.

## Parser and layout pitfalls

- A leading `(*)` starts bold markup and can cause an unclosed delimiter. Use
  `(Note)` instead.
- Prefer plain text or `#raw("code")` to backtick code spans inside table cells.
- Keep the `columns:` tuple, the `align:` tuple, and every row at the same
  column count.
- Balance both `]` and `)` in `#footnote([https://example.test/path])`.
- Replace unusual Unicode arrows in sensitive table or footnote content if a
  parser error is hard to locate.

## Locate an unhelpful compile error

Compile progressively larger valid prefixes of the file until the failure
returns. Inspect the narrowed range for unmatched delimiters, a stray `*` or
`$`, table row length mismatches, and backticks inside content blocks.
