function dueDate(input: string): string {
  // show
  return new Date(input).toISOString().slice(0, 10);
  // end show
}

console.log(dueDate("2025-02-30"));
console.log(dueDate("next friday"));
