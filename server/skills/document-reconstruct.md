+++
name = "document-reconstruct"
title = "Rebuild an OCR'd document as Word or PDF"
description = "Turn OCR Markdown (and its cropped figures) back into an editable .docx (or PDF) document"
+++

# Reconstructing a document from OCR output

Use this after the `ocr` tool has produced Markdown from a PDF or scan, when the
user wants an editable document back — a Word `.docx` (preferred) or a PDF.

The `ocr` tool writes:
- `<name>.md` — the transcription (headings `#`, paragraphs, pipe-tables, and
  `![caption](assets/figN.png)` references for figures it cropped),
- `<name>_assets/` — the cropped figure images referenced from the Markdown.

Reconstruction embeds those figure crops as-is: text reflows, but figures stay
raster images (we cannot re-vectorise a scan). Encrypted PDFs are not supported.

## Word (.docx) — preferred

Run the builder below with the `python` tool. Pass the OCR Markdown path and the
output path; asset image paths in the Markdown are resolved relative to the
Markdown file's directory (as the `ocr` tool writes them).

```python
import re, os
from docx import Document
from docx.shared import Pt, Inches
from docx.enum.text import WD_ALIGN_PARAGRAPH

MD_PATH = "/home/agent/ocr/king.md"          # <- the ocr tool's output
OUT_PATH = "/home/agent/king.docx"           # <- where to write the .docx

INLINE = re.compile(r"(\*\*.+?\*\*|\*.+?\*|`.+?`)")
IMAGE = re.compile(r"^!\[(.*?)\]\((.+?)\)\s*$")
HEADING = re.compile(r"^(#{1,6})\s+(.*)$")
TABLE_SEP = re.compile(r"^\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$")


def add_runs(paragraph, text):
    for part in INLINE.split(text):
        if not part:
            continue
        if part.startswith("**") and part.endswith("**"):
            paragraph.add_run(part[2:-2]).bold = True
        elif part.startswith("*") and part.endswith("*"):
            paragraph.add_run(part[1:-1]).italic = True
        elif part.startswith("`") and part.endswith("`"):
            paragraph.add_run(part[1:-1]).font.name = "Courier New"
        else:
            paragraph.add_run(part)


def split_row(line):
    line = line.strip().strip("|")
    return [c.strip() for c in line.split("|")]


def add_table(doc, rows):
    cols = max(len(r) for r in rows)
    table = doc.add_table(rows=0, cols=cols)
    table.style = "Table Grid"
    for i, row in enumerate(rows):
        cells = table.add_row().cells
        for j in range(cols):
            cells[j].text = ""
            add_runs(cells[j].paragraphs[0], row[j] if j < len(row) else "")
            if i == 0:
                for run in cells[j].paragraphs[0].runs:
                    run.bold = True


def add_image(doc, alt, path, base_dir):
    resolved = path if os.path.isabs(path) else os.path.join(base_dir, path)
    if os.path.exists(resolved):
        try:
            doc.add_picture(resolved, width=Inches(5.8))
            doc.paragraphs[-1].alignment = WD_ALIGN_PARAGRAPH.CENTER
        except Exception:
            doc.add_paragraph(f"[image: {alt or path}]")
    else:
        doc.add_paragraph(f"[missing image: {alt or path}]")
    if alt:
        cap = doc.add_paragraph()
        cap.alignment = WD_ALIGN_PARAGRAPH.CENTER
        run = cap.add_run(alt)
        run.italic = True
        run.font.size = Pt(9)


def build(markdown, out_path, base_dir):
    doc = Document()
    lines = markdown.replace("\r\n", "\n").split("\n")
    i = 0
    while i < len(lines):
        stripped = lines[i].strip()
        if not stripped:
            i += 1
            continue
        m = HEADING.match(stripped)
        if m:
            doc.add_heading(m.group(2).strip(), level=min(len(m.group(1)), 4))
            i += 1
            continue
        m = IMAGE.match(stripped)
        if m:
            add_image(doc, m.group(1).strip(), m.group(2).strip(), base_dir)
            i += 1
            continue
        if stripped.startswith("|") and i + 1 < len(lines) and TABLE_SEP.match(lines[i + 1].strip()):
            rows = [split_row(stripped)]
            i += 2
            while i < len(lines) and lines[i].strip().startswith("|"):
                rows.append(split_row(lines[i].strip()))
                i += 1
            add_table(doc, rows)
            doc.add_paragraph()
            continue
        buf = [stripped]
        i += 1
        while i < len(lines):
            nxt = lines[i].strip()
            if not nxt or HEADING.match(nxt) or IMAGE.match(nxt) or nxt.startswith("|"):
                break
            buf.append(nxt)
            i += 1
        add_runs(doc.add_paragraph(), " ".join(buf))
    doc.save(out_path)


with open(MD_PATH, encoding="utf-8") as f:
    build(f.read(), OUT_PATH, os.path.dirname(os.path.abspath(MD_PATH)))
print("wrote", OUT_PATH)
```

Notes:
- `python-docx` is imported as `docx`.
- Set `MD_PATH`/`OUT_PATH` to the real paths before running.
- The builder handles `#` headings, paragraphs, GitHub pipe-tables, inline
  `**bold**`/`*italic*`/`` `code` ``, and `![caption](path)` images with a
  centred italic caption underneath.

## PDF instead of Word

If the user asks for a PDF, prefer the `pdf-report` skill: convert the Markdown
to Typst and `typst compile`. That gives higher visual fidelity than Word. Embed
figure crops with Typst's `#image("assets/figN.png")`.
