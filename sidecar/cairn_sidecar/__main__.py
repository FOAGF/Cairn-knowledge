"""Entry point: `python -I -m cairn_sidecar --role ai|embed`."""
import os
import sys


def lower_priority():
    """Low CPU priority for this process: `nice 10` on Linux; Windows has no `os.nice`, so the
    priority class "below normal" is set through the Win32 API. Failing is harmless."""
    try:
        if hasattr(os, "nice"):
            os.nice(10)
        elif sys.platform == "win32":
            import ctypes
            BELOW_NORMAL_PRIORITY_CLASS = 0x00004000
            kernel32 = ctypes.windll.kernel32
            kernel32.GetCurrentProcess.restype = ctypes.c_void_p
            kernel32.SetPriorityClass.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
            kernel32.SetPriorityClass(kernel32.GetCurrentProcess(), BELOW_NORMAL_PRIORITY_CLASS)
    except Exception:  # noqa: BLE001 - only a nicety
        pass


def main():
    role = None
    args = sys.argv[1:]
    if len(args) == 2 and args[0] == "--role":
        role = args[1]
    elif len(args) == 1 and args[0].startswith("--role="):
        role = args[0].split("=", 1)[1]
    if role not in ("ai", "embed"):
        print("usage: python -m cairn_sidecar --role ai|embed", file=sys.stderr)
        return 2

    sys.stdin.reconfigure(encoding="utf-8", errors="replace")
    # newline="\n": on Windows a text-mode stdout would write "\r\n"; the protocol is one "\n"-ended
    # JSON line per message (the app reads either, but keep the bytes identical on every OS).
    sys.stdout.reconfigure(encoding="utf-8", newline="\n")
    sys.stderr.reconfigure(encoding="utf-8", errors="replace")
    os.environ.setdefault("LITELLM_LOCAL_MODEL_COST_MAP", "True")
    os.environ.setdefault("LITELLM_LOG", "ERROR")

    from . import rpc
    methods = rpc.common_methods(role)
    if role == "ai":
        from . import ai
        methods.update(ai.methods())
        secrets_of = ai.secrets_of
    else:
        from . import embed
        lower_priority()  # indexing must not slow the desktop (FR-RAG-IX-5)
        methods.update(embed.methods())
        secrets_of = lambda params: ()  # noqa: E731
    rpc.log(f"{role} started, pid {os.getpid()}")
    rpc.Server(role, methods, secrets_of).serve()


if __name__ == "__main__":
    sys.exit(main())
