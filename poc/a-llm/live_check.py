#!/usr/bin/env python
"""Owner-run live check of LiteLLM against REAL endpoints. Usage:
    .venv/bin/python live_check.py config.toml [--only NAME] [--no-auth-test] [--verbose]
Prints a pass/fail table per target x feature. Secrets: env var named by key_env, else hidden prompt.
Entra: AzureCliCredential (you must have run `az login` yourself). Nothing is written to disk.
"""
import argparse, base64, getpass, os, struct, sys, time, tomllib, zlib

os.environ.setdefault("LITELLM_LOCAL_MODEL_COST_MAP", "True")   # no GitHub fetch at import
os.environ.setdefault("LITELLM_LOG", "ERROR")
# LiteLLM's Azure handlers silently prefer these env keys over Entra ID; keep them only for key_env lookups.
ENV = dict(os.environ)
for _k in ("AZURE_OPENAI_API_KEY", "AZURE_API_KEY", "AZURE_AI_API_KEY"): os.environ.pop(_k, None)
import litellm  # noqa: E402

AZURE_SCOPE = "https://cognitiveservices.azure.com/.default"
FEATURES = ["basic", "stream", "effort_unset", "effort", "image", "bad_key"]
DEFAULT_LEVELS = ["low", "medium", "high"]


def red_png(n=64):
    raw = b"".join(b"\x00" + b"\xff\x00\x00" * n for _ in range(n))
    def chunk(t, d): return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", n, n, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw)) + chunk(b"IEND", b"")
    return "data:image/png;base64," + base64.b64encode(png).decode()


class Target:
    def __init__(self, c):
        self.c = c; self.name = c["name"]; self.kind = c["kind"]; self.fmt = c.get("format", "chat")
        self.model = c["model"]; self.auth = c.get("auth", "key"); self._key = None; self._prov = None

    def key(self):
        if self._key is None:
            env = self.c.get("key_env")
            self._key = (ENV.get(env) if env else None) or getpass.getpass(f"[{self.name}] API key ({env or 'none'}): ") or "dummy"
        return self._key

    def provider(self):
        if self._prov is None:
            from azure.identity import AzureCliCredential, get_bearer_token_provider
            self._prov = get_bearer_token_provider(AzureCliCredential(), AZURE_SCOPE)   # caches + refreshes
        return self._prov

    def litellm_model(self):
        p = {"azure_classic": "azure/", "foundry_v1": "openai/", "foundry_anthropic": "azure_ai/",
             "openai_compat": "openai/", "anthropic": "anthropic/"}[self.kind]
        if self.kind == "azure_classic" and self.fmt == "responses": p = "azure/responses/"
        return p + self.model

    def kwargs(self, bad_key=False):
        kw = {"model": self.litellm_model(), "num_retries": 3, "timeout": 90}
        if self.c.get("api_base"): kw["api_base"] = self.c["api_base"]
        if self.c.get("api_version"): kw["api_version"] = self.c["api_version"]
        if self.auth == "entra" and not bad_key:
            if self.kind in ("azure_classic", "foundry_anthropic"): kw["azure_ad_token_provider"] = self.provider()
            else: kw["api_key"] = self.provider()()    # openai/ prefix has no token-provider hook: fetch a fresh token per call
        else:
            kw["api_key"] = "definitely-wrong-key" if bad_key else self.key()
        if self.fmt == "responses":
            # LiteLLM fakes Responses streaming for models missing from its cost map -> register as native.
            prov, _, name = self.litellm_model().partition("/")
            name = name.removeprefix("responses/")
            litellm.register_model({f"{prov}/{name}": {"litellm_provider": prov, "supports_native_streaming": True,
                                                       "supports_reasoning": True, "supports_vision": True}})
        return kw

    def call(self, text, stream=False, effort=None, image=None, bad_key=False):
        kw = self.kwargs(bad_key)
        if self.fmt == "chat":
            content = [{"type": "text", "text": text}] + ([{"type": "image_url", "image_url": {"url": image}}] if image else [])
            kw["messages"] = [{"role": "user", "content": content}]
            budgets = self.c.get("thinking_budgets")   # optional per-level override for Claude models
            if effort and budgets:
                kw["thinking"] = {"type": "enabled", "budget_tokens": int(budgets[effort])}
                kw["max_tokens"] = int(budgets[effort]) + 1024
            elif effort:
                kw["reasoning_effort"] = effort
                if self.kind == "azure_classic": kw["allowed_openai_params"] = ["reasoning_effort"]
            return litellm.completion(stream=stream, **kw)
        content = [{"type": "input_text", "text": text}] + ([{"type": "input_image", "image_url": image}] if image else [])
        kw["input"] = [{"role": "user", "content": content}]
        if effort: kw["reasoning"] = {"effort": effort}
        return litellm.responses(stream=stream, **kw)

    def text_of(self, r):
        if self.fmt == "chat": return r.choices[0].message.content or ""
        return "".join(c.text for o in r.output if getattr(o, "type", "") == "message" for c in o.content)

    def stream_chunks(self, r):
        t0 = time.time(); n = 0; first = None; out = ""
        for ev in r:
            if self.fmt == "chat":
                d = (ev.choices[0].delta.content or "") if ev.choices else ""
            else:
                d = ev.delta if getattr(ev, "type", "") == "response.output_text.delta" else ""
            if d:
                n += 1; out += d
                first = first or time.time() - t0
        return n, first, out


def check(t, feat):
    if feat == "basic":
        r = t.call("Reply with exactly: pong"); s = t.text_of(r); return bool(s.strip()), s[:40]
    if feat == "stream":
        n, first, out = t.stream_chunks(t.call("Count from 1 to 15 separated by spaces.", stream=True))
        return n > 1, f"{n} text chunks, first after {first or 0:.2f}s (1 chunk = probably not truly streaming)"
    if feat == "effort_unset":
        r = t.call("Reply with exactly: pong"); return bool(t.text_of(r).strip()), "no effort param sent"
    if feat == "effort":
        if not t.c.get("effort", True): return None, "skipped (effort=false)"
        bad = []
        for lv in t.c.get("effort_levels", DEFAULT_LEVELS):
            try:
                if not t.text_of(t.call("Reply with exactly: pong", effort=lv)).strip(): bad.append(f"{lv}: empty reply")
            except Exception as e:
                h = hint(e); bad.append(f"{lv}: " + (f"[{h}] " if h else "") + f"{type(e).__name__}: {str(e)[:120]}".replace("\n", " "))
        levels = ",".join(t.c.get("effort_levels", DEFAULT_LEVELS))
        return not bad, ("; ".join(bad) if bad else f"levels {levels} accepted")
    if feat == "image":
        if not t.c.get("image", True): return None, "skipped (image=false)"
        r = t.call("What colour is this image? Answer with one word.", image=red_png()); s = t.text_of(r)
        return "red" in s.lower(), s[:40]
    if feat == "bad_key":
        if t.auth != "key": return None, "n/a (entra)"
        try: t.call("hi", bad_key=True)
        except Exception as e:
            code = getattr(e, "status_code", None); return code in (401, 403), f"HTTP {code}: {str(e)[:70]!r}"
        return False, "bogus key was ACCEPTED"


def hint(e):
    """Plain-language hint for the common failures, shown next to the raw error."""
    name, code, msg = type(e).__name__, getattr(e, "status_code", None), str(e).lower()
    if name == "CredentialUnavailableError" or "az login" in msg: return "Azure CLI sign-in missing or expired: run `az login`"
    if code == 401 or name == "AuthenticationError": return "the endpoint rejected the key/token: check the key, or that your account has access"
    if code == 403: return "signed in but not allowed: your account/key lacks permission on this resource"
    if code == 404 or name == "NotFoundError": return "not found: check api_base and the model/deployment name"
    if code == 400 or name in ("BadRequestError", "UnsupportedParamsError"): return "request refused: this model may not support this feature (set effort/image = false)"
    if code == 429 or name == "RateLimitError": return "rate limited even after retries: wait and run again"
    if name in ("APIConnectionError", "Timeout") or "connect" in msg: return "could not reach the endpoint: check api_base and the network"
    return ""


def main():
    ap = argparse.ArgumentParser(); ap.add_argument("config"); ap.add_argument("--only"); ap.add_argument("--no-auth-test", action="store_true"); ap.add_argument("--verbose", action="store_true")
    a = ap.parse_args()
    targets = [Target(c) for c in tomllib.load(open(a.config, "rb"))["target"] if not a.only or c["name"] == a.only]
    for t in targets:
        if t.kind == "foundry_anthropic" and "claude" not in t.model.lower():
            print(f"WARNING [{t.name}]: LiteLLM only uses the Claude route when the deployment name contains 'claude'.")
    rows = []; details = []
    for t in targets:
        row = {}
        for f in FEATURES:
            if f == "bad_key" and a.no_auth_test: row[f] = "-"; continue
            try:
                ok, note = check(t, f)
            except Exception as e:
                h = hint(e)
                ok, note = False, (f"[{h}] " if h else "") + f"{type(e).__name__}: {str(e)[:160]}".replace("\n", " ")
            row[f] = "skip" if ok is None else ("PASS" if ok else "FAIL")
            if a.verbose or ok is not True: details.append(f"{t.name}/{f}: {note}")
        rows.append((t.name, row))
    w = max([len(n) for n, _ in rows] + [6])
    print("\n" + "target".ljust(w), *[f[:12].ljust(12) for f in FEATURES])
    for n, r in rows: print(n.ljust(w), *[r[f].ljust(12) for f in FEATURES])
    print("\nNotes / failures:"); print("\n".join(details) or "(none)")
    sys.exit(1 if any(v == "FAIL" for _, r in rows for v in r.values()) else 0)


if __name__ == "__main__":
    main()
