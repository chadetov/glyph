// tsc --strict accepts this. `JSON.parse` returns `any`, so every field read
// past it is unchecked and the annotation on `user` is a promise nothing keeps.
// This is the shape of most real boundary code.
export type User = { id: number; email: string };

export function emailOf(body: string): string {
  const user: User = JSON.parse(body);
  return user.email;
}
