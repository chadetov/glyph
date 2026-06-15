// The Glyph playground. Loads the WASM compiler, compiles the editor's contents
// to TypeScript on every (debounced) keystroke, renders diagnostics, and shows
// a fixed "agent edit" diff that makes the diff-stability pillar legible.

import init, { compile, version } from "./pkg/glyph_wasm.js";

const DEFAULT_SOURCE = `module pricing

import std/result { Result, Ok, Err }

type Plan =
  | Free
  | Pro({ seats: number })
  | Enterprise({ seats: number, discount: number })

fn monthly_cost(plan: Plan) -> Result<number, string> {
  return match plan {
    Free => Ok(0),
    Pro({ seats }) => Ok(seats * 12),
    Enterprise({ seats, discount }) => match discount >= 0 {
      true => Ok(seats * 12 - discount),
      false => Err("discount cannot be negative"),
    },
  }
}
`;

// The agent-edit demo: one line of Glyph changes (the Pro per-seat price), and
// the emitted TypeScript changes exactly one line in turn.
const EDIT_BEFORE = DEFAULT_SOURCE;
const EDIT_AFTER = DEFAULT_SOURCE.replace(
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
    .join("\n");
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
      return `<div class="diag" data-diag="${i}" role="button" tabindex="0" title="Jump to line ${
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

// Paint the gutter row where each diagnostic starts red (and clickable). Only
// the start line is marked: some spans (an unterminated string, say) run to the
// end of the file, and painting every line in between would be noise.
function markErrorLines() {
  const g = $("gutter");
  const lines = new Set(currentDiags.map((d) => d.start_line + 1));
  for (const ln of g.children) {
    ln.classList.toggle("err", lines.has(Number(ln.dataset.line)));
  }
}

function compileToView() {
  const src = $("source").value;
  let out;
  try {
    out = JSON.parse(compile(src));
  } catch (e) {
    $("ts").querySelector("code").textContent = `// playground error: ${e}`;
    return;
  }
  $("ts").querySelector("code").textContent =
    out.ts != null ? out.ts : "// (no output — fix the errors on the left)";
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
    ` Fixed-width, no-reflow formatting keeps a one-line change a one-line diff &mdash; review stays small.`;
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
  ta.value = DEFAULT_SOURCE;
  ta.addEventListener("input", () => {
    renderGutter(); // keep line numbers immediate; diagnostics follow debounced
    scheduleCompile();
  });
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

  compileToView();
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
