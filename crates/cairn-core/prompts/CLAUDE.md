# Prompt templates

- The format and the checks are in [README.md](README.md).
- Never write or rewrite a template yourself: delegate it to an Opus sub-agent at max effort (model `opus`, effort `max`), which also adds judges for it to `poc/h-create/llm_check.py` or `poc/i-assistant/llm_check.py`.
- After any template change, run `sidecar/.venv/bin/python poc/h-create/golden.py`; the Rust golden test must pass.
- Only the maintainer runs the live checks on real models. Never use real endpoints, API keys or `poc/a-llm/config.toml`.
