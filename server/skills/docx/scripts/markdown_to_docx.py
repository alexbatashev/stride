import argparse
import re
from pathlib import Path
from urllib.parse import unquote

from docx import Document
from docx.enum.text import WD_ALIGN_PARAGRAPH
from docx.shared import Inches, Pt


INLINE = re.compile(r"(\*\*.+?\*\*|\*.+?\*|`.+?`)")
IMAGE = re.compile(r"^!\[(.*?)\]\((.+?)\)\s*$")
HEADING = re.compile(r"^(#{1,6})\s+(.*)$")
TABLE_SEPARATOR = re.compile(r"^\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)*\|?\s*$")
UNORDERED = re.compile(r"^[-*+]\s+(.*)$")
ORDERED = re.compile(r"^\d+[.)]\s+(.*)$")


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
    return [cell.strip() for cell in line.strip().strip("|").split("|")]


def add_table(document, rows):
    columns = max(len(row) for row in rows)
    table = document.add_table(rows=0, cols=columns)
    table.style = "Table Grid"
    for number, row in enumerate(rows):
        cells = table.add_row().cells
        for index in range(columns):
            add_runs(cells[index].paragraphs[0], row[index] if index < len(row) else "")
            if number == 0:
                for run in cells[index].paragraphs[0].runs:
                    run.bold = True


def add_image(document, alt, path, base):
    resolved = Path(unquote(path))
    if not resolved.is_absolute():
        resolved = base / resolved
    if not resolved.is_file():
        document.add_paragraph(f"[missing image: {alt or path}]")
        return
    try:
        document.add_picture(str(resolved), width=Inches(5.8))
        document.paragraphs[-1].alignment = WD_ALIGN_PARAGRAPH.CENTER
    except Exception:
        document.add_paragraph(f"[unsupported image: {alt or path}]")
        return
    if alt:
        caption = document.add_paragraph()
        caption.alignment = WD_ALIGN_PARAGRAPH.CENTER
        run = caption.add_run(alt)
        run.italic = True
        run.font.size = Pt(9)


def build(markdown_path, output_path):
    source = Path(markdown_path)
    output = Path(output_path)
    document = Document()
    for section in document.sections:
        section.top_margin = Inches(0.8)
        section.right_margin = Inches(0.85)
        section.bottom_margin = Inches(0.8)
        section.left_margin = Inches(0.85)
    lines = source.read_text(encoding="utf-8").replace("\r\n", "\n").split("\n")
    index = 0
    while index < len(lines):
        stripped = lines[index].strip()
        if not stripped:
            index += 1
            continue
        match = HEADING.match(stripped)
        if match:
            document.add_heading(
                match.group(2).strip(), level=min(len(match.group(1)), 4)
            )
            index += 1
            continue
        match = IMAGE.match(stripped)
        if match:
            add_image(
                document, match.group(1).strip(), match.group(2).strip(), source.parent
            )
            index += 1
            continue
        if (
            stripped.startswith("|")
            and index + 1 < len(lines)
            and TABLE_SEPARATOR.match(lines[index + 1].strip())
        ):
            rows = [split_row(stripped)]
            index += 2
            while index < len(lines) and lines[index].strip().startswith("|"):
                rows.append(split_row(lines[index]))
                index += 1
            add_table(document, rows)
            continue
        match = UNORDERED.match(stripped)
        if match:
            add_runs(document.add_paragraph(style="List Bullet"), match.group(1))
            index += 1
            continue
        match = ORDERED.match(stripped)
        if match:
            add_runs(document.add_paragraph(style="List Number"), match.group(1))
            index += 1
            continue
        buffer = [stripped]
        index += 1
        while index < len(lines):
            following = lines[index].strip()
            if (
                not following
                or HEADING.match(following)
                or IMAGE.match(following)
                or UNORDERED.match(following)
                or ORDERED.match(following)
                or following.startswith("|")
            ):
                break
            buffer.append(following)
            index += 1
        add_runs(document.add_paragraph(), " ".join(buffer))
    output.parent.mkdir(parents=True, exist_ok=True)
    document.save(output)
    Document(output)
    print(output)


def main():
    parser = argparse.ArgumentParser(description="Build a basic DOCX from Markdown.")
    parser.add_argument("input")
    parser.add_argument("output")
    args = parser.parse_args()
    if Path(args.input).resolve() == Path(args.output).resolve():
        raise SystemExit("output must differ from input")
    build(args.input, args.output)


if __name__ == "__main__":
    main()
