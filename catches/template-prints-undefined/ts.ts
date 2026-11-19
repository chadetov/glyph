type Manager = { name: string };
type Employee = { name: string; manager: Manager | null };

function reportsTo(e: Employee): string {
  // show
  return `Reports to: ${e.manager?.name}`;
  // end show
}

console.log(reportsTo({ name: "Ada", manager: null }));
