---
id: ocr
purpose: Transcribe a scanned page or an image, text only (FR-KN-UP-4, FR-KN-UP-4b, FR-KN-UP-9)
model: ocr
output: text
stream: false
variables: []
---
<<<user>>>
Transcribe all the text in this image exactly as written, in reading order and in its original language and script. Handwriting counts as text.
- Keep the original spelling, numbers, punctuation and line breaks. Do not translate, correct, complete or summarise anything.
- Use Markdown only where the layout clearly shows structure (headings, lists, tables). Otherwise write plain lines, one per line of text.
- Ignore pictures, drawings, sketches, logos, stamps and diagrams: do not describe or mention them.
- Write [unreadable] in place of a word or line you cannot read. If the image has no text at all, reply with [no text].
- When several images are given, they are consecutive pages: transcribe them in order and put a line containing only [page break] between them.
Reply with the transcription only: no preamble, no notes, no code fences.
