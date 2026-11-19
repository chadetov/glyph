// The page's version takes `x: any`, and the only thing the full lint run
// reports on it is `no-explicit-any`, which is about the parameter and not
// about the guard. This version takes `unknown`, so what is left is the guard
// claiming `User` after checking one field, and nothing reports that.
type User = { id: number; email: string };

// show
function isUser(x: unknown): x is User {
  return typeof x === "object" && x !== null && "id" in x;
}
// end show

const body: unknown = JSON.parse('{"id": 7}');
if (isUser(body)) console.log(body.email.toLowerCase());
