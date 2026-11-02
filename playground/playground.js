// The Glyph playground. Loads the WASM compiler, compiles the editor's contents
// to TypeScript on every (debounced) keystroke, renders diagnostics, and shows
// a fixed "agent edit" diff that makes the diff-stability pillar legible.
//
// The example picker comes from examples.js, which playground/build.sh
// generates from catches/ and playground/examples/. A program is shared in the
// URL fragment, which the browser never sends to a server.

import init, { compile, version } from "./pkg/glyph_wasm.js";
import { DEFAULT_EXAMPLE, EXAMPLES, TSC_ONLY } from "./examples.js";

const bySlug = new Map(EXAMPLES.map((e) => [e.slug, e]));

// The agent-edit demo: one line of Glyph changes (the Pro per-seat price), and
// the emitted TypeScript changes exactly one line in turn.
const EDIT_BEFORE = bySlug.get("pricing").source;
const EDIT_AFTER = EDIT_BEFORE.replace(
  "Pro({ seats }) => Ok(seats * 12),",
  "Pro({ seats }) => Ok(seats * 10),"
);

const $ = (id) => document.getElementById(id);

function escapeHtml(s) {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

// A minimal LCS line diff -> ops [{ type: "same" | "add" | "del", text }].
function diffLines(aText, bText) {
  const a = aText.replace(/\n$/, "").split("\n");
  const b = bText.replace(/\n$/, "").split("\n");
  const n = a.length;
  const m = b.length;
  const lcs = Array.from({ length: n + 1 }, () => new Array(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      lcs[i][j] =
        a[i] === b[j]
          ? lcs[i + 1][j + 1] + 1
          : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
    }
  }
  const ops = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      ops.push({ type: "same", text: a[i] });
      i++;
      j++;
    } else if (lcs[i + 1][j] >= lcs[i][j + 1]) {
      ops.push({ type: "del", text: a[i] });
      i++;
    } else {
      ops.push({ type: "add", text: b[j] });
      j++;
    }
  }
  while (i < n) ops.push({ type: "del", text: a[i++] });
  while (j < m) ops.push({ type: "add", text: b[j++] });
  return ops;
}

function changeCounts(ops) {
  return {
    del: ops.filter((o) => o.type === "del").length,
    add: ops.filter((o) => o.type === "add").length,
  };
}

function fmtCounts(c) {
  return `&minus;${c.del} / +${c.add}`;
}

function renderDiff(ops) {
  const prefix = { same: "  ", add: "+ ", del: "- " };
  return ops
    .map(
      (o) =>
        `<span class="diff-line diff-${o.type}">${escapeHtml(
          prefix[o.type] + o.text
        )}</span>`
    )
    // Each line is a block-level span already; a newline between them inside
    // the <pre> would add a blank line after every one.
    .join("");
}

// The diagnostics currently shown, so the gutter and click handlers can map a
// line number or a clicked row back to a source span.
let currentDiags = [];

// Map a (0-based line, UTF-16 column) to an index into the textarea's value.
// The textarea uses UTF-16 code units, matching the compiler's column units.
function offsetOf(src, line, col) {
  const lines = src.split("\n");
  let off = 0;
  for (let i = 0; i < line && i < lines.length; i++) off += lines[i].length + 1;
  return off + col;
}

// Select a diagnostic's span in the editor and scroll it into view, so a click
// on the message (or its gutter line) shows exactly where the error is.
function jumpToDiag(d) {
  const ta = $("source");
  const src = ta.value;
  const start = offsetOf(src, d.start_line, d.start_col);
  const end = Math.max(offsetOf(src, d.end_line, d.end_col), start + 1);
  ta.focus();
  ta.setSelectionRange(start, end);
  const lineHeight = parseFloat(getComputedStyle(ta).lineHeight) || 20;
  ta.scrollTop = Math.max(0, (d.start_line - 3) * lineHeight);
  syncGutterScroll();
}

function renderDiagnostics(diags) {
  currentDiags = diags;
  const el = $("diagnostics");
  if (diags.length === 0) {
    el.innerHTML = `<div class="diag-ok">✓ no diagnostics</div>`;
    markErrorLines();
    return;
  }
  el.innerHTML = diags
    .map((d, i) => {
      const loc = `${d.start_line + 1}:${d.start_col + 1}`;
      // A lint and a must-use warning are warnings in `glyph check`, so they are
      // warnings here. Painting them red would tell you a program is broken
      // when the compiler will build it.
      const kind = d.severity === "warning" ? "warn" : "err";
      return `<div class="diag ${kind}" data-diag="${i}" role="button" tabindex="0" title="Jump to line ${
        d.start_line + 1
      }"><span class="code">${escapeHtml(
        d.code
      )}</span><span class="loc">${loc}</span><span class="msg">${escapeHtml(
        d.message
      )}</span></div>`;
    })
    .join("");
  markErrorLines();
}

// Say which imports this page could not check against a project, and what
// `glyph build` would do differently with them. Rendered only when the compiler
// names one: a module that imports nothing outside `std/*` compiles here to
// what `glyph build` writes, and there is nothing to say about it.
function renderProjectNote(paths) {
  const el = $("project-note");
  if (!paths || paths.length === 0) {
    el.hidden = true;
    el.innerHTML = "";
    return;
  }
  const coded = paths.map((p) => `<code>${escapeHtml(p)}</code>`);
  const names =
    coded.length > 1
      ? coded.slice(0, -1).join(", ") + " or " + coded[coded.length - 1]
      : coded[0];
  const one = paths.length === 1;
  el.innerHTML =
    `This page is a single-module demonstration: it compiles the one module in ` +
    `the editor and has no ${names}, so ` +
    `${one ? "that import was" : "those imports were"} compiled as ` +
    `${one ? "an npm package" : "npm packages"}. If ` +
    `${one ? "it is a module" : "any of them is a module"} in your own project, ` +
    `<code>glyph build</code> writes something different below: a relative ` +
    `specifier, and the real runtime check for any type ` +
    `${one ? "that module declares" : "those modules declare"} where this page ` +
    `falls back to a presence check.`;
  el.hidden = false;
}

// Keep the gutter's line count in step with the source and its scroll position
// locked to the textarea's.
function renderGutter() {
  const ta = $("source");
  const g = $("gutter");
  const count = ta.value.split("\n").length;
  if (g.childElementCount !== count) {
    let html = "";
    for (let i = 1; i <= count; i++) html += `<div class="ln" data-line="${i}">${i}</div>`;
    g.innerHTML = html;
    markErrorLines();
  }
  syncGutterScroll();
}

function syncGutterScroll() {
  $("gutter").scrollTop = $("source").scrollTop;
}

// Paint the gutter row where each diagnostic starts (and make it clickable),
// red for an error and amber for a warning. Only the start line is marked: some
// spans (an unterminated string, say) run to the end of the file, and painting
// every line in between would be noise. A line carrying both takes the red.
function markErrorLines() {
  const g = $("gutter");
  const errs = new Set(
    currentDiags.filter((d) => d.severity !== "warning").map((d) => d.start_line + 1)
  );
  const warns = new Set(
    currentDiags.filter((d) => d.severity === "warning").map((d) => d.start_line + 1)
  );
  for (const ln of g.children) {
    const n = Number(ln.dataset.line);
    ln.classList.toggle("err", errs.has(n));
    ln.classList.toggle("warn", !errs.has(n) && warns.has(n));
  }
}

function compileToView() {
  const src = $("source").value;
  let out;
  try {
    out = JSON.parse(compile(src));
  } catch (e) {
    $("ts").querySelector("code").textContent = `// playground error: ${e}`;
    // Clear the note too: it describes output that is no longer on screen.
    renderProjectNote([]);
    return;
  }
  $("ts").querySelector("code").textContent =
    out.ts != null ? out.ts : "// (no output: fix the errors on the left)";
  renderProjectNote(out.assumed_external_imports);
  renderDiagnostics(out.diagnostics || []);
  renderGutter();
}

function renderAgentEditDemo() {
  // Diff the Glyph source the agent edited.
  const glyphOps = diffLines(EDIT_BEFORE, EDIT_AFTER);
  $("diff").innerHTML = renderDiff(glyphOps);

  // Compile both sides and diff the emitted TypeScript to show the change
  // propagates to just as few lines.
  let tsCounts = null;
  try {
    const before = JSON.parse(compile(EDIT_BEFORE));
    const after = JSON.parse(compile(EDIT_AFTER));
    if (before.ts != null && after.ts != null) {
      tsCounts = changeCounts(diffLines(before.ts, after.ts));
    }
  } catch (_) {
    // leave tsCounts null
  }

  const glyphCounts = changeCounts(glyphOps);
  const tsPart =
    tsCounts != null
      ? ` The compiled TypeScript changes just as little: <strong>${fmtCounts(
          tsCounts
        )}</strong>.`
      : "";
  $("diff-note").innerHTML =
    `The Glyph diff is <strong>${fmtCounts(glyphCounts)}</strong>.` +
    tsPart +
    ` Fixed-width, no-reflow formatting keeps a one-line change a one-line diff, so the review stays small.`;
}

// Inline `code` spans in a case's title or note, which are written with
// backticks in case.toml.
function inlineCode(s) {
  return escapeHtml(s).replace(/`([^`]+)`/g, "<code>$1</code>");
}

// ---- The example picker -------------------------------------------------

// The slug of the example in the editor, or null once the source is something
// else (an edit, or a program loaded from a shared link).
let currentSlug = null;

function renderPicker() {
  const sel = $("example");
  const group = (label, kind) =>
    `<optgroup label="${label}">` +
    EXAMPLES.filter((e) => e.kind === kind)
      .map((e) => `<option value="${e.slug}">${escapeHtml(e.label)}</option>`)
      .join("") +
    `</optgroup>`;
  sel.innerHTML =
    `<option value="" hidden>Your program</option>` +
    group("Refused by Glyph, accepted by tsc --strict", "catch") +
    group("Compiles", "program");
}

function renderCaption() {
  const el = $("example-caption");
  const e = currentSlug != null ? bySlug.get(currentSlug) : null;
  if (e == null) {
    el.innerHTML = "";
    el.hidden = true;
    return;
  }
  let html = inlineCode(e.title) + (/[.!?]$/.test(e.title) ? "" : ".");
  if (e.kind === "catch") {
    html +=
      ` Glyph refuses it with <code>${escapeHtml(e.code)}</code>. ` +
      `<a href="${e.ts_url}" target="_blank" rel="noopener">The same program in TypeScript</a> ` +
      `passes <code>tsc --strict</code>.`;
    if (e.note) html += " " + inlineCode(e.note);
    if (TSC_ONLY.length > 0) {
      const names = TSC_ONLY.map(
        (t) => `<code>${escapeHtml(t.slug)}</code> (${escapeHtml(t.code)})`
      ).join(", ");
      html +=
        ` <span class="muted">Not in this list: ${names}, refused by the ` +
        `<code>tsc</code> pass <code>glyph build</code> runs, which this page does not.</span>`;
    }
  }
  el.innerHTML = html;
  el.hidden = false;
}

function showMessage(text) {
  const el = $("example-caption");
  el.textContent = text;
  el.hidden = false;
}

function loadSource(src, slug) {
  const ta = $("source");
  ta.value = src;
  ta.scrollTop = 0;
  currentSlug = slug;
  $("example").value = slug ?? "";
  renderCaption();
  renderGutter();
  compileToView();
}

function loadExample(slug) {
  const e = bySlug.get(slug) ?? bySlug.get(DEFAULT_EXAMPLE);
  loadSource(e.source, e.slug);
}

// ---- Sharing by URL -------------------------------------------------------
//
// Three fragment forms, all read on load:
//   #example=<slug>   one of the picker's programs, unedited
//   #z=<base64url>    the source as UTF-8, deflate-raw compressed
//   #code=<base64url> the source as UTF-8, uncompressed
// `z` is written only when the browser has CompressionStream and it comes out
// shorter; `code` is the fallback every browser can read.

function toBase64Url(bytes) {
  let bin = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    bin += String.fromCharCode.apply(null, bytes.subarray(i, i + 0x8000));
  }
  return btoa(bin).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromBase64Url(text) {
  const b64 = text.replace(/-/g, "+").replace(/_/g, "/");
  const bin = atob(b64 + "=".repeat((4 - (b64.length % 4)) % 4));
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return bytes;
}

async function pipeBytes(bytes, stream) {
  const out = new Blob([bytes]).stream().pipeThrough(stream);
  return new Uint8Array(await new Response(out).arrayBuffer());
}

async function encodeFragment(src) {
  if (currentSlug != null && bySlug.get(currentSlug).source === src) {
    return `example=${currentSlug}`;
  }
  const raw = new TextEncoder().encode(src);
  const plain = `code=${toBase64Url(raw)}`;
  if (typeof CompressionStream === "undefined") return plain;
  try {
    const packed = `z=${toBase64Url(await pipeBytes(raw, new CompressionStream("deflate-raw")))}`;
    return packed.length < plain.length ? packed : plain;
  } catch (_) {
    return plain;
  }
}

// Load whatever the fragment names. Returns false when it names nothing.
async function loadFromFragment() {
  const params = new URLSearchParams(location.hash.slice(1));
  try {
    if (params.has("example")) {
      const slug = params.get("example");
      if (!bySlug.has(slug)) {
        loadExample(DEFAULT_EXAMPLE);
        showMessage(`This link names an example the playground does not have (${slug}).`);
        return true;
      }
      loadExample(slug);
      return true;
    }
    if (params.has("z")) {
      if (typeof DecompressionStream === "undefined") {
        loadExample(DEFAULT_EXAMPLE);
        showMessage(
          "This link holds a compressed program, and this browser has no DecompressionStream to read it."
        );
        return true;
      }
      const bytes = await pipeBytes(
        fromBase64Url(params.get("z")),
        new DecompressionStream("deflate-raw")
      );
      loadSource(new TextDecoder("utf-8", { fatal: true }).decode(bytes), null);
      return true;
    }
    if (params.has("code")) {
      const bytes = fromBase64Url(params.get("code"));
      loadSource(new TextDecoder("utf-8", { fatal: true }).decode(bytes), null);
      return true;
    }
  } catch (_) {
    loadExample(DEFAULT_EXAMPLE);
    showMessage("The program in this link could not be read; it may have been cut short.");
    return true;
  }
  return false;
}

async function share() {
  const status = $("share-status");
  const fragment = await encodeFragment($("source").value);
  // replaceState does not fire `hashchange`, so this does not reload the editor.
  history.replaceState(null, "", "#" + fragment);
  const url = location.href;
  try {
    await navigator.clipboard.writeText(url);
    status.textContent = "Link copied";
  } catch (_) {
    status.textContent = "Copy the link from the address bar";
  }
  clearTimeout(share.timer);
  share.timer = setTimeout(() => (status.textContent = ""), 4000);
}

let timer = null;
function scheduleCompile() {
  clearTimeout(timer);
  timer = setTimeout(compileToView, 200);
}

async function main() {
  await init();
  $("version").textContent = "v" + version();
  const ta = $("source");
  renderPicker();
  ta.addEventListener("input", () => {
    // An edited example is no longer that example: its caption would describe
    // a program that is not on screen, and Share must send the edit.
    if (currentSlug != null) {
      currentSlug = null;
      $("example").value = "";
      renderCaption();
    }
    // The fragment named the program before this edit; reloading should not
    // quietly bring that one back as if it were what is on screen.
    if (location.hash) history.replaceState(null, "", location.pathname + location.search);
    renderGutter(); // keep line numbers immediate; diagnostics follow debounced
    scheduleCompile();
  });
  $("example").addEventListener("change", (e) => {
    if (e.target.value) {
      loadExample(e.target.value);
      history.replaceState(null, "", "#example=" + e.target.value);
    }
  });
  $("share").addEventListener("click", share);
  window.addEventListener("hashchange", loadFromFragment);
  ta.addEventListener("scroll", syncGutterScroll);

  // Click a diagnostic row to select its span in the editor.
  $("diagnostics").addEventListener("click", (e) => {
    const row = e.target.closest(".diag[data-diag]");
    if (row) jumpToDiag(currentDiags[Number(row.dataset.diag)]);
  });
  $("diagnostics").addEventListener("keydown", (e) => {
    if (e.key !== "Enter" && e.key !== " ") return;
    const row = e.target.closest(".diag[data-diag]");
    if (row) {
      e.preventDefault();
      jumpToDiag(currentDiags[Number(row.dataset.diag)]);
    }
  });

  // Click a red gutter line to jump to the first diagnostic on that line.
  $("gutter").addEventListener("click", (e) => {
    const ln = e.target.closest(".ln.err");
    if (!ln) return;
    const line = Number(ln.dataset.line);
    const d = currentDiags.find(
      (x) => line >= x.start_line + 1 && line <= x.end_line + 1
    );
    if (d) jumpToDiag(d);
  });

  if (!(await loadFromFragment())) loadExample(DEFAULT_EXAMPLE);
  renderAgentEditDemo();
  $("app").setAttribute("aria-busy", "false");
}

main().catch((e) => {
  document.getElementById("ts").querySelector("code").textContent =
    "failed to load the Glyph compiler: " + e;
  // Clear the loading state so the error is not dimmed and assistive tech does
  // not keep announcing "busy".
  $("app").setAttribute("aria-busy", "false");
});
