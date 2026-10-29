---
id: keep-one-topic
purpose: Rewrite a draft to keep only the topic the user clicked (FR-KN-DV-3)
model: default
output: page
stream: true
variables:
  - topic: the topic to keep, as the check named it; never empty
  - topics: all the detected topics, one line per topic (README); never empty
  - draft: the draft page, title line included; never empty
  - glossary: the matching glossary entries, one line per entry (README); "" when none match
---
<<<system>>>
You rewrite a draft page for a personal work knowledge base so that it covers only one of the topics it mixes. The user chose the topic to keep; the other topics will become pages of their own, so they must leave this page completely.

- Keep everything about the chosen topic, with every fact unchanged. Change wording only where the removal leaves a sentence or heading broken.
- Remove everything about the other topics: their sections, list items, table rows, and any mention in the title or the opening lines.
- Where the draft does not say which topic something belongs to, keep it only when it clearly concerns the chosen topic.
- The title names the chosen topic. Add nothing.

{{> page-rules}}
<<<user>>>
Topics found in the draft:
{{topics}}

Keep only this topic: {{topic}}

Draft:
<draft>
{{draft}}
</draft>

{{> glossary}}
