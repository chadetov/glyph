function retries(requested: number): number {
  // show
  return requested || 3;
  // end show
}

console.log("retries", retries(0));
