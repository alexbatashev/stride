---
name: pdf
description: Read, inspect, extract, create, convert, merge, split, rotate, and otherwise work with PDF files. Use for any PDF task, including polished reports, text or table extraction, page operations, metadata inspection, and PDF conversion.
---

# Work with PDF files

Use `/home/agent` for intermediate and output files. Keep the source unchanged
unless the user explicitly asks for in-place replacement.

## Choose the right capability

| Need | Use |
|---|---|
| Inspect metadata, pages, forms, or extract simple text | `pypdf` |
| Extract layout-aware text, words, coordinates, or tables | `pdfplumber` / `pdfminer.six` |
| Read scans or difficult mixed-layout documents | the `ocr` tool, then `document-reconstruct` if needed |
| Merge, select, reorder, or rotate pages | `pypdf` or `scripts/pdf_pages.py` |
| Create a polished report or document | Typst; start from `assets/report-template.typ` |
| Create a small programmatic PDF | `fpdf2` |
| Convert Markdown, HTML, DOCX, or another Pandoc input to PDF | Pandoc to Typst, then Typst to PDF |
| Prepare charts or raster images before embedding | `matplotlib` and Pillow |

Typst is always available as a shell command. Pandoc is a regular shell command
when the optional Pandoc support is enabled; detect it with `command -v pandoc`
or handle a command-not-found failure. Pandoc has no direct PDF engine in this
environment.

## Run bundled scripts

Bundled Python scripts use command-line arguments. Run them with the Python
tool:

```python
import runpy, sys
sys.argv = ["pdf_info.py", "/home/agent/input.pdf"]
runpy.run_path("%SKILL_HOME%/scripts/pdf_info.py", run_name="__main__")
```

- `scripts/pdf_info.py INPUT [--password PASSWORD]`: report encryption,
  metadata, page sizes, rotation, form names, and per-page text counts as JSON.
- `scripts/pdf_extract.py INPUT [-o OUTPUT] [--pages 1,3-5] [--layout]
  [--tables] [--json]`: extract text and optional detected tables.
- `scripts/pdf_pages.py merge OUTPUT INPUT...`: merge PDFs.
- `scripts/pdf_pages.py extract INPUT OUTPUT --pages 1,3-5`: select or reorder
  pages.
- `scripts/pdf_pages.py rotate INPUT OUTPUT --degrees 90 [--pages 2-4]`:
  rotate selected pages clockwise.
- `scripts/render_pdf.py INPUT OUTPUT [--main-font FONT] [--keep-typst]`:
  compile `.typ` directly or use optional Pandoc to produce intermediate Typst
  with a non-empty font fallback before compiling.

Use absolute paths. Page arguments are one-based and preserve the order given.

## Create polished PDFs

Copy `%SKILL_HOME%/assets/report-template.typ` into the workspace and adapt it.
Compile with:

```text
typst compile report.typ report.pdf
```

Author directly in Typst when layout matters. Use `fpdf2` for simple documents
whose content and placement are generated in Python. Register a Unicode font
from `/usr/share/fonts` before writing non-ASCII text with `fpdf2`.

When the input is Markdown, HTML, DOCX, ODT, EPUB, or another supported Pandoc
format:

```text
pandoc input.docx --to typst --output intermediate.typ
typst compile intermediate.typ output.pdf
```

Pandoc is not a PDF reader. For PDF-to-text or PDF-to-Markdown tasks, use
`pdfplumber` or OCR instead.

Read `references/typst.md` when authoring or debugging a non-trivial Typst
document.

## Edit and extract safely

- `pypdf` performs structural edits; it does not reflow or reliably replace
  visible text inside an existing page.
- `pypdf` can also inspect or update metadata, forms, annotations, attachments,
  outlines, page boxes, encryption, and overlays. Test these features directly:
  page selection and merging can invalidate destinations or discard document-
  level structures when they are not copied explicitly.
- `pdfplumber` extracts layout information but does not render pages or perform
  OCR.
- Poppler commands, qpdf, Ghostscript, LibreOffice, and a browser PDF renderer
  are not part of the default agent environment. Do not generate workflows that
  depend on them. PDF/A conversion, digital signing, reliable redaction, and
  pixel-faithful editing of existing page content need a separate capable tool.
- Supply a password only for a document the user is authorized to access.
  Stop on unsupported encryption rather than bypassing it.
- Treat attachments and embedded actions as untrusted. Do not execute them.
- Preserve metadata only when it is wanted; generated PDFs may otherwise leak
  author, source-path, or producer information.

## Verify every output

Reopen the result with `pypdf`, confirm the expected page count and encryption
state, and extract representative text. For layout-sensitive output, render or
inspect the final PDF on the actual application surface. Check page breaks,
clipping, fonts, tables, images, links, headers, footers, and blank pages.
