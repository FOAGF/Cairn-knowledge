#!/usr/bin/env python
"""Prototype of the sidecar's file extraction (FR-KN-UP-4, FR-KN-UP-4b). Runs locally, no LLM.

    sidecar/.venv/bin/python poc/h-create/extract.py poc/h-create/samples/*

`extract(path)` returns the text it could read and, for scanned PDF pages and images, the page
images that still need the OCR model:
    {"type": "pdf", "parts": [{"page": 1, "text": "..."}, {"page": 2, "image": b"<jpeg>", "mime": "image/jpeg"}]}
Pictures, diagrams and figures are never described: DOCX pictures and PDF images on pages that
have a text layer are skipped; a page with (almost) no text layer counts as scanned.
"""
import io, os, re, sys, time

SCANNED_BELOW = 25        # non-space characters on a PDF page below which it counts as scanned
RENDER_DPI = 200          # scanned PDF pages are drawn at this resolution...
MAX_EDGE = 2000           # ...and every image sent to the OCR model is scaled to fit in this many pixels
JPEG_QUALITY = 85


def extract(path):
    ext = os.path.splitext(path)[1].lower()
    if ext in (".md", ".txt"):
        return {"type": "text", "parts": [{"page": 1, "text": read_text(path)}]}
    if ext == ".pdf":
        return {"type": "pdf", "parts": pdf_parts(path)}
    if ext == ".docx":
        return {"type": "docx", "parts": [{"page": 1, "text": docx_markdown(path)}]}
    if ext in (".png", ".jpg", ".jpeg"):
        from PIL import Image
        return {"type": "image", "parts": [{"page": 1, **jpeg(Image.open(path))}]}
    raise ValueError(f"unsupported file type: {ext}")


def read_text(path):
    """UTF-8 first; Chinese Windows tools still write GB18030/GBK."""
    raw = open(path, "rb").read()
    for enc in ("utf-8-sig", "gb18030"):
        try:
            return raw.decode(enc)
        except UnicodeDecodeError:
            pass
    return raw.decode("latin-1")


def jpeg(img):
    """Upright, at most MAX_EDGE pixels on the long side, as JPEG bytes."""
    from PIL import ImageOps
    img = ImageOps.exif_transpose(img)
    if img.mode not in ("L", "RGB"):
        img = img.convert("RGB")
    img.thumbnail((MAX_EDGE, MAX_EDGE))
    buf = io.BytesIO()
    img.save(buf, "JPEG", quality=JPEG_QUALITY)
    return {"image": buf.getvalue(), "mime": "image/jpeg", "size": img.size}


def pdf_parts(path):
    import pypdfium2 as pdfium
    pdf = pdfium.PdfDocument(path)
    parts = []
    try:
        for i, page in enumerate(pdf, 1):
            text = page.get_textpage().get_text_bounded().replace("\r\n", "\n").replace("\r", "\n")
            if len(re.sub(r"\s", "", text)) >= SCANNED_BELOW:
                parts.append({"page": i, "text": text.strip()})
            else:
                bitmap = page.render(scale=RENDER_DPI / 72, grayscale=True)
                parts.append({"page": i, **jpeg(bitmap.to_pil())})
    finally:
        pdf.close()
    return parts


def docx_markdown(path):
    """Body paragraphs and tables in document order, as Markdown. Headers, footers and pictures are skipped."""
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
    # Lists stay tight; everything else is separated by a blank line.
    md = ""
    for line in out:
        tight = re.match(r"(\d+\.|-) ", line) and re.match(r"(\d+\.|-) ", md.rsplit("\n", 1)[-1] or "")
        md += ("\n" if tight else "\n\n") + line if md else line
    return md


def table_markdown(table):
    cell = lambda c: c.text.strip().replace("\n", " ").replace("|", "\\|")
    rows = [[cell(c) for c in row.cells] for row in table.rows]
    if not rows:
        return ""
    width = max(len(r) for r in rows)
    rows = [r + [""] * (width - len(r)) for r in rows]
    lines = ["| " + " | ".join(rows[0]) + " |", "|" + "---|" * width]
    lines += ["| " + " | ".join(r) + " |" for r in rows[1:]]
    return "\n".join(lines)


def rss_mb():
    for line in open("/proc/self/status"):
        if line.startswith("VmRSS:"):
            return int(line.split()[1]) / 1024
    return 0.0


def main():
    before = rss_mb()
    t0 = time.perf_counter()
    import docx, pypdfium2, PIL.Image  # noqa: F401  (import cost, measured once)
    print(f"imports: {1000 * (time.perf_counter() - t0):.0f} ms, +{rss_mb() - before:.0f} MB RSS")
    for path in sys.argv[1:]:
        if path.endswith((".json", ".truth.txt")):
            continue
        t0 = time.perf_counter()
        r = extract(path)
        ms = 1000 * (time.perf_counter() - t0)
        scanned = [p["page"] for p in r["parts"] if "image" in p]
        chars = sum(len(p.get("text", "")) for p in r["parts"])
        print(f"\n=== {os.path.basename(path)}: {r['type']}, {len(r['parts'])} part(s), {chars} chars of text, "
              f"needs OCR: {scanned or 'no'}, {ms:.0f} ms")
        for p in r["parts"]:
            if "image" in p:
                print(f"--- page {p['page']}: image {p['size'][0]}x{p['size'][1]}, {len(p['image']) // 1024} KB")
            else:
                print(f"--- page {p['page']}:\n{p['text'][:1200]}")
    print(f"\npeak RSS now: {rss_mb():.0f} MB")


if __name__ == "__main__":
    main()
