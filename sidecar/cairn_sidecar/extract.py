"""`file.extract` (role `ai`): read an uploaded file locally, no LLM call (FR-KN-UP-1, -4, -4b).

Prototype: poc/h-create/extract.py. pypdfium2, python-docx and Pillow are imported on first use,
so `ping` stays fast (TC-12). Pages that need the OCR model come back as JPEG files in `out_dir`.
"""
import os
import re

from .errors import InvalidParams, SidecarError

SCANNED_BELOW = 25   # non-space characters on a PDF page below which it counts as scanned
DEFAULT_DPI = 200
DEFAULT_MAX_EDGE = 2000
JPEG_QUALITY = 85

KINDS = {".md": "text", ".txt": "text", ".pdf": "pdf", ".docx": "docx",
         ".png": "image", ".jpg": "image", ".jpeg": "image"}


def bad_file(why):
    return SidecarError("bad_file", f"The file could not be read: {why}",
                        hint="The file may be damaged, password-protected, or not what its extension says.")


# ---------------------------------------------------------------- RPC method
def file_extract(req):
    p = req.params
    path, out_dir = p.get("path"), p.get("out_dir")
    if not isinstance(path, str) or not path or not isinstance(out_dir, str) or not out_dir:
        raise InvalidParams("path and out_dir must be strings")
    max_edge, dpi = _positive(p, "max_edge", DEFAULT_MAX_EDGE), _positive(p, "dpi", DEFAULT_DPI)
    kind = KINDS.get(os.path.splitext(path)[1].lower())
    if kind is None:
        raise InvalidParams(f"unsupported file type: {os.path.splitext(path)[1] or '(none)'}")
    try:
        return extract(path, kind, out_dir, max_edge, dpi)
    except SidecarError:
        raise
    except Exception as e:  # noqa: BLE001 - damaged files raise all sorts of library errors
        raise bad_file(type(e).__name__ if not isinstance(e, FileNotFoundError) else "file not found")


def _positive(params, key, default):
    v = params.get(key)
    if v is None:
        return default
    if isinstance(v, bool) or not isinstance(v, (int, float)) or v <= 0:
        raise InvalidParams(f"{key} must be a positive number")
    return v


def extract(path, kind, out_dir, max_edge=DEFAULT_MAX_EDGE, dpi=DEFAULT_DPI):
    if kind == "text":
        return {"kind": "text", "pages": 1, "parts": [{"page": 1, "text": read_text(path)}]}
    if kind == "docx":
        return {"kind": "docx", "pages": 1, "parts": [{"page": 1, "text": docx_markdown(path)}]}
    if kind == "pdf":
        parts = pdf_parts(path, out_dir, max_edge, dpi)
        return {"kind": "pdf", "pages": len(parts), "parts": parts}
    from PIL import Image
    os.makedirs(out_dir, exist_ok=True)
    with Image.open(path) as img:
        part = write_jpeg(img, os.path.join(out_dir, "image.jpg"), max_edge)
    return {"kind": "image", "pages": 1, "parts": [{"page": 1, **part}]}


# ---------------------------------------------------------------- text
def read_text(path):
    """UTF-8 first (a BOM is dropped); Chinese Windows tools still write GB18030/GBK."""
    with open(path, "rb") as f:
        raw = f.read()
    for enc in ("utf-8-sig", "gb18030"):
        try:
            return raw.decode(enc)
        except UnicodeDecodeError:
            pass
    return raw.decode("latin-1")


# ---------------------------------------------------------------- images
def write_jpeg(img, out_path, max_edge):
    """Upright, at most `max_edge` pixels on the long side, written as JPEG. Returns the part fields."""
    from PIL import ImageOps
    img = ImageOps.exif_transpose(img)
    if img.mode not in ("L", "RGB"):
        img = img.convert("RGB")
    img.thumbnail((max_edge, max_edge))
    img.save(out_path, "JPEG", quality=JPEG_QUALITY)
    return {"image_path": out_path, "width": img.size[0], "height": img.size[1]}


# ---------------------------------------------------------------- PDF
def pdf_parts(path, out_dir, max_edge, dpi):
    import pypdfium2 as pdfium
    try:
        pdf = pdfium.PdfDocument(path)  # password-protected and damaged files raise PdfiumError
    except pdfium.PdfiumError:
        raise bad_file("the PDF is damaged or password-protected")
    parts = []
    try:
        for i, page in enumerate(pdf, 1):
            text = page.get_textpage().get_text_bounded().replace("\r\n", "\n").replace("\r", "\n")
            if len(re.sub(r"\s", "", text)) >= SCANNED_BELOW:
                parts.append({"page": i, "text": text.strip()})
            else:
                os.makedirs(out_dir, exist_ok=True)
                bitmap = page.render(scale=dpi / 72, grayscale=True)
                parts.append({"page": i, **write_jpeg(bitmap.to_pil(), os.path.join(out_dir, f"p{i:04d}.jpg"), max_edge)})
    finally:
        pdf.close()
    return parts


# ---------------------------------------------------------------- DOCX
def docx_markdown(path):
    """Body paragraphs and tables in document order, as Markdown. Headers, footers, pictures and text boxes are skipped."""
    from docx import Document
    from docx.table import Table
    from docx.text.paragraph import Paragraph
    doc = Document(path)
    out, number = [], 0
    for el in doc.element.body.iterchildren():
        tag = el.tag.rsplit("}", 1)[-1]
        if tag == "tbl":
            out.append(table_markdown(Table(el, doc)))
            number = 0
            continue
        if tag != "p":
            continue
        p = Paragraph(el, doc)
        text = p.text.strip()
        if not text:
            continue
        style = (p.style.name if p.style is not None else "") or ""
        if style == "Title":
            out.append(f"# {text}")
        elif m := re.match(r"Heading (\d)", style):
            out.append(f"{'#' * min(6, int(m.group(1)))} {text}")
        elif style.startswith("List Number"):
            number += 1
            out.append(f"{number}. {text}")
            continue
        elif style.startswith("List"):
            out.append(f"- {text}")
            continue
        else:
            out.append(text)
        number = 0
    # Items of one list stay tight; everything else is separated by a blank line.
    marker = lambda line: "1" if re.match(r"\d+\. ", line) else ("-" if line.startswith("- ") else None)  # noqa: E731
    md, last = "", None
    for line in out:
        md += ("\n" if marker(line) and marker(line) == last else "\n\n") + line if md else line
        last = marker(line)
    return md


def table_markdown(table):
    cell = lambda c: c.text.strip().replace("\n", " ").replace("|", "\\|")  # noqa: E731
    rows = [[cell(c) for c in row.cells] for row in table.rows]
    if not rows:
        return ""
    width = max(len(r) for r in rows)
    rows = [r + [""] * (width - len(r)) for r in rows]
    lines = ["| " + " | ".join(rows[0]) + " |", "|" + "---|" * width]
    lines += ["| " + " | ".join(r) + " |" for r in rows[1:]]
    return "\n".join(lines)
