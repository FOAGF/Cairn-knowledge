"""Error kinds from PROTOCOL.md, and the mapping from library exceptions to them.

Only exception class names and status codes are looked at, so this module
never imports litellm, openai or httpx.
"""
import re

# JSON-RPC codes
PARSE_ERROR = -32700
INVALID_REQUEST = -32600
METHOD_NOT_FOUND = -32601
INVALID_PARAMS = -32602
APP_ERROR = -32000

# Plain-language hints (wording from poc/a-llm/live_check.py)
HINTS = {
    "signin": "Azure CLI sign-in missing or expired: run `az login`",
    "auth": "the endpoint rejected the key/token: check the key, or that your account has access",
    "forbidden": "signed in but not allowed: your account/key lacks permission on this resource",
    "not_found": "not found: check the endpoint and the model/deployment name",
    "bad_request": "request refused: this model may not support this feature (turn off effort or image input for it)",
    "rate_limit": "rate limited even after retries: wait and try again",
    "network": "could not reach the endpoint: check the endpoint address and the network",
    "too_long": "the input is longer than the model accepts: send less text",
    "web_search_unsupported": "this model or deployment can't search the web: untick its Web search option",
}

MESSAGES = {
    "signin": "You are not signed in to Azure.",
    "auth": "The provider rejected the API key or token (401).",
    "forbidden": "The provider says this account or key is not allowed to do that (403).",
    "not_found": "The endpoint, deployment or model was not found (404).",
    "bad_request": "The model refused the request (400).",
    "rate_limit": "The provider is rate limiting requests (429).",
    "network": "The provider could not be reached.",
    "too_long": "The input is longer than the model accepts.",
    "web_search_unsupported": "The provider refused web search.",
    "bad_file": "The file could not be read.",
    "internal": "The AI call failed.",
}

STATUS_KIND = {401: "auth", 403: "forbidden", 404: "not_found", 429: "rate_limit", 400: "bad_request"}

# Exception class names (anywhere in the class hierarchy) -> kind. The groups do not overlap.
NAME_KIND = {
    "AuthenticationError": "auth",
    "PermissionDeniedError": "forbidden",
    "NotFoundError": "not_found",
    "RateLimitError": "rate_limit",
    "BadRequestError": "bad_request",
    "UnsupportedParamsError": "bad_request",
    "UnprocessableEntityError": "bad_request",
    "APIConnectionError": "network",
    "APITimeoutError": "network",
    "Timeout": "network",
    "ConnectError": "network",
    "ConnectTimeout": "network",
    "ReadTimeout": "network",
    "TimeoutError": "network",
    "ConnectionError": "network",
    "CredentialUnavailableError": "signin",
    "ClientAuthenticationError": "signin",
}

# A 400 whose text says the input is too long (providers without a typed exception)
TOO_LONG_RE = re.compile(r"(input|prompt|request)\b[^.]{0,40}\btoo long|too many tokens|"
                         r"exceeds? (the )?(model'?s? )?(maximum )?context|maximum context length|"
                         r"context (length|window) (is|of|exceeded)|context_length_exceeded", re.I)
# A refused request that names web search itself: the tool ("web_search_20250305", "web_search_preview"), the
# option ("web_search_options") or the feature ("web search is not enabled"). Nothing vaguer counts.
WEB_SEARCH_RE = re.compile(r"web[_ -]?search", re.I)
LITELLM_PREFIX_RE = re.compile(r"^(litellm\.\w+: )?\w+Exception - ")
REFUSAL_NAMES = ("BadRequestError", "UnsupportedParamsError", "UnprocessableEntityError")
TIMEOUT_NAMES = ("APITimeoutError", "Timeout", "ConnectTimeout", "ReadTimeout", "TimeoutError")

# Kinds where nothing else can work, so llm.test skips the remaining checks.
FATAL_KINDS = ("auth", "signin", "network", "not_found")


class SidecarError(Exception):
    """An error that is reported to the app as-is."""

    def __init__(self, kind, message, hint=None, status=None, code=APP_ERROR):
        super().__init__(message)
        self.kind, self.message, self.hint, self.status, self.code = kind, message, hint, status, code

    def data(self):
        d = {"kind": self.kind}
        if self.status is not None:
            d["status"] = self.status
        hint = self.hint if self.hint is not None else HINTS.get(self.kind)
        if hint:
            d["hint"] = hint
        return d


class InvalidParams(SidecarError):
    def __init__(self, message):
        super().__init__("internal", message, code=INVALID_PARAMS)


def classify(e):
    """Return (kind, status) for any exception."""
    status = getattr(e, "status_code", None)
    if not isinstance(status, int):
        status = None
    if "az login" in str(e).lower():
        return "signin", status
    names = {c.__name__ for c in type(e).__mro__}
    if "ContextWindowExceededError" in names:
        return "too_long", status
    if (status == 400 or "BadRequestError" in names) and TOO_LONG_RE.search(str(e)):
        return "too_long", status
    if (status in (400, 422) or any(n in names for n in REFUSAL_NAMES)) and WEB_SEARCH_RE.search(str(e)):
        return "web_search_unsupported", status
    for name, kind in NAME_KIND.items():
        if name in names:
            return kind, status
    if status in STATUS_KIND:
        return STATUS_KIND[status], status
    # LiteLLM reports a refused or dropped connection as a 500 "Connection error"
    low = str(e).lower()
    if any(w in low for w in ("connection error", "connection refused", "timed out", "name or service not known",
                              "getaddrinfo", "network is unreachable")):
        return "network", status
    return "internal", status


def is_timeout(e):
    """True when the call timed out (as opposed to a refused or dropped connection)."""
    if any(c.__name__ in TIMEOUT_NAMES for c in type(e).__mro__):
        return True
    return "timed out" in str(e).lower() and classify(e)[0] == "network"


def from_status(status, text=""):
    """For plain HTTP answers (the model lists)."""
    kind = STATUS_KIND.get(status, "internal")
    return SidecarError(kind, _join(MESSAGES[kind], " ".join(text.split())[:300]), status=status)


def _join(base, detail):
    return f"{base} {detail}" if detail else base


def from_exception(e, secrets=()):
    """Turn any exception into a SidecarError. `secrets` are scrubbed from the text."""
    if isinstance(e, SidecarError):
        return e
    kind, status = classify(e)
    detail = " ".join(str(e).split())
    if kind == "web_search_unsupported":
        detail = LITELLM_PREFIX_RE.sub("", detail)  # keep the provider's reason, not LiteLLM's class names
    for s in secrets:
        if s and len(s) >= 4:
            detail = detail.replace(s, "***")
    return SidecarError(kind, _join(MESSAGES[kind], detail[:300]), status=status)
