// show
const isTicket = /^[A-Z]+-\d+$/g;
const ids = ["ABC-1", "ABC-2", "ABC-3"];
console.log(ids.map((id) => isTicket.test(id)));
// end show
