import argparse
import subprocess
from pathlib import Path


def run(command, output):
    try:
        result = subprocess.run(command, capture_output=True, text=True)
    except FileNotFoundError:
        raise SystemExit("pandoc is not enabled") from None
    if result.returncode:
        detail = (result.stderr or result.stdout).strip()
        raise SystemExit(f"pandoc failed: {detail}")
    if not Path(output).is_file():
        raise SystemExit(f"pandoc did not create {output}")
    print(output)


def create(args):
    if Path(args.output).suffix.lower() != ".docx":
        raise SystemExit("output must end in .docx")
    command = ["pandoc", args.input, "--output", args.output]
    if args.input_format:
        command.extend(["--from", args.input_format])
    if args.reference_doc:
        command.extend(["--reference-doc", args.reference_doc])
    if args.resource_path:
        command.extend(["--resource-path", args.resource_path])
    if args.toc:
        command.append("--toc")
    run(command, args.output)


def extract(args):
    command = [
        "pandoc",
        args.input,
        "--to",
        "gfm",
        "--wrap",
        "none",
        "--output",
        args.output,
    ]
    if args.media_dir:
        command.extend(["--extract-media", args.media_dir])
    run(command, args.output)


def main():
    parser = argparse.ArgumentParser(description="Convert DOCX with optional Pandoc.")
    commands = parser.add_subparsers(dest="command", required=True)

    create_parser = commands.add_parser("create")
    create_parser.add_argument("input")
    create_parser.add_argument("output")
    create_parser.add_argument("--from", dest="input_format")
    create_parser.add_argument("--reference-doc")
    create_parser.add_argument("--resource-path")
    create_parser.add_argument("--toc", action="store_true")
    create_parser.set_defaults(run=create)

    extract_parser = commands.add_parser("extract")
    extract_parser.add_argument("input")
    extract_parser.add_argument("output")
    extract_parser.add_argument("--media-dir")
    extract_parser.set_defaults(run=extract)

    args = parser.parse_args()
    if Path(args.input).resolve() == Path(args.output).resolve():
        raise SystemExit("output must differ from input")
    Path(args.output).parent.mkdir(parents=True, exist_ok=True)
    args.run(args)


if __name__ == "__main__":
    main()
