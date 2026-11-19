function discount(plan: "free" | "pro" | "team"): number {
  let pct = 0;
  switch (plan) {
    // show
    case "pro": pct = 10;
    case "team": pct = 20; break;
    // end show
    case "free": pct = 0;
  }
  return pct;
}

console.log("pro discount", discount("pro"));
