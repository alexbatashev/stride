import sys, json, base64
from io import BytesIO

# Substituted by the host before execution.
PATH = "__OCR_PATH__"
MAX_PAGES = __OCR_MAX_PAGES__

# Largest side (px) a returned page raster is scaled to. Keeps stdout bounded
# while staying legible for vision transcription and figure cropping.
MAX_RASTER_SIDE = 2200


def emit(obj):
    sys.stdout.write(json.dumps(obj))


# pdfminer.six eagerly imports `cryptography` (absent on WASI) at module load,
# purely for encrypted-PDF security handlers. Inject a lazy stub so unencrypted
# documents import and run; opening an encrypted PDF raises a clear error.
def install_crypto_stub():
    import types

    if "cryptography" in sys.modules:
        return

    def make(name):
        module = types.ModuleType(name)
        module.__path__ = []

        def __getattr__(_attr):
            class _Unavailable:
                def __init__(self, *a, **k):
                    raise ImportError(
                        "cryptography is unavailable in this sandbox; "
                        "encrypted PDFs are not supported"
                    )

            return _Unavailable

        module.__getattr__ = __getattr__
        return module

    for name in (
        "cryptography",
        "cryptography.hazmat",
        "cryptography.hazmat.backends",
        "cryptography.hazmat.primitives",
        "cryptography.hazmat.primitives.ciphers",
    ):
        sys.modules[name] = make(name)


def page_texts(pages):
    texts = []
    for pg in pages:
        try:
            texts.append(pg.extract_text() or "")
        except Exception:
            texts.append("")
    return texts


def has_text_layer(texts, considered):
    stripped = [t.strip() for t in texts]
    with_text = sum(1 for t in stripped if len(t) >= 20)
    total = sum(len(t) for t in stripped)
    return considered > 0 and with_text >= (considered + 1) // 2 and total >= 40 * considered


def encode_raster(pil_image):
    image = pil_image.convert("RGB")
    image.thumbnail((MAX_RASTER_SIDE, MAX_RASTER_SIDE))
    buf = BytesIO()
    image.save(buf, format="JPEG", quality=85)
    return {
        "mime": "image/jpeg",
        "data": base64.b64encode(buf.getvalue()).decode(),
        "w": image.width,
        "h": image.height,
    }


def page_rasters(pypdf_pages):
    """Full-page rasters for the scanned path: one image per page, ready for
    vision transcription and figure cropping."""
    images = []
    empty = []
    for i, pg in enumerate(pypdf_pages):
        picked = None
        try:
            for obj in pg.images:
                if obj.image is not None:
                    picked = obj.image
                    break
        except Exception:
            picked = None
        if picked is None:
            empty.append(i)
            continue
        raster = encode_raster(picked)
        raster["page"] = i
        images.append(raster)
    return images, empty


# --- Structured Markdown assembly from the text layer (pdfplumber) -----------

def median_line_height(lines):
    hs = sorted((ln["bottom"] - ln["top"]) for ln in lines if ln["bottom"] > ln["top"])
    return hs[len(hs) // 2] if hs else 0


def is_heading(line, body_height):
    """A heading is a short line set in a larger face than the body, or a
    section-numbered title (e.g. "3. Examples"). Bracketed/inline citation
    numbers and long lines are rejected."""
    text = line["text"].strip()
    if not text or len(text) > 90 or text.replace(".", "").isdigit():
        return None  # blank, over-long, or a bare page number
    head = text.split(" ", 1)[0].rstrip(".")
    rest = text.split(" ", 1)[1] if " " in text else ""
    numbered = (
        head.isdigit()
        and len(head) <= 2
        and len(text) < 50
        and len(text.split()) <= 7
        and rest[:1].isupper()
    )
    # A larger face marks a heading only when it reads like a title: short and
    # not terminated by prose punctuation (which flags captions/sentences).
    larger = (
        body_height
        and line["height"] >= body_height * 1.2
        and len(text) < 60
        and not text.endswith((".", ",", ";", ":"))
    )
    if not (larger or numbered):
        return None
    return 2 if numbered and "." in text.split(" ", 1)[0] else 1


def table_to_markdown(table):
    rows = [[(c or "").replace("\n", " ").strip() for c in row] for row in table]
    rows = [r for r in rows if any(cell for cell in r)]
    if not rows:
        return ""
    width = max(len(r) for r in rows)
    rows = [r + [""] * (width - len(r)) for r in rows]
    header = rows[0]
    out = ["| " + " | ".join(header) + " |", "| " + " | ".join(["---"] * width) + " |"]
    for r in rows[1:]:
        out.append("| " + " | ".join(r) + " |")
    return "\n".join(out)


def lines_to_blocks(lines):
    """Merge reading-order lines into paragraph and heading blocks. Lines are
    joined until a heading or a vertical gap larger than the body line height
    starts a new block."""
    body_height = median_line_height(lines)
    blocks = []
    paragraph = []
    prev_bottom = None
    for raw in lines:
        text = raw["text"].strip()
        if not text:
            continue
        ln = {"text": text, "top": raw["top"], "bottom": raw["bottom"],
              "height": raw["bottom"] - raw["top"]}
        level = is_heading(ln, body_height)
        gap = (
            prev_bottom is not None
            and body_height
            and (ln["top"] - prev_bottom) > body_height * 0.7
        )
        if level or gap:
            if paragraph:
                blocks.append(" ".join(paragraph))
                paragraph = []
        if level:
            blocks.append("#" * (level + 1) + " " + text)
        else:
            paragraph.append(text)
        prev_bottom = ln["bottom"]
    if paragraph:
        blocks.append(" ".join(paragraph))
    return blocks


def build_page_markdown(page):
    """Assemble one page top-to-bottom: prose bands (ordered by pdfminer's
    layout analysis) with detected tables spliced in at their position."""
    tables = []
    try:
        for t in page.find_tables():
            md = table_to_markdown(t.extract())
            if md:
                tables.append({"top": t.bbox[1], "bottom": t.bbox[3], "md": md})
    except Exception:
        tables = []
    tables.sort(key=lambda t: t["top"])

    blocks = []

    def text_band(top, bottom):
        if bottom - top < 4:
            return
        band = page.crop((0, max(0, top), page.width, min(page.height, bottom)))
        blocks.extend(lines_to_blocks(band.extract_text_lines()))

    cursor = 0.0
    for t in tables:
        text_band(cursor, t["top"])
        blocks.append(t["md"])
        cursor = t["bottom"]
    text_band(cursor, page.height)

    return "\n\n".join(b for b in blocks if b.strip()), len(tables)


def build_markdown(pdfplumber_pages):
    parts = []
    table_count = 0
    for page in pdfplumber_pages:
        md, tables = build_page_markdown(page)
        table_count += tables
        if md.strip():
            parts.append(md.strip())
    return "\n\n".join(parts), table_count


def is_scan(pages):
    """A scanned document renders each page as one near-full-page raster. Such
    pages carry no usable vector structure, so we hand them to the vision model
    (which also yields clean structure and croppable figures) rather than
    trusting the embedded OCR text layer."""
    full_page = 0
    for pg in pages:
        try:
            for im in pg.images:
                area = (im["x1"] - im["x0"]) * (im["bottom"] - im["top"])
                if area > pg.width * pg.height * 0.7:
                    full_page += 1
                    break
        except Exception:
            pass
    return full_page >= max(1, int(len(pages) * 0.6))


def main():
    install_crypto_stub()
    import pdfplumber
    from pypdf import PdfReader

    reader = PdfReader(PATH)
    total = len(reader.pages)
    considered = min(total, MAX_PAGES)

    with pdfplumber.open(PATH) as pdf:
        pages = pdf.pages[:considered]
        scanned = is_scan(pages)
        texts = page_texts(reader.pages[:considered])
        if not scanned and has_text_layer(texts, considered):
            markdown, table_count = build_markdown(pages)
            emit({
                "kind": "text",
                "pages": total,
                "tables": table_count,
                "markdown": markdown,
            })
            return

    images, empty = page_rasters(reader.pages[:considered])
    emit({"kind": "page_images", "pages": total, "images": images, "empty_pages": empty})


try:
    main()
except Exception as e:
    emit({"kind": "error", "message": str(e)})
