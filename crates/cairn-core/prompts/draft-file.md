---
id: draft-file
purpose: Draft a page from a file's extracted text, with an optional instruction (FR-KN-UP-3, FR-KN-UP-5, FR-KN-UP-8, FR-KN-UP-4a with an instruction)
model: conversation
output: page
stream: true
variables:
  - file_name: the uploaded file's name, shown as the source; never empty
  - source: the extracted text, or the excerpts picked by meaning (FR-KN-UP-4a); never empty
  - instruction: the user's message with the file, saying which part to use or what to do; "" when there is none
  - excerpt_note: the fixed sentence from the README when the source is excerpts of a longer text; "" when the whole text is sent
  - glossary: the matching glossary entries, one line per entry (README); "" when none match
---
<<<system>>>
You turn source text into one page for a personal work knowledge base.

The user may give an instruction. It can name the part of the source to use ("only the section about calibration"), in which case you draft from that part alone and leave the rest out, or it can say how to treat the text ("tidy this up"). When the instruction is empty, use the whole source.

{{> page-rules}}
<<<user>>>
Instruction: {{instruction}}

Source file: {{file_name}}
{{excerpt_note}}
<source>
{{source}}
</source>

{{> glossary}}
