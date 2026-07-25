// std/path — cross-platform filesystem paths, over node's `path`. Segment
// joining, the directory/base/extension split, and normalization all respect
// the host separator, so the same Glyph code runs on Unix and Windows.

import {
  basename as nodeBasename,
  dirname as nodeDirname,
  extname as nodeExtname,
  isAbsolute as nodeIsAbsolute,
  join as nodeJoin,
  normalize as nodeNormalize,
  relative as nodeRelative,
} from "node:path";

export function join(parts: ReadonlyArray<string>): string {
  return nodeJoin(...parts);
}

export function dirname(p: string): string {
  return nodeDirname(p);
}

export function basename(p: string): string {
  return nodeBasename(p);
}

export function extname(p: string): string {
  return nodeExtname(p);
}

export function is_absolute(p: string): boolean {
  return nodeIsAbsolute(p);
}

export function normalize(p: string): string {
  return nodeNormalize(p);
}

export function relative(from: string, to: string): string {
  return nodeRelative(from, to);
}
