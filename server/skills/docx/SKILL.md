---
name: docx
description: Create, inspect, extract, convert, and edit Microsoft Word DOCX documents. Use for Word document tasks involving .docx files, including templated creation, text replacement, tables, headings, headers, footers, images, metadata, or conversion through Pandoc.
---

# Work with Word documents

Use `/home/agent` for intermediate and output files. Preserve the source and
write a new `.docx` unless the user explicitly asks to replace it.

## Choose the workflow

- Use `python-docx` for structured creation and targeted edits to paragraphs,
  runs, styles, tables, sections, headers, footers, images, core properties,
  comments, and relationships.
- Use `lxml` only when a required OOXML feature is not exposed by
  `python-docx`. Make narrow namespace-aware XML edits and reopen the result.
- Use optional Pandoc for Markdown/HTML/ODT/EPUB-to-DOCX conversion, DOCX
  extraction to Markdown, citations, tables of contents, and reference-document
  styling.
- Use the `pdf` skill for PDF output. Pandoc can convert DOCX to intermediate
  Typst, and Typst can compile that to PDF.

Pandoc is a regular shell command when the optional Pandoc support is enabled;
detect it with `command -v pandoc` or handle a command-not-found failure. There
is no Microsoft Word or LibreOffice renderer in the default environment.

## Run bundled scripts

Run a bundled script with the Python tool:

```python
import runpy, sys
sys.argv = ["docx_inspect.py", "/home/agent/input.docx"]
runpy.run_path("%SKILL_HOME%/scripts/docx_inspect.py", run_name="__main__")
```

- `scripts/docx_inspect.py INPUT`: print document properties, sections,
  headings, style counts, tables, images, comments, headers, and footers as
  JSON.
- `scripts/docx_replace.py INPUT OUTPUT --replace OLD NEW [--replace OLD NEW
  ...]`: replace text across split runs while retaining the first affected
  run's formatting. Use `--map replacements.json` for a JSON object of
  replacements.
- `scripts/markdown_to_docx.py INPUT.md OUTPUT.docx`: build a basic DOCX from
  headings, paragraphs, lists, pipe tables, inline emphasis, code, and local
  images without Pandoc.
- `scripts/pandoc_docx.py create INPUT OUTPUT [--from FORMAT]
  [--reference-doc TEMPLATE.docx] [--resource-path DIR] [--toc]`: create DOCX
  with optional Pandoc.
- `scripts/pandoc_docx.py extract INPUT OUTPUT.md [--media-dir DIR]`: extract
  GitHub-Flavored Markdown and media with optional Pandoc.

## Create or edit with `python-docx`

Use built-in styles or a user-supplied template. Set page size, margins, base
font, heading styles, paragraph spacing, and table styles before adding content.
Use semantic styles instead of direct formatting where possible.

Keep calculations and source data outside presentation code. Build tables from
validated data, set widths deliberately, repeat header rows when needed, and
add meaningful alt text to images when the OOXML API permits it.

Do not replace text through `paragraph.text` on a formatted document: it
collapses runs and loses character formatting. Search through runs or use the
bundled replacement script. Include headers, footers, and table cells in any
document-wide edit.

## Use Pandoc deliberately

For a new document whose source is Markdown:

```text
pandoc input.md --output output.docx --reference-doc template.docx
```

For editable extraction:

```text
pandoc input.docx --to gfm --wrap none \
  --extract-media extracted-media --output output.md
```

A reference DOCX controls styles, page setup, headers, and footers; it is not
concatenated with the source. Pandoc conversion is not layout-preserving for
floating shapes, text boxes, tracked changes, macros, or advanced fields.

## Preserve unsupported Word features

`python-docx` does not expose every Word feature. Opening and saving may not
preserve macros or some advanced content controls, fields, charts, drawing
objects, tracked changes, and custom XML exactly. For a preservation-sensitive
edit:

1. Inspect the ZIP package and relevant OOXML parts.
2. Change only the required XML or relationship.
3. Keep unknown parts and relationships untouched.
4. Reopen and compare package entries and representative content.

Do not rename `.docm` to `.docx`; macro-enabled files need an explicit
macro-preserving workflow.

## Verify every output

Reopen the result with `python-docx`, confirm paragraph/table/section counts,
check representative styles and relationships, and inspect headers and footers.
Use `docx_inspect.py` before and after a preservation-sensitive edit. If visual
layout is part of the request, state that structural verification is not a Word
render and use an available renderer or converted PDF for visual inspection.
