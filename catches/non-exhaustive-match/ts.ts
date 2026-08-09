// tsc --strict accepts this. `label` is declared to return a string and
// returns undefined for "archived", because a switch with no default is not
// required to cover its input.
export type Status = "todo" | "doing" | "done" | "archived";

export function label(s: Status): string {
  switch (s) {
    case "todo":
      return "not started";
    case "doing":
      return "in progress";
    case "done":
      return "finished";
  }
  return undefined as unknown as string;
}
