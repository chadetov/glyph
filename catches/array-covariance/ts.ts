type Animal = { name: string };
type Dog = { name: string; bark: () => string };

const dogs: Dog[] = [{ name: "Rex", bark: () => "woof" }];

// show
function adopt(shelter: Animal[]): void {
  shelter.push({ name: "Tom the cat" });
}

adopt(dogs);
// end show
dogs.forEach((d) => { console.log(d.bark()); });
