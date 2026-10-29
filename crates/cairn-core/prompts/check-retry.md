---
id: check-retry
purpose: The one retry when a JSON answer could not be read (FR-KN-SV-1, FR-KN-GR-3); appended after the unreadable reply
model: default
output: json
stream: false
variables: []
---
<<<user>>>
That was not a valid JSON object. Reply with the JSON object only, in the shape given above: no text before or after it, no code fence.
