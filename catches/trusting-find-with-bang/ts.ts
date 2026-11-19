type User = { id: number; name: string };
const users: User[] = [{ id: 1, name: "Ada" }];

function nameOf(id: number): string {
  // show
  return users.find((u) => u.id === id)!.name;
  // end show
}

console.log(nameOf(2));
