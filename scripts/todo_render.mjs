// Drives the bundled todo app in headless Chromium and asserts what a user
// sees. Run by check_todo_renders.py, which builds the app and serves it; the
// URL is the first argument. Playwright comes from the app's own node_modules,
// so the version is the one its lockfile pins.

import { createRequire } from "node:module";

const url = process.argv[2];
const app = new URL("../examples/apps/todo/package.json", import.meta.url);
const { chromium } = createRequire(app)("playwright");

const failures = [];
const expect = (label, actual, wanted) => {
  const ok = JSON.stringify(actual) === JSON.stringify(wanted);
  console.log(`${ok ? "ok  " : "FAIL"} ${label}: ${JSON.stringify(actual)}`);
  if (!ok) failures.push(`${label}: wanted ${JSON.stringify(wanted)}, got ${JSON.stringify(actual)}`);
};

const browser = await chromium.launch();
const page = await browser.newPage();
const errors = [];
page.on("pageerror", (e) => errors.push(`pageerror: ${e.message}`));
page.on("console", (m) => {
  if (m.type() === "error" || m.type() === "warning") errors.push(`${m.type()}: ${m.text()}`);
});

const items = () => page.$$eval("li", (els) => els.map((e) => `${e.textContent}:${e.className}`));

await page.goto(url);
await page.waitForSelector(".counter span");
expect("first render, count", await page.textContent(".counter span"), "0");
expect("first render, left line", await page.textContent(".left"), "0 left");
expect("first render, title", await page.title(), "0 left");

await page.click(".counter button:nth-of-type(2)");
await page.click(".counter button:nth-of-type(2)");
await page.click(".counter button:nth-of-type(1)");
expect("after + + -, count", await page.textContent(".counter span"), "1");

for (const text of ["buy milk", "write tests"]) {
  await page.fill(".todos input", text);
  await page.click(".todos button");
}
expect("two added, items", await items(), ["buy milk:open", "write tests:open"]);
expect("two added, input cleared", await page.inputValue(".todos input"), "");
expect("two added, left line", await page.textContent(".left"), "2 left");
expect("two added, title", await page.title(), "2 left");

await page.click("li >> text=buy milk");
expect("toggled, items", await items(), ["buy milk:done", "write tests:open"]);
expect("toggled, left line", await page.textContent(".left"), "1 left");
expect("toggled, title", await page.title(), "1 left");
const saved = () => page.evaluate(() => localStorage.getItem("todos"));
expect("saved list", await saved(), "[x] buy milk\n[ ] write tests");

// The list keeps each item's text and whether it was done.
await page.reload();
await page.waitForSelector(".counter span");
expect("after reload, items", await items(), ["buy milk:done", "write tests:open"]);
expect("after reload, left line", await page.textContent(".left"), "1 left");
expect("after reload, title", await page.title(), "1 left");

// A list saved by 0.1.128 is text alone, one item per line, and loads with
// nothing done.
await page.evaluate(() => localStorage.setItem("todos", "buy milk\nwrite tests"));
await page.reload();
await page.waitForSelector(".counter span");
expect("old format, items", await items(), ["buy milk:open", "write tests:open"]);
expect("old format, left line", await page.textContent(".left"), "2 left");
expect("old format, saved back with markers", await saved(), "[ ] buy milk\n[ ] write tests");

expect("console errors and warnings", errors, []);
await browser.close();

if (failures.length) {
  console.log(`\n${failures.length} failed:`);
  for (const f of failures) console.log(`  ${f}`);
  process.exit(1);
}
