import { DatabaseSync } from "node:sqlite";

function find(db: DatabaseSync, name: string): number {
  // show
  return db.prepare(`SELECT * FROM users WHERE name = '${name}'`).all().length;
  // end show
}

const db = new DatabaseSync(":memory:");
db.exec("CREATE TABLE users (name TEXT)");
db.exec("INSERT INTO users VALUES ('ada'), ('linus')");
console.log("rows", find(db, "x' OR '1'='1"));
