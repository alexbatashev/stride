---
name: document-reconstruct
description: Turn OCR Markdown and its cropped figures back into an editable DOCX or PDF document. Use when reconstructing documents from OCR output.
---

# Reconstruct a document from OCR output

Use this after the `ocr` tool produces:

- `<name>.md` with headings, paragraphs, pipe tables, and image references;
- `<name>_assets/` with cropped figures referenced by the Markdown.

Text will reflow. Figure crops remain raster images; a scan cannot be
re-vectorized automatically. Encrypted PDFs are not supported.

## Reconstruct Word

Prefer optional Pandoc when it is enabled as a regular shell command:

```text
pandoc input.md --output output.docx --resource-path /path/to/input-directory
```

The `docx` skill also provides
`/usr/share/skills/docx/scripts/pandoc_docx.py` for the same workflow. When
Pandoc is disabled, run the deterministic fallback:

```python
import runpy, sys
sys.argv = [
    "markdown_to_docx.py",
    "/home/agent/input.md",
    "/home/agent/output.docx",
]
runpy.run_path(
    "/usr/share/skills/docx/scripts/markdown_to_docx.py",
    run_name="__main__",
)
```

Relative image paths are resolved from the Markdown file. The fallback handles
headings, paragraphs, lists, GitHub pipe tables, bold, italic, inline code, and
local images with captions.

Load the `docx` skill for editing, preservation limits, and verification.

## Reconstruct PDF

Load the `pdf` skill. For a polished reconstruction, adapt the OCR Markdown into
Typst and compile it. If optional Pandoc is enabled, its Markdown-to-Typst output
is a useful starting point:

```text
pandoc input.md --to typst --standalone --output intermediate.typ
typst compile intermediate.typ output.pdf
```

Check every image path after conversion and inspect page breaks, tables,
captions, and reading order in the final PDF.
