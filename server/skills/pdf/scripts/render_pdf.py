import argparse
import subprocess
from pathlib import Path


def run(command):
    try:
        result = subprocess.run(command, capture_output=True, text=True)
    except FileNotFoundError:
        raise SystemExit(f"{command[0]} is not enabled") from None
    if result.returncode:
        detail = (result.stderr or result.stdout).strip()
        raise SystemExit(f"{command[0]} failed: {detail}")


def main():
    parser = argparse.ArgumentParser(
        description="Compile Typst, or convert a Pandoc input through Typst, to PDF."
    )
    parser.add_argument("input")
    parser.add_argument("output")
    parser.add_argument("--from", dest="input_format")
    parser.add_argument("--main-font", default="Libertinus Serif")
    parser.add_argument("--keep-typst", action="store_true")
    args = parser.parse_args()

    source = Path(args.input)
    output = Path(args.output)
    if not source.is_file():
        raise SystemExit(f"input does not exist: {source}")
    if output.suffix.lower() != ".pdf":
        raise SystemExit("output must end in .pdf")
    if source.resolve() == output.resolve():
        raise SystemExit("output must differ from input")
    output.parent.mkdir(parents=True, exist_ok=True)

    intermediate = source
    generated = source.suffix.lower() != ".typ"
    if generated:
        intermediate = output.with_name(f".{output.stem}.pandoc.typ")
        command = [
            "pandoc",
            str(source),
            "--standalone",
            "--to",
            "typst",
            "--variable",
            f"mainfont={args.main_font}",
            "--output",
            str(intermediate),
        ]
        if args.input_format:
            command.extend(["--from", args.input_format])
        run(command)

    try:
        run(["typst", "compile", str(intermediate), str(output)])
    finally:
        if generated and not args.keep_typst and intermediate.exists():
            intermediate.unlink()
    print(output)


if __name__ == "__main__":
    main()
