// show
type Order = { qty: number };

const text = '{"qty": 1.5}';
console.log((JSON.parse(text) as Order).qty);
// end show
