"""Checks LiteLLM's route table for the routes Cairn uses (FR-LLM-LIB-2, D-82). Standard library only.

The sidecar bundle leaves out LiteLLM's Rust extension (`litellm/rust_bridge/_native`) and the app runs
the sidecar with LITELLM_RUST=false. That is safe only while every route Cairn uses runs in Python: the
table in `litellm/rust_bridge/catalog.py` must make each of them PYTHON_ONLY or RUST_OPT_IN (opt-in is
off under LITELLM_RUST=false). RUST_OPT_OUT and RUST_REQUIRED stop the build: read LiteLLM's change notes
before an upgrade goes out.

    <bundled python> -I check-litellm-routes.py --import
        imports the catalog and asks it (build-sidecar.sh, in the smoke test; LITELLM_RUST=false set)
    python3 -I check-litellm-routes.py --source <site-packages>/litellm/rust_bridge/catalog.py
        reads the rules from the file without importing it (build-sidecar-windows.sh, where the
        Windows Python cannot run). Anything in the file it doesn't understand stops the build.
"""

import ast
import sys

# What Cairn calls (sidecar/cairn_sidecar/ai.py): litellm.completion and litellm.responses, and the
# Anthropic messages route, with these providers. Model names are free, so a rule for a single model
# counts too. The logger and the tokenizers run inside those calls.
ROUTES = ("chat_completions", "responses", "messages")
PROVIDERS = ("openai", "azure", "azure_ai", "anthropic")
TOKENIZERS = ("tiktoken", "huggingface")
ALLOWED = {"PYTHON_ONLY", "RUST_OPT_IN"}

# catalog.rollout() as of LiteLLM 1.105.0: the first matching rule wins, PYTHON_ONLY when none does.
ROLLOUT_BODY = ("selected_rules: Final = RULES if rules is None else rules\n"
                "return next((rule.rollout for rule in selected_rules if rule.matches(context)), Rollout.PYTHON_ONLY)")


def contexts():
    """(label, route, provider) for every pair Cairn uses."""
    for route in ROUTES:
        for provider in PROVIDERS:
            yield f"{route} / {provider}", route, provider
    for tok in TOKENIZERS:
        yield f"tokenizer / {tok}", "tokenizer", tok


def report(problems, how):
    if problems:
        print("[litellm-routes] FAILED: every route Cairn uses must run in Python, because the bundle leaves out")
        print("[litellm-routes] LiteLLM's Rust extension (FR-LLM-LIB-2, D-82):")
        for p in problems:
            print(f"[litellm-routes]   {p}")
        sys.exit(1)
    n = len(ROUTES) * len(PROVIDERS) + len(TOKENIZERS)
    print(f"[litellm-routes] {n} routes Cairn uses run in Python ({how})")


# ---------------------------------------------------------------- --import (the bundled Python)
def check_import():
    from litellm.rust_bridge import catalog
    from litellm.rust_bridge.configuration import Decision

    problems = []
    for label, route, provider in contexts():
        try:
            route_enum = catalog.Route(route)
        except ValueError:
            problems.append(f"{label}: LiteLLM has no route '{route}' any more")
            continue
        ctx = catalog.RouteContext(route_enum, provider=provider)
        rollout = catalog.rollout(ctx)
        if rollout.name not in ALLOWED:
            problems.append(f"{label}: {rollout.name}")
        elif catalog.decision(ctx) is not Decision.PYTHON:
            problems.append(f"{label}: {rollout.name}, and not Python here (is LITELLM_RUST=false set?)")
        for rule in catalog.RULES:
            if (isinstance(rule, catalog.RouteRule) and rule.models is not None and rule.route is route_enum
                    and (rule.providers is None or provider in rule.providers) and rule.rollout.name not in ALLOWED):
                problems.append(f"{label}, models {sorted(rule.models)}: {rule.rollout.name}")
    rollout = catalog.rollout(catalog.LoggerContext())
    if rollout.name not in ALLOWED:
        problems.append(f"logger: {rollout.name}")
    report(problems, "asked the catalog")


# ---------------------------------------------------------------- --source (read the file)
def _names(node):
    """frozenset({"a", "b"}) -> {"a", "b"}; None -> None."""
    if isinstance(node, ast.Constant) and node.value is None:
        return None
    if (isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and node.func.id == "frozenset"
            and len(node.args) == 1 and isinstance(node.args[0], (ast.Set, ast.List, ast.Tuple))
            and all(isinstance(e, ast.Constant) and isinstance(e.value, str) for e in node.args[0].elts)):
        return {e.value for e in node.args[0].elts}
    raise ValueError(ast.unparse(node))


def _member(node, enum):
    if isinstance(node, ast.Attribute) and isinstance(node.value, ast.Name) and node.value.id == enum:
        return node.attr
    raise ValueError(ast.unparse(node))


def check_source(path):
    tree = ast.parse(open(path, encoding="utf-8").read(), path)
    problems = []
    route_values = {}
    rules = None
    body = None
    for node in tree.body:
        if isinstance(node, ast.ClassDef) and node.name == "Route":
            for item in node.body:
                if (isinstance(item, ast.Assign) and len(item.targets) == 1 and isinstance(item.targets[0], ast.Name)
                        and isinstance(item.value, ast.Constant)):
                    route_values[item.value.value] = item.targets[0].id
        elif isinstance(node, ast.AnnAssign) and isinstance(node.target, ast.Name) and node.target.id == "RULES":
            rules = node.value
        elif isinstance(node, ast.Assign) and any(isinstance(t, ast.Name) and t.id == "RULES" for t in node.targets):
            rules = node.value
        elif isinstance(node, ast.FunctionDef) and node.name == "rollout":
            body = "\n".join(ast.unparse(s) for s in node.body if not (
                isinstance(s, ast.Expr) and isinstance(s.value, ast.Constant)))
    if body != ROLLOUT_BODY:
        problems.append("catalog.rollout() is not the first-match lookup this check knows; read it and update "
                        "packaging/check-litellm-routes.py")
    if not isinstance(rules, ast.Tuple):
        problems.append("RULES is not a plain tuple; update packaging/check-litellm-routes.py")
        report(problems, path)
    parsed = []   # (kind, route member, rollout member, providers, models)
    for call in rules.elts:
        try:
            if not (isinstance(call, ast.Call) and isinstance(call.func, ast.Name)):
                raise ValueError(ast.unparse(call))
            kind = call.func.id
            if kind == "RouteRule":
                args = list(call.args) + [None] * (4 - len(call.args))
                kw = {k.arg: k.value for k in call.keywords}
                route = _member(args[0] or kw["route"], "Route")
                rollout = _member(args[1] or kw["rollout"], "Rollout")
                providers = _names(kw.get("providers", args[2] or ast.Constant(None)))
                models = _names(kw.get("models", args[3] or ast.Constant(None)))
                parsed.append((kind, route, rollout, providers, models))
            elif kind == "LoggerRule":
                parsed.append((kind, None, _member(call.args[0] if call.args else call.keywords[0].value, "Rollout"),
                               None, None))
            elif kind == "SecretManagerRule":
                continue   # Cairn uses no secret manager
            else:
                raise ValueError(kind)
        except (ValueError, KeyError, IndexError) as e:
            problems.append(f"a rule this check doesn't understand: {e}; update packaging/check-litellm-routes.py")
    for label, route, provider in contexts():
        member = route_values.get(route)
        if member is None:
            problems.append(f"{label}: LiteLLM has no route '{route}' any more")
            continue
        first = next((r for k, rt, r, prov, mod in parsed if k == "RouteRule" and rt == member
                      and (prov is None or provider in prov) and mod is None), "PYTHON_ONLY")
        if first not in ALLOWED:
            problems.append(f"{label}: {first}")
        for k, rt, r, prov, mod in parsed:
            if k == "RouteRule" and rt == member and mod is not None and (prov is None or provider in prov) \
                    and r not in ALLOWED:
                problems.append(f"{label}, models {sorted(mod)}: {r}")
    logger = next((r for k, _, r, _, _ in parsed if k == "LoggerRule"), "PYTHON_ONLY")
    if logger not in ALLOWED:
        problems.append(f"logger: {logger}")
    report(problems, "read from catalog.py")


if __name__ == "__main__":
    if sys.argv[1:] == ["--import"]:
        check_import()
    elif len(sys.argv) == 3 and sys.argv[1] == "--source":
        check_source(sys.argv[2])
    else:
        sys.exit(__doc__)
