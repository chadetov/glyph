#!/usr/bin/env python3
"""Editor-latency measurement for `glyph lsp`, over the real protocol.

What a language server costs is what the person typing waits for, so this
measures the wait and nothing else: a client speaking LSP framing over stdio to
a `glyph lsp` child process, timing from the moment a request leaves the client
to the moment its answer arrives.

Three scenarios, each a thing an editor actually does:

  keystroke   `didChange` carrying one more character, timed until the
              `publishDiagnostics` that carries the matching version. This is
              the squiggle latency, the one a human feels.
  burst       The same `didChange`, immediately followed by hover, definition
              and document symbols over the new text, timed until the last of
              the four answers lands. This is what an editor sends when the
              cursor stops moving.
  references  A workspace-wide find-references, repeated over a buffer nobody
              has touched. Nothing changed between the two asks, so the second
              one measures how much a server re-derives for no reason.

The keystroke and burst sweep three files at 535, 1,652 and 2,205 lines so the
growth in file size is visible and not just the cost at one size. References
runs against `examples/apps/csvql`, eleven files, because a workspace-wide
query over a single file measures nothing about a workspace.

The edit is a comment line inserted inside the last declaration's body rather
than appended past the end of the file. Both are real keystrokes, but a
trailing edit changes no declaration, so a per-declaration memo layer reuses
everything and reports a latency no user will ever see. This one changes
exactly one declaration, which is what typing does.

Usage:

    python3 benchmarks/lsp-latency/measure.py --binary glyph-compiler/target/release/glyph
    python3 benchmarks/lsp-latency/measure.py --binary /path/to/other/glyph --label baseline

Writes `benchmarks/results/lsp-latency-<timestamp>.json` and prints a table.
Run it twice, once per binary, in one sitting: a wall-clock number from another
session on another machine load is not comparable to this one, and comparing
across sessions is how a regression gets read as an improvement.
"""

import argparse
import json
import os
import queue
import statistics
import subprocess
import sys
import threading
import time
from datetime import datetime, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]

# The sweep: the three files the lane has always measured, smallest first.
SWEEP = [
    "examples/corpus/json_parser.glyph",
    "examples/apps/sheet/main.glyph",
    "examples/apps/minilang/main.glyph",
]

# The workspace the references scenario ranges over, and the symbol it asks
# about: `render.fail_lines` is defined in one file and called from another, so
# the answer has to leave the open document to be right.
REFERENCES_ROOT = "examples/apps/csvql"
REFERENCES_FILE = "examples/apps/csvql/main.glyph"
REFERENCES_NEEDLE = "render.fail_lines("
REFERENCES_SYMBOL = "fail_lines"

TIMEOUT = 30.0


class Lsp:
    """A client speaking real LSP framing over stdio to a server process."""

    def __init__(self, binary, cwd):
        self.proc = subprocess.Popen(
            [str(binary), "lsp"],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            cwd=str(cwd),
        )
        self._next_id = 0
        self._lock = threading.Condition()
        self._responses = {}
        self._diagnostics = {}
        self._dead = None
        self._reader = threading.Thread(target=self._read_loop, daemon=True)
        self._reader.start()

    # -- wire ------------------------------------------------------------

    def _read_loop(self):
        out = self.proc.stdout
        try:
            while True:
                length = None
                while True:
                    line = out.readline()
                    if not line:
                        raise EOFError("server closed stdout")
                    line = line.strip()
                    if not line:
                        break
                    if line.lower().startswith(b"content-length:"):
                        length = int(line.split(b":", 1)[1])
                if length is None:
                    raise EOFError("header block with no Content-Length")
                body = b""
                while len(body) < length:
                    chunk = out.read(length - len(body))
                    if not chunk:
                        raise EOFError("truncated body")
                    body += chunk
                self._dispatch(json.loads(body))
        except Exception as exc:  # the server died, or spoke nonsense
            with self._lock:
                self._dead = exc
                self._lock.notify_all()

    def _dispatch(self, msg):
        with self._lock:
            if "id" in msg and ("result" in msg or "error" in msg):
                self._responses[msg["id"]] = msg
            elif msg.get("method") == "textDocument/publishDiagnostics":
                params = msg["params"]
                key = (params["uri"], params.get("version"))
                self._diagnostics[key] = params
            self._lock.notify_all()

    def _send(self, msg):
        body = json.dumps(msg).encode()
        header = b"Content-Length: %d\r\n\r\n" % len(body)
        self.proc.stdin.write(header + body)
        self.proc.stdin.flush()

    def notify(self, method, params):
        self._send({"jsonrpc": "2.0", "method": method, "params": params})

    def request(self, method, params):
        self._next_id += 1
        rid = self._next_id
        self._send({"jsonrpc": "2.0", "id": rid, "method": method, "params": params})
        return rid

    # -- waiting ---------------------------------------------------------

    def _wait(self, ready, what):
        deadline = time.monotonic() + TIMEOUT
        with self._lock:
            while True:
                got = ready()
                if got is not None:
                    return got
                if self._dead is not None:
                    raise RuntimeError(f"server died waiting for {what}: {self._dead}")
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TimeoutError(f"timed out waiting for {what}")
                self._lock.wait(remaining)

    def await_response(self, rid):
        return self._wait(lambda: self._responses.pop(rid, None), f"response {rid}")

    def await_diagnostics(self, uri, version):
        key = (uri, version)
        return self._wait(
            lambda: self._diagnostics.pop(key, None),
            f"diagnostics for {uri}@{version}",
        )

    # -- lifecycle -------------------------------------------------------

    def initialize(self, root):
        uri = Path(root).resolve().as_uri()
        rid = self.request(
            "initialize",
            {
                "processId": os.getpid(),
                "rootUri": uri,
                "workspaceFolders": [{"uri": uri, "name": Path(root).name}],
                "capabilities": {},
            },
        )
        self.await_response(rid)
        self.notify("initialized", {})

    def shutdown(self):
        try:
            self.await_response(self.request("shutdown", None))
            self.notify("exit", None)
        except Exception:
            pass
        try:
            self.proc.wait(timeout=5)
        except Exception:
            self.proc.kill()


# -- the edit, and where to point ----------------------------------------


def mutate(base, n):
    """`base` with a comment line inserted inside the last declaration's body.

    Every sweep file ends with a `}` in column zero closing the last top-level
    declaration, so the line before it is inside that body. The text differs on
    every `n`, so no write is a no-op and no measurement is of a cache hit the
    editor would not have had.
    """
    lines = base.split("\n")
    for i in range(len(lines) - 1, -1, -1):
        if lines[i].startswith("}"):
            return "\n".join(lines[:i] + [f"  // bench {n}"] + lines[i:])
    raise SystemExit("no top-level closing brace found; the edit point is wrong")


def line_col(text, offset):
    """LSP line/character for a byte offset, over source with no astral text."""
    head = text[:offset]
    line = head.count("\n")
    return line, offset - (head.rfind("\n") + 1)


def call_site(text):
    """A position on an identifier that is used, not merely declared.

    Hover, definition and references all want a name in expression position.
    The first `fn NAME` whose name appears again somewhere else in the file is
    a call site, and every sweep file has one.
    """
    for raw in text.split("\n"):
        stripped = raw.strip()
        for prefix in ("fn ", "pub fn "):
            if not stripped.startswith(prefix):
                continue
            name = stripped[len(prefix):].split("(")[0].split("<")[0].strip()
            if not name:
                continue
            decl = text.find(stripped)
            probe = 0
            while True:
                at = text.find(name, probe)
                if at < 0:
                    break
                if not (decl <= at < decl + len(stripped)):
                    before = text[at - 1] if at else " "
                    after = text[at + len(name):at + len(name) + 1]
                    if not before.isalnum() and before not in "_." and after == "(":
                        return line_col(text, at)
                probe = at + 1
    raise SystemExit("no call site found; the sweep file is not what was expected")


# -- scenarios ------------------------------------------------------------


def doc(uri, text, version):
    return {"textDocument": {"uri": uri, "languageId": "glyph", "version": version, "text": text}}


def at(uri, pos):
    return {"textDocument": {"uri": uri}, "position": {"line": pos[0], "character": pos[1]}}


def run_keystroke(lsp, uri, base, pos, count, burst, start):
    """`count` edits from `start`, each timed until its answers land.

    `start` carries across the warm-up so the measured edits are texts the
    server has never seen and versions it has never been sent. Replaying the
    warm-up's edits would measure the same keystroke twice and call the second
    one a fresh sample.
    """
    samples = []
    for i in range(start, start + count):
        version = 2 + i
        text = mutate(base, i)
        started = time.perf_counter()
        lsp.notify(
            "textDocument/didChange",
            {
                "textDocument": {"uri": uri, "version": version},
                "contentChanges": [{"text": text}],
            },
        )
        pending = []
        if burst:
            pending.append(lsp.request("textDocument/hover", at(uri, pos)))
            pending.append(lsp.request("textDocument/definition", at(uri, pos)))
            pending.append(
                lsp.request("textDocument/documentSymbol", {"textDocument": {"uri": uri}})
            )
        lsp.await_diagnostics(uri, version)
        for rid in pending:
            lsp.await_response(rid)
        samples.append((time.perf_counter() - started) * 1000.0)
    return samples


def run_references(lsp, uri, pos, count):
    """`count` workspace-wide find-references over a buffer nobody touched."""
    samples = []
    files = 0
    for _ in range(count):
        started = time.perf_counter()
        rid = lsp.request(
            "textDocument/references",
            {**at(uri, pos), "context": {"includeDeclaration": True}},
        )
        result = lsp.await_response(rid).get("result") or []
        samples.append((time.perf_counter() - started) * 1000.0)
        files = len({loc["uri"] for loc in result})
    if files < 2:
        raise SystemExit(
            f"references answered from {files} file(s); this must be the "
            "workspace-wide path or it measures the wrong thing"
        )
    return samples


# -- driving --------------------------------------------------------------


def split(total, parts):
    """`total` samples over `parts` processes, as evenly as they divide."""
    base, extra = divmod(total, parts)
    return [base + (1 if i < extra else 0) for i in range(parts)]


def summarize(samples):
    ordered = sorted(samples)
    return {
        "n": len(ordered),
        "median_ms": round(statistics.median(ordered), 2),
        "mean_ms": round(statistics.fmean(ordered), 2),
        "p95_ms": round(ordered[min(len(ordered) - 1, int(len(ordered) * 0.95))], 2),
        "min_ms": round(ordered[0], 2),
        "max_ms": round(ordered[-1], 2),
    }


def measure_file(binary, rel, warmup, counts, scenario):
    path = REPO / rel
    base = path.read_text()
    uri = path.as_uri()
    # The edit goes in near the end of the file and the call site is the first
    # one in it, so this position stays valid in every mutated text.
    pos = call_site(base)
    samples = []
    for count in counts:
        lsp = Lsp(binary, REPO)
        try:
            lsp.initialize(REPO)
            lsp.notify("textDocument/didOpen", doc(uri, base, 1))
            lsp.await_diagnostics(uri, 1)
            run_keystroke(lsp, uri, base, pos, warmup, scenario == "burst", 0)
            samples += run_keystroke(
                lsp, uri, base, pos, count, scenario == "burst", warmup
            )
        finally:
            lsp.shutdown()
    return samples


def measure_references(binary, warmup, counts):
    root = REPO / REFERENCES_ROOT
    path = REPO / REFERENCES_FILE
    text = path.read_text()
    uri = path.as_uri()
    needle = text.find(REFERENCES_NEEDLE)
    if needle < 0:
        raise SystemExit(f"{REFERENCES_NEEDLE!r} is gone from {REFERENCES_FILE}")
    pos = line_col(text, needle + REFERENCES_NEEDLE.index(REFERENCES_SYMBOL))
    samples = []
    for count in counts:
        lsp = Lsp(binary, root)
        try:
            lsp.initialize(root)
            lsp.notify("textDocument/didOpen", doc(uri, text, 1))
            lsp.await_diagnostics(uri, 1)
            run_references(lsp, uri, pos, warmup)
            samples += run_references(lsp, uri, pos, count)
        finally:
            lsp.shutdown()
    return samples


def git_commit():
    try:
        out = subprocess.run(
            ["git", "-C", str(REPO), "rev-parse", "--short", "HEAD"],
            capture_output=True,
            text=True,
            check=True,
        )
        return out.stdout.strip()
    except Exception:
        return None


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--binary", default="glyph-compiler/target/release/glyph")
    ap.add_argument("--label", default="current", help="what this binary is, for the record")
    ap.add_argument("--commit", default=None, help="the commit it was built from")
    ap.add_argument("--processes", type=int, default=3)
    ap.add_argument("--warmup", type=int, default=10)
    ap.add_argument("--measured", type=int, default=100)
    ap.add_argument(
        "--scenario",
        action="append",
        choices=["keystroke", "burst", "references"],
        help="repeatable; default is all three",
    )
    ap.add_argument("--out", default=None)
    args = ap.parse_args()

    binary = Path(args.binary)
    if not binary.is_absolute():
        binary = (REPO / binary).resolve()
    if not binary.exists():
        raise SystemExit(f"no binary at {binary}")
    scenarios = args.scenario or ["keystroke", "burst", "references"]
    counts = split(args.measured, args.processes)

    version = subprocess.run(
        [str(binary), "--version"], capture_output=True, text=True
    ).stdout.strip()
    print(f"{args.label}: {binary} ({version})")
    print(f"{args.processes} processes, {args.warmup} warm-up, {args.measured} measured\n")

    record = {
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "label": args.label,
        "binary": str(binary),
        "binary_version": version,
        "commit": args.commit or git_commit(),
        "processes": args.processes,
        "warmup": args.warmup,
        "measured": args.measured,
        "scenarios": {},
    }

    width = max(len(r) for r in SWEEP) + 2
    for scenario in scenarios:
        record["scenarios"][scenario] = {}
        if scenario == "references":
            samples = measure_references(binary, args.warmup, counts)
            stats = summarize(samples)
            record["scenarios"][scenario][REFERENCES_ROOT] = stats
            print(f"references  {REFERENCES_ROOT:<{width}} {stats['median_ms']:>8.2f} ms "
                  f"(mean {stats['mean_ms']:.2f}, p95 {stats['p95_ms']:.2f}, n={stats['n']})")
            continue
        for rel in SWEEP:
            samples = measure_file(binary, rel, args.warmup, counts, scenario)
            stats = summarize(samples)
            stats["lines"] = (REPO / rel).read_text().count("\n") + 1
            record["scenarios"][scenario][rel] = stats
            print(f"{scenario:<11} {rel:<{width}} {stats['median_ms']:>8.2f} ms "
                  f"(mean {stats['mean_ms']:.2f}, p95 {stats['p95_ms']:.2f}, n={stats['n']})")
        print()

    out = args.out
    if out is None:
        stamp = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H-%M-%SZ")
        out = REPO / "benchmarks" / "results" / f"lsp-latency-{args.label}-{stamp}.json"
    out = Path(out)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(record, indent=2) + "\n")
    print(f"wrote {out}")


if __name__ == "__main__":
    sys.exit(main())
