import argparse
import json
from collections import Counter

from docx import Document


def property_value(value):
    if value is None or isinstance(value, (str, int, float, bool)):
        return value
    return value.isoformat() if hasattr(value, "isoformat") else str(value)


def core_properties(document):
    properties = document.core_properties
    names = [
        "title",
        "subject",
        "author",
        "keywords",
        "comments",
        "category",
        "created",
        "modified",
        "last_modified_by",
        "revision",
        "language",
        "version",
    ]
    return {
        name: property_value(getattr(properties, name))
        for name in names
        if getattr(properties, name) is not None
    }


def container_counts(container):
    return {
        "paragraphs": len(container.paragraphs),
        "tables": len(container.tables),
        "text_characters": sum(
            len(paragraph.text) for paragraph in container.paragraphs
        ),
    }


def inspect_docx(path):
    document = Document(path)
    styles = Counter(
        paragraph.style.name if paragraph.style else "(none)"
        for paragraph in document.paragraphs
    )
    headings = [
        {
            "text": paragraph.text,
            "style": paragraph.style.name if paragraph.style else None,
        }
        for paragraph in document.paragraphs
        if paragraph.style and paragraph.style.name.startswith("Heading")
    ]
    sections = []
    for number, section in enumerate(document.sections, start=1):
        sections.append(
            {
                "section": number,
                "width_inches": round(section.page_width.inches, 3),
                "height_inches": round(section.page_height.inches, 3),
                "margins_inches": {
                    "top": round(section.top_margin.inches, 3),
                    "right": round(section.right_margin.inches, 3),
                    "bottom": round(section.bottom_margin.inches, 3),
                    "left": round(section.left_margin.inches, 3),
                },
                "header": container_counts(section.header),
                "footer": container_counts(section.footer),
            }
        )
    comments = getattr(document, "comments", ())
    return {
        "path": path,
        "properties": core_properties(document),
        "paragraphs": len(document.paragraphs),
        "text_characters": sum(
            len(paragraph.text) for paragraph in document.paragraphs
        ),
        "styles": dict(styles.most_common()),
        "headings": headings,
        "tables": [
            {
                "table": number,
                "rows": len(table.rows),
                "columns": len(table.columns),
                "style": table.style.name if table.style else None,
            }
            for number, table in enumerate(document.tables, start=1)
        ],
        "inline_shapes": len(document.inline_shapes),
        "comments": len(comments),
        "sections": sections,
    }


def main():
    parser = argparse.ArgumentParser(description="Inspect a DOCX and print JSON.")
    parser.add_argument("input")
    args = parser.parse_args()
    print(json.dumps(inspect_docx(args.input), indent=2, ensure_ascii=False))


if __name__ == "__main__":
    main()
