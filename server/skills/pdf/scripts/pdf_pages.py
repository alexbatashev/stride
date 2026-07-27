import argparse
import os
import tempfile
from pathlib import Path

from pypdf import PdfReader, PdfWriter


def parse_pages(spec, page_count):
    pages = []
    for part in spec.split(","):
        bounds = part.strip().split("-", 1)
        start = int(bounds[0])
        end = int(bounds[1]) if len(bounds) == 2 else start
        if start < 1 or end < start or end > page_count:
            raise SystemExit(f"invalid page range: {part}")
        pages.extend(range(start - 1, end))
    return pages


def open_reader(path, password=None):
    reader = PdfReader(path)
    if reader.is_encrypted and (password is None or reader.decrypt(password) == 0):
        raise SystemExit(f"{path}: encrypted; provide a valid --password")
    return reader


def copy_metadata(reader, writer):
    if reader.metadata:
        writer.add_metadata(
            {
                str(key): str(value)
                for key, value in reader.metadata.items()
                if value is not None
            }
        )


def write_atomic(writer, output):
    output = Path(output)
    output.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(
            dir=output.parent, prefix=f".{output.name}.", suffix=".tmp", delete=False
        ) as handle:
            temporary = Path(handle.name)
            writer.write(handle)
        os.replace(temporary, output)
    finally:
        if temporary and temporary.exists():
            temporary.unlink()
    return len(PdfReader(output).pages)


def ensure_distinct(output, inputs):
    target = Path(output).resolve()
    if any(Path(path).resolve() == target for path in inputs):
        raise SystemExit("output must differ from every input")


def merge(args):
    ensure_distinct(args.output, args.inputs)
    writer = PdfWriter()
    for path in args.inputs:
        writer.append(open_reader(path, args.password))
    pages = write_atomic(writer, args.output)
    print(f"{args.output}: {pages} pages")


def extract(args):
    ensure_distinct(args.output, [args.input])
    reader = open_reader(args.input, args.password)
    writer = PdfWriter()
    for index in parse_pages(args.pages, len(reader.pages)):
        writer.add_page(reader.pages[index])
    copy_metadata(reader, writer)
    pages = write_atomic(writer, args.output)
    print(f"{args.output}: {pages} pages")


def rotate(args):
    if args.degrees % 90:
        raise SystemExit("--degrees must be a multiple of 90")
    ensure_distinct(args.output, [args.input])
    reader = open_reader(args.input, args.password)
    selected = (
        set(parse_pages(args.pages, len(reader.pages)))
        if args.pages
        else set(range(len(reader.pages)))
    )
    writer = PdfWriter()
    for index, page in enumerate(reader.pages):
        if index in selected:
            page.rotate(args.degrees)
        writer.add_page(page)
    copy_metadata(reader, writer)
    pages = write_atomic(writer, args.output)
    print(f"{args.output}: {pages} pages")


def main():
    parser = argparse.ArgumentParser(description="Merge, select, or rotate PDF pages.")
    commands = parser.add_subparsers(dest="command", required=True)

    merge_parser = commands.add_parser("merge")
    merge_parser.add_argument("output")
    merge_parser.add_argument("inputs", nargs="+")
    merge_parser.add_argument("--password")
    merge_parser.set_defaults(run=merge)

    extract_parser = commands.add_parser("extract")
    extract_parser.add_argument("input")
    extract_parser.add_argument("output")
    extract_parser.add_argument("--pages", required=True)
    extract_parser.add_argument("--password")
    extract_parser.set_defaults(run=extract)

    rotate_parser = commands.add_parser("rotate")
    rotate_parser.add_argument("input")
    rotate_parser.add_argument("output")
    rotate_parser.add_argument("--degrees", type=int, required=True)
    rotate_parser.add_argument("--pages")
    rotate_parser.add_argument("--password")
    rotate_parser.set_defaults(run=rotate)

    args = parser.parse_args()
    args.run(args)


if __name__ == "__main__":
    main()
