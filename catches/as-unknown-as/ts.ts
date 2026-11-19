type Config = { hosts: string[] };

const raw = { hostname: "db.internal" };
// show
const cfg = raw as unknown as Config;
console.log(cfg.hosts.length);
// end show
