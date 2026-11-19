type User = { name: string };
type Saved = { ok: boolean };

function save(user: User): Saved {
  return { ok: user.name.length > 100 };
}

function submit(user: User): string {
  // show
  save(user); // returns { ok: false }
  return "done";
  // end show
}

console.log(submit({ name: "ada" }));
