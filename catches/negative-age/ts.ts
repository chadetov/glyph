// show
type Person = { age: number }; // must be >= 0

const text = '{"age": -5}';
console.log((JSON.parse(text) as Person).age);
// end show
