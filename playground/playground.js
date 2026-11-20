// The Glyph playground. Loads the WASM compiler and compiles the editor's
// contents on every (debounced) keystroke. The right-hand panel has two tabs:
// "TypeScript" (what a refused case looks like in TypeScript, or what the
// program compiles to) and "Your edit" (a line diff of your Glyph against the
// program you loaded, and of the TypeScript each compiles to).
//
// The example picker comes from examples.js, which playground/build.sh
// generates from catches/ and playground/examples/. A refused case carries the
// TypeScript excerpt and the run output recorded in its case files; this page
// runs neither tsc nor the program. A program is shared in the URL fragment,
// which the browser never sends to a server.

import init, { compile, version } from "./pkg/glyph_wasm.js";
import { CASE_COUNT, DEFAULT_EXAMPLE, EXAMPLES, KEYWORDS } from "./examples.js";

const bySlug = new Map(EXAMPLES.map((e) => [e.slug, e]));
const ERROR_CODES_URL = "https://github.com/chadetov/glyph/blob/main/docs/error-codes.md";

const $ = (id) => document.getElementById(id);

function escapeHtml(s) {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

// Inline `code` spans in a caption written with backticks.
function inlineCode(s) {
  return escapeHtml(s).replace(/`([^`]+)`/g, "<code>$1</code>");
}

const plural = (n, one, many) => `${n} ${n === 1 ? one : many}`;

// ---- Highlighting ---------------------------------------------------------
//
// A small tokenizer for Glyph and TypeScript, coloured with the site's code
// palette. The keyword sets come from examples.js, which reads them from the
// same place the site's highlighter does (the Glyph ones from the lexer).

const SETS = {
  glyph: new Set(KEYWORDS.glyph),
  ts: new Set(KEYWORDS.ts),
};
const PRIMITIVES = new Set(KEYWORDS.primitives);
const IDENT = /[A-Za-z_$][A-Za-z0-9_$]*/y;
const NUMBER = /\d[\d_]*(?:\.\d[\d_]*)?(?:[eE][+-]?\d+)?n?/y;

function tokenize(src, lang) {
  const kw = SETS[lang];
  const out = [];
  const n = src.length;
  let i = 0;
  while (i < n) {
    const c = src[i];
    if (c === "/" && src[i + 1] === "/") {
      let j = src.indexOf("\n", i);
      if (j < 0) j = n;
      out.push([i, j, "c"]);
      i = j;
    } else if (c === "/" && src[i + 1] === "*") {
      let j = src.indexOf("*/", i + 2);
      j = j < 0 ? n : j + 2;
      out.push([i, j, "c"]);
      i = j;
    } else if (c === '"' || c === "'" || (c === "`" && lang === "ts")) {
      let j = i + 1;
      while (j < n && src[j] !== c && (c === "`" || src[j] !== "\n")) j += src[j] === "\\" ? 2 : 1;
      j = Math.min(n, j + 1);
      out.push([i, j, "s"]);
      i = j;
    } else if (c >= "0" && c <= "9") {
      NUMBER.lastIndex = i;
      const m = NUMBER.exec(src);
      const j = m ? i + m[0].length : i + 1;
      out.push([i, j, "n"]);
      i = j;
    } else if (/[A-Za-z_$]/.test(c)) {
      IDENT.lastIndex = i;
      const word = IDENT.exec(src)[0];
      const j = i + word.length;
      let k = j;
      while (src[k] === " ") k++;
      const cls = kw.has(word)
        ? "k"
        : PRIMITIVES.has(word) || /^[A-Z]/.test(word)
        ? "t"
        : src[k] === "("
        ? "f"
        : null;
      if (cls) out.push([i, j, cls]);
      i = j;
    } else if ((c === "-" || c === "=") && src[i + 1] === ">") {
      out.push([i, i + 2, "op"]);
      i += 2;
    } else if (c === "?" && lang === "glyph") {
      out.push([i, i + 1, "op"]);
      i += 1;
    } else {
      i += 1;
    }
  }
  return out;
}

// The text as highlighted HTML, one string per line. `squiggles` are
// [start, end, "e" | "w"] offsets to underline.
function highlightLines(src, lang, squiggles = []) {
  const cls = new Array(src.length).fill(null);
  for (const [s, e, c] of tokenize(src, lang)) for (let i = s; i < e; i++) cls[i] = c;
  const sq = new Array(src.length).fill(null);
  for (const [s, e, w] of squiggles) {
    for (let i = s; i < Math.min(e, src.length); i++) if (sq[i] !== "e") sq[i] = w;
  }
  const lines = [];
  let start = 0;
  for (const text of src.split("\n")) {
    let html = "";
    let i = 0;
    while (i < text.length) {
      const c = cls[start + i];
      const w = sq[start + i];
      let j = i + 1;
      while (j < text.length && cls[start + j] === c && sq[start + j] === w) j++;
      let piece = escapeHtml(text.slice(i, j));
      if (c) piece = `<span class="${c}">${piece}</span>`;
      if (w) piece = `<span class="sq${w === "w" ? " w" : ""}">${piece}</span>`;
      html += piece;
      i = j;
    }
    lines.push(html);
    start += text.length + 1;
  }
  return lines;
}

// Numbered, highlighted code for an output pane.
function codeBlock(src, lang, extra = "") {
  const rows = highlightLines(src.replace(/\n$/, ""), lang)
    .map((h, i) => `<span class="row" data-n="${i + 1}">${h || " "}</span>`)
    .join("");
  return `<pre class="out ${extra}">${rows}</pre>`;
}

// ---- Line diff --------------------------------------------------------------

// An LCS line diff -> ops [{ type: "same" | "add" | "del", a, b }], where a and
// b are line indexes into each side. The common head and tail are matched
// first, so the table is only as large as the region that changed.
function diffLines(aText, bText) {
  const a = aText.replace(/\n$/, "").split("\n");
  const b = bText.replace(/\n$/, "").split("\n");
  let head = 0;
  while (head < a.length && head < b.length && a[head] === b[head]) head++;
  let tail = 0;
  while (
    tail < a.length - head &&
    tail < b.length - head &&
    a[a.length - 1 - tail] === b[b.length - 1 - tail]
  )
    tail++;
  const n = a.length - head - tail;
  const m = b.length - head - tail;
  const ops = [];
  for (let i = 0; i < head; i++) ops.push({ type: "same", a: i, b: i });
  if (n * m > 4_000_000) {
    // Too large to align line by line in a keystroke: show it as replaced.
    for (let i = 0; i < n; i++) ops.push({ type: "del", a: head + i, b: null });
    for (let j = 0; j < m; j++) ops.push({ type: "add", a: null, b: head + j });
  } else {
    const w = m + 1;
    const lcs = new Uint32Array((n + 1) * w);
    for (let i = n - 1; i >= 0; i--) {
      for (let j = m - 1; j >= 0; j--) {
        lcs[i * w + j] =
          a[head + i] === b[head + j]
            ? lcs[(i + 1) * w + j + 1] + 1
            : Math.max(lcs[(i + 1) * w + j], lcs[i * w + j + 1]);
      }
    }
    let i = 0;
    let j = 0;
    while (i < n && j < m) {
      if (a[head + i] === b[head + j]) {
        ops.push({ type: "same", a: head + i++, b: head + j++ });
      } else if (lcs[(i + 1) * w + j] >= lcs[i * w + j + 1]) {
        ops.push({ type: "del", a: head + i++, b: null });
      } else {
        ops.push({ type: "add", a: null, b: head + j++ });
      }
    }
    while (i < n) ops.push({ type: "del", a: head + i++, b: null });
    while (j < m) ops.push({ type: "add", a: null, b: head + j++ });
  }
  for (let k = 0; k < tail; k++) ops.push({ type: "same", a: a.length - tail + k, b: b.length - tail + k });
  return ops;
}

function counts(ops) {
  let del = 0;
  let add = 0;
  for (const o of ops) {
    if (o.type === "del") del++;
    else if (o.type === "add") add++;
  }
  return { del, add };
}

const fmtCounts = (c) => `<b class="r">&minus;${c.del}</b> <b class="g">+${c.add}</b>`;
const plainCounts = (c) => `−${c.del} +${c.add}`;

// The changed lines with two lines of context around each run, highlighted.
function renderDiff(aText, bText, ops, lang) {
  const aLines = highlightLines(aText.replace(/\n$/, ""), lang);
  const bLines = highlightLines(bText.replace(/\n$/, ""), lang);
  const CONTEXT = 2;
  const near = new Array(ops.length).fill(false);
  ops.forEach((o, k) => {
    if (o.type === "same") return;
    for (let d = -CONTEXT; d <= CONTEXT; d++) if (ops[k + d]) near[k + d] = true;
  });
  let html = "";
  let skipped = false;
  ops.forEach((o, k) => {
    if (!near[k]) {
      skipped = true;
      return;
    }
    if (skipped && html) html += `<span class="row gap"> </span>`;
    skipped = false;
    const text = o.type === "add" ? bLines[o.b] : o.type === "del" ? aLines[o.a] : bLines[o.b];
    const cls = o.type === "same" ? "ctx" : o.type;
    html += `<span class="row ${cls}">${text || " "}</span>`;
  });
  return `<pre class="out compact">${html}</pre>`;
}

// ---- State ------------------------------------------------------------------

// What "Your edit" compares against: the example or shared program last
// loaded, and its compile output.
let base = { slug: null, source: "", out: null };
// The latest compile of the editor.
let current = { source: "", out: null };
let currentDiags = [];

function compileJson(src) {
  try {
    return JSON.parse(compile(src));
  } catch (e) {
    return { ts: null, diagnostics: [], failure: String(e) };
  }
}

const errorsOf = (out) => (out.diagnostics || []).filter((d) => d.severity !== "warning");
const warningsOf = (out) => (out.diagnostics || []).filter((d) => d.severity === "warning");
const isEdited = () => current.source !== base.source;
const baseExample = () => (base.slug != null ? bySlug.get(base.slug) : null);

// ---- The editor ----------------------------------------------------------

// Map a (0-based line, UTF-16 column) to an index into the source. The textarea
// uses UTF-16 code units, matching the compiler's column units.
function offsetOf(src, line, col) {
  const lines = src.split("\n");
  let off = 0;
  for (let i = 0; i < line && i < lines.length; i++) off += lines[i].length + 1;
  return off + col;
}

function renderEditor() {
  const src = $("source").value;
  const lines = src.split("\n");
  // Underline each diagnostic on the line it starts on: some spans (an
  // unterminated string, say) run to the end of the file.
  const squiggles = currentDiags.map((d) => {
    const s = offsetOf(src, d.start_line, d.start_col);
    const lineEnd = offsetOf(src, d.start_line, (lines[d.start_line] || "").length);
    let e = d.end_line === d.start_line ? offsetOf(src, d.end_line, d.end_col) : lineEnd;
    e = Math.min(Math.max(e, s + 1), Math.max(lineEnd, s + 1));
    return [s, e, d.severity === "warning" ? "w" : "e"];
  });
  const errs = new Set(currentDiags.filter((d) => d.severity !== "warning").map((d) => d.start_line));
  const warns = new Set(currentDiags.filter((d) => d.severity === "warning").map((d) => d.start_line));
  const mark = (i) => (errs.has(i) ? " err" : warns.has(i) ? " warn" : "");
  $("highlight").innerHTML = highlightLines(src, "glyph", squiggles)
    .map((h, i) => `<span class="row${mark(i)}">${h || " "}</span>`)
    .join("");
  let gutter = "";
  for (let i = 0; i < lines.length; i++) {
    gutter += `<div class="ln${mark(i)}" data-line="${i + 1}">${i + 1}</div>`;
  }
  $("gutter").innerHTML = gutter;
  syncScroll();
}

function syncScroll() {
  const ta = $("source");
  const hl = $("highlight");
  hl.scrollTop = ta.scrollTop;
  hl.scrollLeft = ta.scrollLeft;
  $("gutter").scrollTop = ta.scrollTop;
}

// Select a diagnostic's span in the editor and scroll it into view.
function jumpToDiag(d) {
  const ta = $("source");
  const src = ta.value;
  const start = offsetOf(src, d.start_line, d.start_col);
  const end = Math.max(offsetOf(src, d.end_line, d.end_col), start + 1);
  ta.focus();
  ta.setSelectionRange(start, end);
  const lineHeight = parseFloat(getComputedStyle(ta).lineHeight) || 24;
  ta.scrollTop = Math.max(0, (d.start_line - 3) * lineHeight);
  syncScroll();
}

function renderDiagnostics() {
  const el = $("diagnostics");
  if (current.out.failure) {
    el.innerHTML = `<div class="diag"><span class="code-id">playground error</span> ${escapeHtml(current.out.failure)}</div>`;
    return;
  }
  if (currentDiags.length === 0) {
    el.innerHTML = `<div class="diag-ok">✓ No errors</div>`;
    return;
  }
  el.innerHTML = currentDiags
    .map((d, i) => {
      // A lint and a must-use warning are warnings in `glyph check`, so they
      // are warnings here; painting them red would call a buildable program
      // broken.
      const kind = d.severity === "warning" ? "warn" : "err";
      const [msg, ...help] = d.message.split("\n");
      const explain = `<a class="explain" href="${ERROR_CODES_URL}" target="_blank" rel="noopener" title="${escapeHtml(
        d.code
      )} in the error-code catalogue (glyph --explain ${escapeHtml(d.code)})">--explain</a>`;
      return (
        `<div class="diag ${kind}" data-diag="${i}" role="button" tabindex="0" title="Jump to line ${d.start_line + 1}">` +
        `<span class="code-id">${escapeHtml(d.code)}</span>` +
        `<span class="at">${d.start_line + 1}:${d.start_col + 1}</span>${escapeHtml(msg)}` +
        (help.length ? `<span class="help">${escapeHtml(help.join(" "))} ${explain}</span>` : ` ${explain}`) +
        `</div>`
      );
    })
    .join("");
}

// ---- Caption ----------------------------------------------------------------

function renderCaption() {
  const el = $("caption");
  const e = baseExample();
  const errs = errorsOf(current.out);
  if (e != null && !isEdited()) {
    if (e.kind === "catch") {
      el.innerHTML =
        `<span class="tag">Glyph refuses · ${escapeHtml(e.code)}</span>` +
        `<span class="text">${e.why || escapeHtml(e.title)}</span>` +
        `<a class="more" href="/catches/">${CASE_COUNT - 1} more cases →</a>`;
    } else {
      el.innerHTML =
        `<span class="tag ok">Compiles</span>` + `<span class="text">${inlineCode(e.title)}</span>`;
    }
    return;
  }
  const tag =
    errs.length > 0
      ? `<span class="tag">Does not compile · ${escapeHtml(errs[0].code)}</span>`
      : `<span class="tag ok">Compiles</span>`;
  let text;
  if (e != null) {
    const what = e.kind === "catch" ? e.why || escapeHtml(e.title) : inlineCode(e.title);
    text = `${what} You edited it: ${plainCounts(counts(diffLines(base.source, current.source)))}.`;
  } else {
    text = isEdited() ? "Your program, edited." : "Your program, from a shared link.";
  }
  el.innerHTML =
    tag +
    `<span class="text">${text}</span>` +
    (e != null ? `<button class="more" type="button" id="reset">Reset example</button>` : "");
}

// ---- TypeScript tab -------------------------------------------------------

// Say which imports this page could not check against a project, and what
// `glyph build` would do differently with them. Empty when the compiler names
// none: a module that imports nothing outside `std/*` compiles here to what
// `glyph build` writes.
function projectNote(paths) {
  if (!paths || paths.length === 0) return "";
  const coded = paths.map((p) => `<code>${escapeHtml(p)}</code>`);
  const names = coded.length > 1 ? coded.slice(0, -1).join(", ") + " or " + coded[coded.length - 1] : coded[0];
  const one = paths.length === 1;
  return (
    `<div class="note top">This page compiles the one module in the editor and has no ${names}, so ` +
    `${one ? "that import was" : "those imports were"} compiled as ${one ? "an npm package" : "npm packages"}. ` +
    `If ${one ? "it is a module" : "any of them is a module"} in your own project, <code>glyph build</code> ` +
    `writes a relative specifier there, and the real runtime check for any type ` +
    `${one ? "that module declares" : "those modules declare"} where this page falls back to a presence check.</div>`
  );
}

function renderTsTab() {
  const el = $("body-ts");
  const e = baseExample();
  const out = current.out;
  if (e != null && e.kind === "catch" && !isEdited()) {
    const crashed = /^\w*Error\b/.test(e.ts_output);
    el.innerHTML =
      `<div class="verdict"><span class="pill">tsc --strict: 0 errors</span>` +
      (e.ts_verdict ? `<span class="pill bad">${escapeHtml(e.ts_verdict)}</span>` : "") +
      `</div>` +
      codeBlock(e.ts_code, "ts") +
      `<div class="runout"><span class="cmd">$ ${escapeHtml(e.ts_command)}</span>` +
      `<span${crashed ? ' class="err"' : ""}>${escapeHtml(e.ts_output)}</span></div>` +
      `<div class="note">The same program in TypeScript, from <a href="/catches/#${e.slug}">this case</a>. ` +
      `The verdict and output were recorded when the case was last checked; this page runs neither ` +
      `<code>tsc</code> nor the program. Glyph refuses the version on the left with <code>${escapeHtml(e.code)}</code>.</div>`;
    return;
  }
  if (out.failure) {
    el.innerHTML = `<div class="empty">The compiler stopped: ${escapeHtml(out.failure)}</div>`;
    return;
  }
  const errs = errorsOf(out);
  if (errs.length > 0) {
    el.innerHTML =
      `<div class="verdict"><span class="pill bad">${plural(errs.length, "error", "errors")}</span>` +
      `<span>glyph build writes no TypeScript for this</span></div>` +
      (out.ts != null
        ? `<div class="note top">Your Glyph has errors, listed under the editor. Below is what the emitter produces ` +
          `anyway, so you can see where it is going; <code>glyph build</code> would stop before writing it.</div>` +
          projectNote(out.assumed_external_imports) +
          codeBlock(out.ts, "ts")
        : `<div class="empty">Your Glyph has errors, listed under the editor, and the compiler stopped before ` +
          `producing any TypeScript. Fix the first one and the output appears here.</div>`);
    return;
  }
  const warns = warningsOf(out);
  const lines = out.ts.replace(/\n$/, "").split("\n").length;
  el.innerHTML =
    `<div class="verdict"><span class="pill">compiled</span><span>main.ts · ${plural(lines, "line", "lines")}</span>` +
    (warns.length ? `<span>${plural(warns.length, "warning", "warnings")}</span>` : "") +
    `</div>` +
    projectNote(out.assumed_external_imports) +
    codeBlock(out.ts, "ts");
}

// ---- Your edit tab ----------------------------------------------------------

function renderEditTab() {
  const el = $("body-edit");
  const badge = $("edit-count");
  if (!isEdited()) {
    badge.textContent = "none yet";
    el.innerHTML =
      `<div class="empty">Change anything on the left and this shows what changed: ` +
      `in your Glyph, and in the TypeScript it compiles to.</div>`;
    return;
  }
  const gOps = diffLines(base.source, current.source);
  const g = counts(gOps);
  badge.textContent = plainCounts(g);
  let html =
    `<div class="dlabel">Glyph · main.glyph <span class="counts">${fmtCounts(g)}</span></div>` +
    renderDiff(base.source, current.source, gOps, "glyph");
  let sum = `Your Glyph diff: ${fmtCounts(g)}.`;
  const nowOk = current.out.ts != null && errorsOf(current.out).length === 0;
  const wasOk = base.out.ts != null && errorsOf(base.out).length === 0;
  if (!nowOk) {
    html += `<div class="note">Your Glyph does not compile yet. The TypeScript diff appears once it does.</div>`;
  } else if (!wasOk) {
    html +=
      `<div class="note">The program you started from does not compile, so there is no TypeScript ` +
      `to compare against. Your version compiles; its TypeScript is on the other tab.</div>`;
  } else {
    const tOps = diffLines(base.out.ts, current.out.ts);
    const t = counts(tOps);
    html +=
      `<div class="dlabel">Compiled TypeScript · main.ts <span class="counts">${fmtCounts(t)}</span></div>` +
      (t.del + t.add > 0 ? renderDiff(base.out.ts, current.out.ts, tOps, "ts") : `<div class="empty">No change.</div>`);
    sum += ` The TypeScript it compiles to: ${fmtCounts(t)}.`;
    if (t.del + t.add === 0) {
      sum += " The TypeScript did not change.";
    } else if (t.del === g.del && t.add === g.add) {
      const n = Math.max(g.del, g.add);
      sum += n === 1 ? " One line changed, one line to review, in both." : ` The same ${n} lines to review in both.`;
    }
  }
  html += `<div class="sum">${sum}</div>`;
  el.innerHTML = html;
}

// ---- Tabs -------------------------------------------------------------------

function showTab(name) {
  for (const t of document.querySelectorAll(".tab")) {
    const on = t.dataset.tab === name;
    t.classList.toggle("on", on);
    t.setAttribute("aria-selected", on ? "true" : "false");
  }
  $("body-ts").hidden = name !== "ts";
  $("body-edit").hidden = name !== "edit";
}

// ---- Compile and render -------------------------------------------------------

function compileToView() {
  const src = $("source").value;
  current = { source: src, out: compileJson(src) };
  currentDiags = current.out.diagnostics || [];
  renderEditor();
  renderDiagnostics();
  renderCaption();
  renderTsTab();
  renderEditTab();
  // The picker names the loaded example, or says it has been edited.
  const e = baseExample();
  const own = $("example").querySelector('option[value=""]');
  own.textContent = e != null ? `${e.label} (edited)` : "Your program";
  $("example").value = isEdited() || e == null ? "" : e.slug;
}

// ---- The example picker -------------------------------------------------------

function renderPicker() {
  const group = (label, kind) =>
    `<optgroup label="${label}">` +
    EXAMPLES.filter((e) => e.kind === kind)
      .map((e) => `<option value="${e.slug}">${escapeHtml(e.label)}</option>`)
      .join("") +
    `</optgroup>`;
  $("example").innerHTML =
    `<option value="" hidden>Your program</option>` +
    group("Refused by Glyph, accepted by tsc --strict", "catch") +
    group("Compiles", "program");
}

function loadSource(src, slug) {
  const ta = $("source");
  ta.value = src;
  ta.scrollTop = 0;
  ta.scrollLeft = 0;
  base = { slug, source: src, out: compileJson(src) };
  compileToView();
}

function loadExample(slug) {
  const e = bySlug.get(slug) ?? bySlug.get(DEFAULT_EXAMPLE);
  loadSource(e.source, e.slug);
  // A case opens on its TypeScript, which is the point of it.
  showTab("ts");
}

function showMessage(text) {
  $("caption").innerHTML = `<span class="tag">Link</span><span class="text">${escapeHtml(text)}</span>`;
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
  if (base.slug != null && !isEdited()) return `example=${base.slug}`;
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
      loadExample(slug);
      if (!bySlug.has(slug)) showMessage(`This link names an example the playground does not have (${slug}).`);
      return true;
    }
    if (params.has("z")) {
      if (typeof DecompressionStream === "undefined") {
        loadExample(DEFAULT_EXAMPLE);
        showMessage("This link holds a compressed program, and this browser has no DecompressionStream to read it.");
        return true;
      }
      const bytes = await pipeBytes(fromBase64Url(params.get("z")), new DecompressionStream("deflate-raw"));
      loadSource(new TextDecoder("utf-8", { fatal: true }).decode(bytes), null);
      return true;
    }
    if (params.has("code")) {
      loadSource(new TextDecoder("utf-8", { fatal: true }).decode(fromBase64Url(params.get("code"))), null);
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
  try {
    await navigator.clipboard.writeText(location.href);
    status.textContent = "Link copied";
  } catch (_) {
    status.textContent = "Copy the link from the address bar";
  }
  clearTimeout(share.timer);
  share.timer = setTimeout(() => (status.textContent = ""), 4000);
}

// ---- Wiring -------------------------------------------------------------------

let timer = null;
function scheduleCompile() {
  clearTimeout(timer);
  timer = setTimeout(compileToView, 200);
}

async function main() {
  await init();
  $("version").textContent = "v" + version();
  renderPicker();
  const ta = $("source");
  ta.addEventListener("input", () => {
    // The fragment named the program before this edit; reloading should not
    // quietly bring that one back as if it were what is on screen.
    if (location.hash) history.replaceState(null, "", location.pathname + location.search);
    // Keep the text, line numbers and colours immediate; the compile follows.
    current.source = ta.value;
    renderEditor();
    scheduleCompile();
  });
  ta.addEventListener("scroll", syncScroll);
  // Tab indents instead of leaving the editor; Escape then Tab still leaves.
  ta.addEventListener("keydown", (e) => {
    if (e.key === "Tab" && !e.shiftKey && !e.altKey && !e.metaKey && !e.ctrlKey && !ta.dataset.escaped) {
      e.preventDefault();
      document.execCommand("insertText", false, "  ");
    }
    ta.dataset.escaped = e.key === "Escape" ? "1" : "";
  });
  $("example").addEventListener("change", (e) => {
    if (e.target.value) {
      loadExample(e.target.value);
      history.replaceState(null, "", "#example=" + e.target.value);
    }
  });
  $("share").addEventListener("click", share);
  $("caption").addEventListener("click", (e) => {
    if (e.target.id !== "reset") return;
    $("source").value = base.source;
    compileToView();
    if (base.slug != null) history.replaceState(null, "", "#example=" + base.slug);
  });
  for (const t of document.querySelectorAll(".tab")) {
    t.addEventListener("click", () => showTab(t.dataset.tab));
  }
  window.addEventListener("hashchange", loadFromFragment);

  // Click a diagnostic to select its span in the editor.
  $("diagnostics").addEventListener("click", (e) => {
    if (e.target.closest("a")) return;
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
  // Click a red line number to jump to the first diagnostic on that line.
  $("gutter").addEventListener("click", (e) => {
    const ln = e.target.closest(".ln.err");
    if (!ln) return;
    const line = Number(ln.dataset.line);
    const d = currentDiags.find((x) => line >= x.start_line + 1 && line <= x.end_line + 1);
    if (d) jumpToDiag(d);
  });

  if (!(await loadFromFragment())) loadExample(DEFAULT_EXAMPLE);
  $("app").setAttribute("aria-busy", "false");
}

main().catch((e) => {
  $("body-ts").innerHTML = `<div class="empty">Failed to load the Glyph compiler: ${escapeHtml(String(e))}</div>`;
  // Clear the loading state so the error is not dimmed and assistive tech does
  // not keep announcing "busy".
  $("app").setAttribute("aria-busy", "false");
});
