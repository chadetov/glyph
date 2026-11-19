enum Status { Active, Suspended }

function label(s: Status): string {
  return s === Status.Active ? "active" : "suspended";
}

// show
const fromDb: number = 7;
console.log(label(fromDb));
// end show
