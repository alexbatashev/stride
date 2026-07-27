import argparse
import json
from pathlib import Path

import pdfplumber


def parse_pages(spec, page_count):
    if not spec:
        return list(range(page_count))
    pages = []
    for part in spec.split(","):
        bounds = part.strip().split("-", 1)
        start = int(bounds[0])
        end = int(bounds[1]) if len(bounds) == 2 else start
        if start < 1 or end < start or end > page_count:
            raise SystemExit(f"invalid page range: {part}")
        pages.extend(range(start - 1, end))
    return pages


def clean_table(table):
    return [["" if cell is None else cell for cell in row] for row in table]


def extract(path, password, page_spec, layout, include_tables):
    with pdfplumber.open(path, password=password) as pdf:
        selected = parse_pages(page_spec, len(pdf.pages))
        pages = []
        for index in selected:
            page = pdf.pages[index]
            item = {
                "page": index + 1,
                "text": page.extract_text(layout=layout) or "",
            }
            if include_tables:
                item["tables"] = [clean_table(table) for table in page.extract_tables()]
            pages.append(item)
    return pages


def escape_cell(value):
    return str(value).replace("|", "\\|").replace("\n", " ")


def table_markdown(table):
    if not table:
        return ""
    width = max(len(row) for row in table)
    rows = [row + [""] * (width - len(row)) for row in table]
    lines = ["| " + " | ".join(escape_cell(cell) for cell in rows[0]) + " |"]
    lines.append("| " + " | ".join("---" for _ in range(width)) + " |")
    lines.extend(
        "| " + " | ".join(escape_cell(cell) for cell in row) + " |" for row in rows[1:]
    )
    return "\n".join(lines)


def as_text(pages, include_tables):
    sections = []
    for page in pages:
        blocks = [f"--- Page {page['page']} ---", page["text"]]
        if include_tables:
            blocks.extend(
                f"Table {number}\n\n{table_markdown(table)}"
                for number, table in enumerate(page["tables"], start=1)
            )
        sections.append("\n\n".join(block for block in blocks if block))
    return "\n\n".join(sections) + "\n"


def main():
    parser = argparse.ArgumentParser(description="Extract text and tables from a PDF.")
    parser.add_argument("input")
    parser.add_argument("-o", "--output")
    parser.add_argument("--password")
    parser.add_argument("--pages", help="One-based pages such as 1,3-5.")
    parser.add_argument("--layout", action="store_true")
    parser.add_argument("--tables", action="store_true")
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args()

    pages = extract(args.input, args.password, args.pages, args.layout, args.tables)
    content = (
        json.dumps({"pages": pages}, indent=2, ensure_ascii=False) + "\n"
        if args.json
        else as_text(pages, args.tables)
    )
    if args.output:
        Path(args.output).write_text(content, encoding="utf-8")
        print(args.output)
    else:
        print(content, end="")


if __name__ == "__main__":
    main()
