type Account = { balance: number };

function fee(): Promise<number> {
  return Promise.resolve(10);
}

async function charge(a: Account): Promise<void> {
  // show
  const before = a.balance;
  const f = await fee();
  a.balance = before - f;
  // end show
}

const acct: Account = { balance: 100 };
void Promise.all([charge(acct), charge(acct)]).then(() => {
  console.log("balance", acct.balance);
});
