// std/log — structured (JSON-line) logging. Each call emits one JSON object
// with a level, message, and ISO timestamp, plus any extra fields, to stdout
// (info/debug) or stderr (warn/error). Structured output is what log
// aggregators and observability pipelines consume, rather than free text.

export type Level = "debug" | "info" | "warn" | "error";

function emit(level: Level, message: string, fields: Record<string, unknown>): void {
  const entry: Record<string, unknown> = {
    level,
    msg: message,
    time: new Date().toISOString(),
  };
  for (const key of Object.keys(fields)) {
    entry[key] = fields[key];
  }
  const line = JSON.stringify(entry);
  if (level === "warn" || level === "error") {
    console.error(line);
  } else {
    console.log(line);
  }
}

export function debug(message: string): void {
  emit("debug", message, {});
}

export function info(message: string): void {
  emit("info", message, {});
}

export function warn(message: string): void {
  emit("warn", message, {});
}

export function error(message: string): void {
  emit("error", message, {});
}

// A log line carrying structured fields, e.g. `with_fields("info", "request",
// { path: "/", status: 200 })`.
export function with_fields(
  level: Level,
  message: string,
  fields: Record<string, unknown>,
): void {
  emit(level, message, fields);
}
