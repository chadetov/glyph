function total(prices: number[]): number {
  let sum = 0;
  for (let i = 0; i < prices.length; i++)
    sum += prices[i];
  return sum;
}

console.log(total([10, 20, 30]));
