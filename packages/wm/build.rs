fn main() {
  // The version is read with `env!` at compile time, so a change to it has
  // to force a rebuild.
  println!("cargo:rerun-if-env-changed=VERSION_NUMBER");

  // No Windows resources are emitted here any more. This crate is a
  // library, and the binary that hosts it — the merged app in
  // `bar/packages/desktop` — carries the icon, version info and the
  // UIAccess/DPI manifest. Emitting them from both linked two `VERSION`
  // resources into one executable, which the resource compiler rejects
  // outright (`CVT1100: duplicate resource`).
}
