import sys, json, base64
from io import BytesIO

# Substituted by the host before execution.
PATH = "__OCR_PATH__"
MAX_PAGES = __OCR_MAX_PAGES__


def emit(obj):
    sys.stdout.write(json.dumps(obj))


def extract_text(pages):
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


def extract_images(pages):
    from PIL import Image

    images = []
    empty = []
    for i, pg in enumerate(pages):
        found = False
        try:
            for obj in pg.images:
                im = obj.image
                if im is None:
                    continue
                im = im.convert("RGB")
                im.thumbnail((2000, 2000))
                buf = BytesIO()
                im.save(buf, format="JPEG", quality=85)
                images.append(
                    {"page": i, "mime": "image/jpeg", "data": base64.b64encode(buf.getvalue()).decode()}
                )
                found = True
        except Exception:
            pass
        if not found:
            empty.append(i)
    return images, empty


try:
    from pypdf import PdfReader

    reader = PdfReader(PATH)
    all_pages = reader.pages
    n = len(all_pages)
    pages = all_pages[:MAX_PAGES]
    considered = len(pages)
    texts = extract_text(pages)
    if has_text_layer(texts, considered):
        emit({"kind": "text", "pages": n, "text": texts})
    else:
        images, empty = extract_images(pages)
        emit({"kind": "images", "pages": n, "images": images, "empty_pages": empty})
except Exception as e:
    emit({"kind": "error", "message": str(e)})
