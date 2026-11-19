function normalizeEmail(input: string): string {
  // show
  const cleaned = input.trim().toLowerCase();
  return input;
  // end show
}

console.log(JSON.stringify(normalizeEmail("  Ada@Example.COM ")));
