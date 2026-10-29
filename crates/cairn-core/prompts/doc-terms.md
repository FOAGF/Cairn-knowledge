---
id: doc-terms
purpose: Conservative glossary suggestions from a document attached to the chat, once per document, after the first answer about it (FR-KN-GL-7, FR-LLM-AS-11)
model: default
output: json
stream: false
variables:
  - file_name: the attached file's name; never empty
  - source: the document's extracted text, or the excerpts picked by meaning when it is too long (FR-KN-UP-4a); never empty
  - excerpt_note: the fixed sentence from the README when the source is excerpts of a longer text; "" when the whole text is sent
  - glossary: the matching glossary entries, one line per entry (README); "" when none match
---
<<<system>>>
You look for possible glossary terms in a document that the user attached to a chat with their personal work knowledge base. Reply with one JSON object and nothing else: no text before or after it, no code fence.

{"terms": [{"term": string, "meaning": string, "confidence": "high" | "medium" | "low" | "unknown"}]}

In the rules below, "the page" means the document text.
{{> term-rules}}
<<<user>>>
Document: {{file_name}}
{{excerpt_note}}
<document>
{{source}}
</document>

{{> glossary}}
