// show
function smallest(xs: number[]): number | undefined {
  return xs.sort((a, b) => a - b)[0];
}
// end show

const xs = [3, 1, 2];
console.log("smallest", smallest(xs), "and xs[0] is now", xs[0]);
