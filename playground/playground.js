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

function renderDiagnostics(diags) {
  const el = $("diagnostics");
  if (diags.length === 0) {
    el.innerHTML = `<div class="diag-ok">✓ no diagnostics</div>`;
    return;
  }
  el.innerHTML = diags
    .map((d) => {
      const loc = `${d.start_line + 1}:${d.start_col + 1}`;
      return `<div class="diag"><span class="code">${escapeHtml(
        d.code
      )}</span><span class="loc">${loc}</span><span class="msg">${escapeHtml(
        d.message
      )}</span></div>`;
    })
    .join("");
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
  ta.addEventListener("input", scheduleCompile);
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
