function fee(): Promise<number> {
  return Promise.resolve(10);
}

async function charge(c: number[]): Promise<void> {
  // show
  const first = c[0] ?? 0;
  const f = await fee();
  c[0] = first - f;
  // end show
}

const balances = [100];
void Promise.all([charge(balances), charge(balances)]).then(() => {
  console.log("balance", balances[0]);
});
