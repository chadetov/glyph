type Opts = { timeoutMs?: number };

// show
const opts = JSON.parse('{"timeoutMs": null}') as Opts;
const cfg = { timeoutMs: 5000, ...opts };
// end show
console.log("timeout x2 =", cfg.timeoutMs * 2);
