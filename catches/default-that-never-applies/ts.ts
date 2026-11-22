type Option<T> = { tag: "Some"; value: T } | { tag: "None" };

function find<T>(xs: T[], f: (x: T) => boolean): Option<T> {
  for (const x of xs) {
    if (f(x)) return { tag: "Some", value: x };
  }
  return { tag: "None" };
}

const ports = [80, 8080];
// show
const port = find(ports, (p) => p > 9000) ?? 3000;
// end show
console.log("listening on " + String(port));
