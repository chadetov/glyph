// demo.ml — the program fixture the interpreter runs by default.
// Every construct the language has appears here at least once.

let greeting = 'hello'
let name = "world"
print(greeting + ", " + name + "!")

// arithmetic and precedence
let a = 2 + 3 * 4
let b = (2 + 3) * 4
print("a=" + str(a) + " b=" + str(b))
print("17 % 5 = " + str(17 % 5) + ", 7 / 2 = " + str(7 / 2))
print("unary " + str(-a) + " " + str(!false))

// comparison and short-circuit logic
print(str(a < b) + " " + str(a == 14) + " " + str(a != b) + " " + str(b >= 20))
print(str(a < b && b > 0) + " " + str(false || nil == nil))

// recursion
fn fact(n) {
  if n <= 1 {
    return 1
  }
  return n * fact(n - 1)
}
print("fact(10) = " + str(fact(10)))

fn fib(n) {
  if n < 2 {
    return n
  } else {
    return fib(n - 1) + fib(n - 2)
  }
}
print("fib(15) = " + str(fib(15)))

// closures capture the environment they were defined in
fn make_counter(start) {
  let n = start
  fn tick() {
    n = n + 1
    return n
  }
  return tick
}
let c = make_counter(10)
print("counter " + str(c()) + " " + str(c()) + " " + str(c()))

// arrays
let xs = [1, 2, 3]
push(xs, 4)
print("xs = " + str(xs) + " len " + str(len(xs)) + " last " + str(xs[3]))

// records
let user = { name: "ada", age: 36, tags: ["math", "engines"] }
print(user.name + " is " + str(user.age) + " " + str(user["tags"][1]))
print("keys " + str(keys(user)))

// while, mutation, early return
fn sum_to(n) {
  let i = 0
  let total = 0
  while i < n {
    i = i + 1
    total = total + i
  }
  return total
}
print("sum_to(100) = " + str(sum_to(100)))

fn first_over(limit, values) {
  let i = 0
  while i < len(values) {
    if values[i] > limit {
      return values[i]
    }
    i = i + 1
  }
  return nil
}
print("first_over " + str(first_over(2, xs)) + " " + str(first_over(99, xs)))

// first-class functions
fn apply(f, x) {
  return f(x)
}
print("apply " + str(apply(fn(v) { return v * v }, 9)))

// blocks are expressions with their own scope
let shadow = 1
{
  let shadow = 2
  print("inner shadow " + str(shadow))
}
print("outer shadow " + str(shadow))

// strings and escapes
print("tab:[\t] escaped-newline:[\\n] quote:[\"] backslash:[\\]")
print('single \'quoted\' string')

// builtins
print(type_of(1) + " " + type_of("s") + " " + type_of(true) + " " + type_of(nil))
print(type_of(xs) + " " + type_of(user) + " " + type_of(apply) + " " + type_of(len))
print("num(\"42\") + 1 = " + str(num("42") + 1))
