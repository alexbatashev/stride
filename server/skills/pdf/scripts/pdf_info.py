import argparse
import json

from pypdf import PdfReader


def unlock(reader, password):
    if not reader.is_encrypted:
        return
    if password is None or reader.decrypt(password) == 0:
        raise SystemExit("PDF is encrypted; provide a valid --password")


def metadata_dict(metadata):
    if not metadata:
        return {}
    return {
        str(key): str(value) for key, value in metadata.items() if value is not None
    }


def page_info(page, number):
    box = page.mediabox
    try:
        text_chars = len(page.extract_text() or "")
        text_error = None
    except Exception as error:
        text_chars = None
        text_error = str(error)
    result = {
        "page": number,
        "width_points": round(float(box.width), 2),
        "height_points": round(float(box.height), 2),
        "rotation": int(page.rotation or 0),
        "text_characters": text_chars,
    }
    if text_error:
        result["text_error"] = text_error
    return result


def inspect_pdf(path, password):
    reader = PdfReader(path)
    encrypted = reader.is_encrypted
    unlock(reader, password)
    fields = reader.get_fields() or {}
    return {
        "path": path,
        "encrypted": encrypted,
        "pages": len(reader.pages),
        "metadata": metadata_dict(reader.metadata),
        "form_fields": sorted(fields),
        "page_details": [
            page_info(page, number) for number, page in enumerate(reader.pages, start=1)
        ],
    }


def main():
    parser = argparse.ArgumentParser(description="Inspect a PDF and print JSON.")
    parser.add_argument("input")
    parser.add_argument("--password")
    args = parser.parse_args()
    print(
        json.dumps(inspect_pdf(args.input, args.password), indent=2, ensure_ascii=False)
    )


if __name__ == "__main__":
    main()
