type User = { id: number; name: string };

const users: User[] = [{ id: 1, name: "Ada" }];
// show
const rows = users.map((u) => { label: u.name });
// end show
console.log(rows);
