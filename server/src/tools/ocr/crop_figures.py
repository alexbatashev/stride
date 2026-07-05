import sys, json, base64
from io import BytesIO

# Substituted by the host before execution.
PATH = "__OCR_PATH__"
# List of {"n": int, "page": int, "box": [x0, y0, x1, y1]} with box coordinates
# in a 0-1000 space relative to each page's full raster.
REQUESTS = json.loads("""__CROP_REQUESTS__""")

# Fraction of the figure size to pad the crop by on each side, so tight model
# boxes keep captions/axis labels.
PAD = 0.02


def emit(obj):
    sys.stdout.write(json.dumps(obj))


def full_page_raster(page):
    for obj in page.images:
        if obj.image is not None:
            return obj.image.convert("RGB")
    return None


def crop_one(raster, box):
    w, h = raster.width, raster.height
    x0, y0, x1, y1 = box
    x0, x1 = sorted((x0, x1))
    y0, y1 = sorted((y0, y1))
    px = (x1 - x0) * PAD
    py = (y1 - y0) * PAD
    left = max(0, int((x0 - px) / 1000 * w))
    top = max(0, int((y0 - py) / 1000 * h))
    right = min(w, int((x1 + px) / 1000 * w))
    bottom = min(h, int((y1 + py) / 1000 * h))
    if right - left < 8 or bottom - top < 8:
        return None
    crop = raster.crop((left, top, right, bottom))
    buf = BytesIO()
    crop.save(buf, format="PNG")
    return base64.b64encode(buf.getvalue()).decode()


def main():
    from pypdf import PdfReader

    reader = PdfReader(PATH)
    rasters = {}
    crops = []
    for req in REQUESTS:
        page = req["page"]
        if page not in rasters:
            try:
                rasters[page] = full_page_raster(reader.pages[page])
            except Exception:
                rasters[page] = None
        raster = rasters[page]
        if raster is None:
            continue
        try:
            data = crop_one(raster, req["box"])
        except Exception:
            data = None
        if data:
            crops.append({"n": req["n"], "mime": "image/png", "data": data})
    emit({"crops": crops})


try:
    main()
except Exception as e:
    emit({"crops": [], "error": str(e)})
