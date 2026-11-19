type User = { name: string; plan: "free" | "pro" };

function sendInvoice(plan: "free" | "pro"): string {
  return "invoice for the " + plan + " plan";
}

const body = '{"name":"ana"}';
// show
const user = JSON.parse(body) as User;
console.log(sendInvoice(user.plan));
// end show
