type ConnectOptions = { host: string; timeoutMs?: number };

function connect(o: ConnectOptions): number {
  return o.timeoutMs ?? 30000;
}

// show
const opts = { host: "db", timeoutMS: 500 };
console.log("timeout", connect(opts));
// end show
