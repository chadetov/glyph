function median(sorted: number[]): number {
  return sorted[Math.floor(sorted.length / 2)];
}

console.log(median([1, 2, 3]) * 2);
