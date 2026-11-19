type User = { name: string };

// show
function label(u: User): string {
  u.name = "admin";
  return u.name;
}
// end show

const ada: User = { name: "ada" };
label(ada);
console.log(ada.name);
