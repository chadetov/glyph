function total(prices: number[], taxRate: number): number {
  let sum = 0;
  for (const p of prices) sum += p;
  // show
  return sum;
  sum = sum * (1 + taxRate);
  // end show
}

console.log(total([10, 20], 0.2));
