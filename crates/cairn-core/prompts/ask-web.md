---
id: ask-web
purpose: The one message sent with the provider's web search on, or answered again from the model's own knowledge after the provider refused the search; it carries only what the user typed (FR-LLM-AS-12, NFR-SEC-2)
model: conversation
output: text
stream: true
variables:
  - today: today's date with the weekday, e.g. "2026-10-15 (Thursday)"; never empty
  - search: the fixed search instruction for the mode (README, Other conventions): with web search on, "search before you answer, cite the pages"; after a refusal, "web search is not available, answer from what you know and say so"; never empty
  - message: the user's message as typed, trimmed; never empty
---
<<<system>>>
You answer the user's message. Today is {{today}}. {{search}} Answer in the language of the message. Keep the answer short.
<<<user>>>
{{message}}
