// std/sqlite — a persisted SQL database over Node's built-in synchronous SQLite
// (`node:sqlite`). A thin platform-binding wrapper (the same role std/fs plays
// over node:fs): `open(path)` returns a `Db` handle whose methods run statements
// with positional parameters. Rows come back as `Record<string, unknown>` — the
// untrusted boundary — so a caller validates each row into a typed record with
// its descriptor's `.parse` before trusting it.

import { DatabaseSync } from "node:sqlite";
import { None, type Option, Some } from "./option";
import { expose, type Trusted } from "./taint";

export type Row = Record<string, unknown>;

// The text of a query (D52). In TypeScript it is a `string`; Glyph's checker
// holds every `Sql` to a constant string expression (E0239), so a run-time
// value reaches a query only through the parameter array or `from_trusted`.
export type Sql = string;

// Text built at run time, run as SQL. The argument is a `Trusted<string>`,
// which only `taint.sanitize` and `taint.trust_unchecked` produce, so every
// query that is not a constant is one grep away.
export function from_trusted(t: Trusted<string>): Sql {
  return expose(t);
}

export type Db = {
  // Execute one or more statements with no parameters and no result (DDL).
  exec: (sql: Sql) => void;
  // Run a parameterized statement (INSERT/UPDATE/DELETE); returns rows affected.
  run: (sql: Sql, params: ReadonlyArray<unknown>) => number;
  // The last auto-increment rowid produced on this connection.
  last_insert_id: () => number;
  // Query rows.
  query: (sql: Sql, params: ReadonlyArray<unknown>) => Array<Row>;
  // Query the first row, or `None` if the result is empty.
  query_one: (sql: Sql, params: ReadonlyArray<unknown>) => Option<Row>;
  close: () => void;
};

export function open(path: string): Db {
  const db = new DatabaseSync(path);
  let last = 0;
  // The bound params cross from Glyph's `unknown` into node:sqlite's stricter
  // `SQLInputValue`. We assert them at this one platform seam (`as any`) so the
  // wrapper type-checks the same whether the project pulls in the real
  // `@types/node` sqlite types or the bundled shim. A caller still passes values
  // they intend as SQL parameters; node validates them at run time.
  return {
    exec: (sql: string) => {
      db.exec(sql);
    },
    run: (sql: string, params: ReadonlyArray<unknown>) => {
      const info = db.prepare(sql).run(...(params as readonly any[]));
      last = Number(info.lastInsertRowid);
      return Number(info.changes);
    },
    last_insert_id: () => last,
    query: (sql: string, params: ReadonlyArray<unknown>) =>
      db.prepare(sql).all(...(params as readonly any[])) as Array<Row>,
    query_one: (sql: string, params: ReadonlyArray<unknown>) => {
      const row = db.prepare(sql).get(...(params as readonly any[])) as
        | Row
        | undefined;
      return row === undefined ? None : Some(row);
    },
    close: () => {
      db.close();
    },
  };
}
