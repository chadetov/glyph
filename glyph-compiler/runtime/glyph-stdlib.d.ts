// Every v1 stdlib module now ships a runnable implementation as a `.ts` file
// under `std/` (resolved via the tsconfig `paths` mapping), so there are no
// type-only ambient declarations left here. `http` and `time`, previously
// type-only until their runtime wrappers landed, are now real modules. This
// file is intentionally empty; it is kept so the bundle layout is stable.
export {};
