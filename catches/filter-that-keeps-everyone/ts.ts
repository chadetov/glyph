const banned = new Set(["mallory"]);

function isActive(name: string): Promise<boolean> {
  return Promise.resolve(!banned.has(name));
}

const names = ["ada", "mallory"];
// show
const active = names.filter(
  async (n) => await isActive(n));
// end show
console.log(active);
