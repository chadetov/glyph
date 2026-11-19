type Conn = { id: number; open: boolean };

const conns: Conn[] = [];

function openConn(): Conn {
  const c = { id: conns.length + 1, open: true };
  conns.push(c);
  return c;
}

function closeConn(c: Conn): void {
  c.open = false;
}

function save(valid: boolean): string {
  // show
  const c = openConn();
  if (!valid) return "rejected";
  closeConn(c);
  // end show
  return "saved";
}

console.log(save(false), conns.filter((c) => c.open).length, "still open");
