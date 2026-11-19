class Counter {
  count = 0;
  increment(): void {
    this.count++;
  }
}

const counter = new Counter();
// show
[1, 2, 3].forEach(counter.increment);
// end show
console.log(counter.count);
