// std/dns — name lookups, as values.
//
// Every function here is async and returns a `Result`, because a lookup fails
// for ordinary reasons a program is expected to handle: the name does not
// exist, the resolver did not answer in time, the record type is not present.
// Node throws for all of those, and a throw from a name lookup is how a health
// check turns into a crash.
//
// `lookup` and `resolve4` are not the same operation and the difference catches
// people out. `lookup` asks the operating system, so it sees `/etc/hosts`,
// mDNS, and whatever the resolver library is configured to do, which is what
// you want when the question is "what would connecting to this name do?".
// `resolve4` queries DNS directly and ignores all of that, which is what you
// want when the question is about the DNS record itself.

import { type Result, Ok, Err } from "./result";
import { lookup as node_lookup, resolve4, resolve6, resolveTxt, resolveMx } from "node:dns/promises";

/** A mail exchanger: `priority` orders them, lowest first. */
export type MailHost = {
  readonly priority: number;
  readonly host: string;
};

function reason(e: unknown): string {
  const code = (e as { code?: string } | null)?.code;
  const message = (e as { message?: string } | null)?.message ?? String(e);
  return code === undefined ? message : `${code}: ${message}`;
}

/**
 * Resolve a name the way connecting to it would, consulting the operating
 * system rather than DNS alone.
 *
 * This is the one to use before dialling: it honours `/etc/hosts`, so a name
 * pointed at `127.0.0.1` for local development resolves there instead of to
 * whatever public DNS says.
 */
export async function lookup(hostname: string): Promise<Result<string, string>> {
  try {
    const r = await node_lookup(hostname);
    return Ok(r.address);
  } catch (e: unknown) {
    return Err(reason(e));
  }
}

/** The A records for a name, querying DNS directly. Empty when there are none. */
export async function ipv4(hostname: string): Promise<Result<Array<string>, string>> {
  try {
    return Ok(await resolve4(hostname));
  } catch (e: unknown) {
    return Err(reason(e));
  }
}

/** The AAAA records for a name. */
export async function ipv6(hostname: string): Promise<Result<Array<string>, string>> {
  try {
    return Ok(await resolve6(hostname));
  } catch (e: unknown) {
    return Err(reason(e));
  }
}

/**
 * The TXT records for a name, one string per record.
 *
 * DNS splits a long TXT record into chunks of at most 255 bytes, and node
 * reports those chunks separately. They are joined back here with no separator,
 * which is what every consumer of a TXT record does: an SPF or a domain
 * verification token split across two chunks is one value, not two.
 */
export async function text(hostname: string): Promise<Result<Array<string>, string>> {
  try {
    const records = await resolveTxt(hostname);
    return Ok(records.map((chunks) => chunks.join("")));
  } catch (e: unknown) {
    return Err(reason(e));
  }
}

/** The MX records for a name. Sort by `priority` ascending to pick one. */
export async function mail(hostname: string): Promise<Result<Array<MailHost>, string>> {
  try {
    const records = await resolveMx(hostname);
    return Ok(records.map((r) => ({ priority: r.priority, host: r.exchange })));
  } catch (e: unknown) {
    return Err(reason(e));
  }
}
