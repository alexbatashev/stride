import argparse
import json
from pathlib import Path

from docx import Document
from docx.text.run import Run


def iter_paragraphs(container, seen):
    for paragraph in container.paragraphs:
        marker = paragraph._p
        if marker not in seen:
            seen.add(marker)
            yield paragraph
    for table in container.tables:
        for row in table.rows:
            for cell in row.cells:
                yield from iter_paragraphs(cell, seen)


def document_paragraphs(document):
    seen = set()
    yield from iter_paragraphs(document, seen)
    for section in document.sections:
        for container in (
            section.header,
            section.first_page_header,
            section.even_page_header,
            section.footer,
            section.first_page_footer,
            section.even_page_footer,
        ):
            yield from iter_paragraphs(container, seen)


def paragraph_runs(paragraph):
    runs = []
    for item in paragraph.iter_inner_content():
        if isinstance(item, Run):
            runs.append(item)
        else:
            runs.extend(item.runs)
    return runs


def replace_in_runs(runs, old, new):
    text = "".join(run.text for run in runs)
    starts = []
    cursor = 0
    while True:
        start = text.find(old, cursor)
        if start < 0:
            break
        starts.append(start)
        cursor = start + len(old)
    for start in reversed(starts):
        end = start + len(old)
        offsets = []
        cursor = 0
        for run in runs:
            offsets.append((cursor, cursor + len(run.text)))
            cursor += len(run.text)
        first = next(index for index, (_, stop) in enumerate(offsets) if stop > start)
        last = next(
            index for index, (begin, stop) in enumerate(offsets) if begin < end <= stop
        )
        first_begin = offsets[first][0]
        last_begin = offsets[last][0]
        prefix = runs[first].text[: start - first_begin]
        suffix = runs[last].text[end - last_begin :]
        if first == last:
            runs[first].text = prefix + new + suffix
        else:
            runs[first].text = prefix + new
            for index in range(first + 1, last):
                runs[index].text = ""
            runs[last].text = suffix
    return len(starts)


def load_replacements(args):
    replacements = {}
    if args.map:
        value = json.loads(Path(args.map).read_text(encoding="utf-8"))
        if not isinstance(value, dict) or not all(
            isinstance(key, str) and isinstance(item, str)
            for key, item in value.items()
        ):
            raise SystemExit("--map must contain a JSON object of string replacements")
        replacements.update(value)
    for old, new in args.replace:
        replacements[old] = new
    if not replacements or any(not old for old in replacements):
        raise SystemExit("provide at least one non-empty replacement")
    return replacements


def main():
    parser = argparse.ArgumentParser(
        description="Replace DOCX text across runs while preserving surrounding formatting."
    )
    parser.add_argument("input")
    parser.add_argument("output")
    parser.add_argument("--replace", nargs=2, action="append", default=[])
    parser.add_argument("--map")
    args = parser.parse_args()
    if Path(args.input).resolve() == Path(args.output).resolve():
        raise SystemExit("output must differ from input")

    replacements = load_replacements(args)
    document = Document(args.input)
    counts = {old: 0 for old in replacements}
    for paragraph in document_paragraphs(document):
        runs = paragraph_runs(paragraph)
        for old, new in replacements.items():
            counts[old] += replace_in_runs(runs, old, new)
    Path(args.output).parent.mkdir(parents=True, exist_ok=True)
    document.save(args.output)
    Document(args.output)
    print(
        json.dumps({"output": args.output, "replacements": counts}, ensure_ascii=False)
    )


if __name__ == "__main__":
    main()
