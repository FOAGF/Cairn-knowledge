---
id: draft-part
purpose: Draft one part of a long source that does not fit the model, no instruction (FR-KN-UP-4a); the parts are combined by draft-combine
model: conversation
output: page
stream: false
variables:
  - file_name: the uploaded file's name; never empty
  - part_index: the number of this part, counted from 1
  - part_count: the number of parts
  - source: the text of this part; never empty
  - glossary: the matching glossary entries, one line per entry (README); "" when none match
---
<<<system>>>
You draft one part of a long source text for a personal work knowledge base. The source was cut into consecutive parts because it is too long to read at once; each part is drafted on its own and the drafts are combined into one page afterwards.

For this part:
- Draft from this part only. Do not refer to other parts, and do not write an introduction, overview or summary of the whole document.
- Where the source has its own section names, keep them as the "## " headings, so that the drafts can be combined.
- The part may start or end in the middle of a sentence, list or table: use what can be read and leave out the broken remainder.
- The title on line 1 names the subject of this part.

{{> page-rules}}
<<<user>>>
Source file: {{file_name}}, part {{part_index}} of {{part_count}}
<source>
{{source}}
</source>

{{> glossary}}
