"""`model.download` (role `ai`; FR-RAG-EM-3, D-82): downloads the search model's pinned files.

The lite packages leave the model out; the app downloads it here, because this worker already has
an HTTP client that uses the same proxy settings and certificates as the LLM calls. The app passes
each file's URL, size and SHA-256 (from `packaging/model-pins.sh`) and the folder to write to.

- **Resuming:** a finished file is kept as `<path>`; one being written is `<path>.part`. A second
  call (a new start, or *Try again*) keeps finished files, hashes the part already on disk and
  asks for the rest with an HTTP range request; a server that answers with the whole file instead
  is read from the start.
- **Checks:** never more than the pinned size is written; a file that ends up with the wrong size
  or hash is deleted and the call fails with `model_check_failed`. The app checks the files again
  when it puts them in place.
- **Progress** goes out as `stream` notifications whose `delta` is JSON text,
  `{"done": <bytes>, "total": <bytes>}`, at most about four times a second.
- **Cancel** stops between two reads; the files stay for the next call.
"""
import hashlib
import json
import os
import re
import time

from .errors import InvalidParams, SidecarError

CHUNK = 1 << 20
TIMEOUT = 60  # seconds without data before a read fails
PROGRESS_EVERY = 0.25
SHA256_RE = re.compile(r"^[0-9a-f]{64}$")


def _files(params):
    dest = params.get("dest_dir")
    if not isinstance(dest, str) or not dest:
        raise InvalidParams("dest_dir is required")
    files = params.get("files")
    if not isinstance(files, list) or not files:
        raise InvalidParams("files must be a non-empty list")
    out = []
    for f in files:
        if not isinstance(f, dict):
            raise InvalidParams("each file must be an object")
        url, path, size, sha = f.get("url"), f.get("path"), f.get("size"), f.get("sha256")
        if not isinstance(url, str) or not url.startswith(("https://", "http://")):
            raise InvalidParams("file.url must be an http(s) address")
        parts = path.split("/") if isinstance(path, str) else []
        if not parts or any(p in ("", ".", "..") for p in parts) or "\\" in path or ":" in path:
            raise InvalidParams("file.path must be a relative path")
        if not isinstance(size, int) or isinstance(size, bool) or size <= 0:
            raise InvalidParams("file.size must be a positive whole number")
        if not isinstance(sha, str) or not SHA256_RE.match(sha.lower()):
            raise InvalidParams("file.sha256 must be 64 hex digits")
        out.append({"url": url, "path": path, "size": size, "sha256": sha.lower(),
                    "target": os.path.join(dest, *parts)})
    return out


def _name(f):
    return f["path"].rsplit("/", 1)[-1]


def _failed(detail):
    return SidecarError("network", f"The search model couldn't be downloaded: {detail}",
                        hint="Check the network connection, then try again; finished files are kept.")


def model_download(req):
    files = _files(req.params)
    total = sum(f["size"] for f in files)
    progress = {"done": 0, "sent": 0.0}

    def report(force=False):
        now = time.monotonic()
        if force or now - progress["sent"] >= PROGRESS_EVERY:
            progress["sent"] = now
            req.stream(json.dumps({"done": progress["done"], "total": total}))

    import httpx

    timeout = httpx.Timeout(TIMEOUT, connect=30)
    with httpx.Client(follow_redirects=True, timeout=timeout) as client:
        for f in files:
            if req.cancelled.is_set():
                return {"cancelled": True}
            target, part = f["target"], f["target"] + ".part"
            os.makedirs(os.path.dirname(target), exist_ok=True)
            if os.path.isfile(target) and os.path.getsize(target) == f["size"]:
                progress["done"] += f["size"]  # finished (and checked) by an earlier call
                report(force=True)
                continue
            try:
                ok = _fetch(client, req, f, part, progress, report)
            except SidecarError:
                raise
            except httpx.HTTPError as e:
                raise _failed(" ".join(str(e).split())[:300] or type(e).__name__) from e
            if not ok:
                return {"cancelled": True}
            os.replace(part, target)
    report(force=True)
    return {"done": total, "total": total}


def _fetch(client, req, f, part, progress, report):
    """Writes `f` to `part`, resuming what is there. Returns False when cancelled."""
    size, digest, have = f["size"], hashlib.sha256(), 0
    if os.path.isfile(part):
        have = os.path.getsize(part)
        if have > size:
            os.remove(part)
            have = 0
        else:
            with open(part, "rb") as fh:
                for block in iter(lambda: fh.read(CHUNK), b""):
                    digest.update(block)
    progress["done"] += have
    if have < size:
        headers = {"Range": f"bytes={have}-"} if have else {}
        with client.stream("GET", f["url"], headers=headers) as r:
            if have and r.status_code == 206:
                mode = "ab"
            elif r.status_code == 200:
                # No range support (or nothing on disk yet): from the start.
                progress["done"] -= have
                have, digest, mode = 0, hashlib.sha256(), "wb"
            else:
                raise _failed(f"the server answered {r.status_code} for {_name(f)}")
            with open(part, mode) as out:
                for chunk in r.iter_bytes(CHUNK):
                    if req.cancelled.is_set():
                        return False
                    if have + len(chunk) > size:
                        out.close()
                        os.remove(part)
                        raise SidecarError("model_check_failed",
                                           f"{_name(f)} is longer than the expected file; it was deleted.")
                    out.write(chunk)
                    digest.update(chunk)
                    have += len(chunk)
                    progress["done"] += len(chunk)
                    report()
    if have != size:
        raise _failed(f"{_name(f)} ended after {have} of {size} bytes")
    if digest.hexdigest() != f["sha256"]:
        os.remove(part)
        raise SidecarError("model_check_failed",
                           f"{_name(f)} doesn't match the expected file; it was deleted. Try the download again.")
    return True


def methods():
    return {"model.download": (model_download, None)}
