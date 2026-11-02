// tsc --strict accepts this. `announce` returns nothing, so there is no return
// type for the missing case to violate, and a `switch` is not required to
// cover its input. "archived" falls out of the bottom and nothing is logged.
export type Status = "todo" | "doing" | "done" | "archived";

export function announce(s: Status): void {
  switch (s) {
    case "todo":
      console.log("not started");
      break;
    case "doing":
      console.log("in progress");
      break;
    case "done":
      console.log("finished");
      break;
  }
}
