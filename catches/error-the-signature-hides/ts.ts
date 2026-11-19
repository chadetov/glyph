type Config = { port: number };

// show
function loadConfig(): Config {
  throw new Error("missing file");
}

const cfg = loadConfig(); // no try
// end show
console.log(cfg.port);
