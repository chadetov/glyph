// show
type Account = { status: "active" | "banned" };

const body = '{"status":"deleted"}';
const acc = JSON.parse(body) as Account;
// end show
console.log("status", acc.status);
