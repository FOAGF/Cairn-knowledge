"""Web search (M6, FR-LLM-AS-12): the request side of each format, and the evidence read back from the answer.

The provider runs the search itself, so nothing here is a client tool call. The shapes are the ones the owner's
probe recorded (poc/i-assistant/out): Claude on Foundry through LiteLLM, the Responses API, and Chat Completions
`url_citation` annotations. A provider that ignores the option simply leaves the evidence empty (`searches: 0`).
"""
from .errors import InvalidParams

DEFAULT_MAX_USES = 5
CLAUDE_TOOL = "web_search_20250305"  # the only version Foundry deployments hosted on Azure accept
SERVER_TOOL_PREFIX = "srvtoolu_"  # ids of Claude's server-side tool calls


def parse_web_search(value):
    """The `web_search` param: None (off) or {"max_uses": N}. Returns None or the checked dict."""
    if value is None or value is False:
        return None
    if not isinstance(value, dict):
        raise InvalidParams("web_search must be null or an object")
    n = value.get("max_uses", DEFAULT_MAX_USES)
    if isinstance(n, bool) or not isinstance(n, int) or n < 1:
        raise InvalidParams("web_search.max_uses must be a positive integer")
    return {"max_uses": n}


def request_params(api, claude_style, web_search):
    """The LiteLLM keyword arguments that switch the provider's web search on."""
    if api == "responses":
        return {"tools": [{"type": "web_search"}]}
    if claude_style:
        # LiteLLM passes a hosted-tool dict through unchanged (AnthropicConfig._map_tool_helper), which keeps
        # max_uses exact; web_search_options would only offer 1, 5 or 10.
        return {"tools": [{"type": CLAUDE_TOOL, "name": "web_search", "max_uses": web_search["max_uses"]}]}
    return {"web_search_options": {"search_context_size": "medium"}}


def _get(o, key):
    if o is None:
        return None
    return o.get(key) if isinstance(o, dict) else getattr(o, key, None)


def _int(v):
    return v if isinstance(v, int) and not isinstance(v, bool) else None


class Evidence:
    """What the answer shows about the search: how many ran, and the pages it cited."""

    def __init__(self):
        self.reset()

    def reset(self):
        self.reported = None  # a search count the provider gave (Anthropic usage)
        self.calls = set()  # ids of server-side search calls seen
        self.sources = {}  # url -> {"url", "title", "cited_text"}, in first-cited order
        self.anonymous = 0

    # ---- sources
    def add_source(self, url, title=None, cited_text=None):
        if not isinstance(url, str) or not url or url in self.sources:
            return
        self.sources[url] = {"url": url, "title": title if isinstance(title, str) else "",
                             "cited_text": cited_text if isinstance(cited_text, str) else ""}

    def add_citations(self, c):
        """Anthropic citations: a dict, a list of them, or (non-streamed) a list of lists, one per text block."""
        if isinstance(c, (list, tuple)):
            for x in c:
                self.add_citations(x)
        elif c is not None:
            self.add_source(_get(c, "url"), _get(c, "title"), _get(c, "cited_text"))

    def add_annotations(self, items):
        """Responses / Chat Completions `url_citation` annotations (the page is under `url_citation` on Chat)."""
        for a in items or []:
            if _get(a, "type") != "url_citation":
                continue
            inner = _get(a, "url_citation") or a
            self.add_source(_get(inner, "url"), _get(inner, "title"))

    # ---- searches
    def add_call(self, call_id=None):
        if call_id:
            self.calls.add(call_id)
        else:
            self.anonymous += 1
            self.calls.add(f"_anon{self.anonymous}")

    def add_tool_calls(self, tool_calls):
        """LiteLLM shows Claude's server-side searches as tool_calls with srvtoolu_ ids."""
        for tc in tool_calls or []:
            tid = _get(tc, "id")
            if isinstance(tid, str) and tid.startswith(SERVER_TOOL_PREFIX):
                self.calls.add(tid)

    def add_psf(self, psf):
        """`provider_specific_fields` of a message or a streamed delta."""
        if not psf:
            return
        self.add_citations(_get(psf, "citations"))
        self.add_citations(_get(psf, "citation"))
        calls = _get(psf, "web_search_calls")
        if isinstance(calls, dict):
            self.calls.update(calls)

    def add_usage(self, usage):
        stu = _get(usage, "server_tool_use")
        n = _int(_get(stu, "web_search_requests"))
        if n is None:
            n = _int(_get(_get(usage, "prompt_tokens_details"), "web_search_requests"))
        if n is not None:
            self.reported = max(self.reported or 0, n)

    # ---- the three response shapes
    def read_message(self, msg):
        """A Chat Completions message (plain call)."""
        self.add_psf(_get(msg, "provider_specific_fields"))
        self.add_tool_calls(_get(msg, "tool_calls"))
        self.add_annotations(_get(msg, "annotations"))

    def read_delta(self, delta):
        """A streamed Chat Completions delta."""
        self.add_psf(_get(delta, "provider_specific_fields"))
        self.add_tool_calls(_get(delta, "tool_calls"))
        self.add_annotations(_get(delta, "annotations"))

    def read_responses(self, resp):
        """A Responses API object: `web_search_call` output items and `url_citation` annotations."""
        for item in _get(resp, "output") or []:
            kind = _get(item, "type")
            if kind == "web_search_call":
                self.add_call(_get(item, "id"))
            elif kind == "message":
                for part in _get(item, "content") or []:
                    self.add_annotations(_get(part, "annotations"))

    def result(self, claude_style=False):
        n = self.reported if self.reported is not None else len(self.calls)
        if n == 0 and self.sources and not claude_style:
            n = 1  # Chat Completions search models cite pages without counting the searches
        return {"searches": n, "sources": list(self.sources.values())}
