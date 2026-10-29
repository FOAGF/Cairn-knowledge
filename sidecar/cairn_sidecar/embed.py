"""Role `embed`: EmbeddingGemma (q4 ONNX) on raw onnxruntime + tokenizers (M0-b).

onnxruntime, tokenizers and numpy are imported when the model is first loaded.
"""
import base64
import os
import threading
import time

from .errors import InvalidParams, SidecarError

QUERY_PROMPT = "task: search result | query: "
DOC_PROMPT = "title: {title} | text: {text}"
MAX_TOKENS = 2048
FULL_DIMS = 768
MODEL_NAME = "embeddinggemma-300m-q4"
FILES = ("onnx/model_q4.onnx", "onnx/model_q4.onnx_data", "tokenizer.json")
BATCH = 8  # documents per ONNX run (the app already sends at most 8)

# priorities on the worker queue: lowest first
PRIO_QUERY, PRIO_INFO, PRIO_DOCS = 0, 1, 2

_models = {}  # model_dir -> Embedder (kept for the life of the process)
_lock = threading.Lock()


class Embedder:
    def __init__(self, model_dir, threads):
        missing = [f for f in FILES if not os.path.isfile(os.path.join(model_dir, f))]
        if missing:
            raise SidecarError("model_missing", f"Embedding model files not found in {model_dir}: {', '.join(missing)}",
                               hint="The model files are part of Cairn's install; reinstall Cairn, or check the model folder.")
        t0 = time.time()
        import numpy as np
        import onnxruntime as ort
        from tokenizers import Tokenizer
        self.np = np
        so = ort.SessionOptions()
        so.intra_op_num_threads = max(1, int(threads))
        so.inter_op_num_threads = 1
        self.session = ort.InferenceSession(os.path.join(model_dir, FILES[0]), so, providers=["CPUExecutionProvider"])
        self.tok = Tokenizer.from_file(os.path.join(model_dir, "tokenizer.json"))
        self.tok.enable_truncation(MAX_TOKENS)
        self.tok.enable_padding(pad_id=0, pad_token="<pad>")
        self.load_ms = int((time.time() - t0) * 1000)

    def _run(self, texts, dims):
        np = self.np
        enc = self.tok.encode_batch(texts)
        ids = np.array([e.ids for e in enc], dtype=np.int64)
        mask = np.array([e.attention_mask for e in enc], dtype=np.int64)
        out = self.session.run(["sentence_embedding"], {"input_ids": ids, "attention_mask": mask})[0]
        out = out[:, :dims]  # Matryoshka truncation, then re-normalise
        out = out / np.linalg.norm(out, axis=1, keepdims=True)
        return out.astype("<f4")

    def embed(self, texts, dims):
        """Vectors as base64 little-endian float32, in the order of `texts`."""
        order = sorted(range(len(texts)), key=lambda i: len(texts[i]))  # similar lengths: less padding
        result = [None] * len(texts)
        for s in range(0, len(order), BATCH):
            idx = order[s:s + BATCH]
            vecs = self._run([texts[i] for i in idx], dims)
            for i, v in zip(idx, vecs):
                result[i] = base64.b64encode(v.tobytes()).decode()
        return result


def _params(p):
    model_dir = p.get("model_dir")
    if not isinstance(model_dir, str) or not model_dir:
        raise InvalidParams("model_dir is required")
    dims = p.get("dims", 256)
    if not isinstance(dims, int) or not 1 <= dims <= FULL_DIMS:
        raise InvalidParams(f"dims must be a whole number from 1 to {FULL_DIMS}")
    return model_dir, dims, p.get("threads") or 4


def get_model(model_dir, threads):
    """The session is cached per model_dir; `threads` only matters for the first load."""
    with _lock:
        m = _models.get(model_dir)
        if m is None:
            m = _models[model_dir] = Embedder(model_dir, threads)
        return m


def embed_query(req):
    model_dir, dims, threads = _params(req.params)
    text = req.params.get("text")
    if not isinstance(text, str):
        raise InvalidParams("text must be a string")
    return {"vector": get_model(model_dir, threads).embed([QUERY_PROMPT + text], dims)[0]}


def embed_documents(req):
    model_dir, dims, threads = _params(req.params)
    items = req.params.get("items")
    if not isinstance(items, list) or not all(isinstance(i, dict) for i in items):
        raise InvalidParams("items must be a list of {title, text}")
    texts = [DOC_PROMPT.format(title=i.get("title") or "none", text=i.get("text") or "") for i in items]
    return {"vectors": get_model(model_dir, threads).embed(texts, dims) if texts else []}


def embed_info(req):
    model_dir, dims, threads = _params(req.params)
    m = get_model(model_dir, threads)
    return {"loaded": True, "model": MODEL_NAME, "dims": dims, "load_ms": m.load_ms}


def methods():
    return {
        "embed.query": (embed_query, PRIO_QUERY),
        "embed.documents": (embed_documents, PRIO_DOCS),
        "embed.info": (embed_info, PRIO_INFO),
    }
