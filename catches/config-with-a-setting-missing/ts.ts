type Plan = "free" | "pro" | "team";

const text = '{"free": 1, "pro": 5}';
// show
const limits = JSON.parse(text) as Record<Plan, number>;
console.log(limits.team * 2);
// end show
