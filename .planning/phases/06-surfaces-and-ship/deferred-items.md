## From 6-01 (out of scope — pre-existing, not caused by this plan)

- `sync::passphrase::read_from_file(&Path)` (src/sync/passphrase.rs:213) has **no
  caller anywhere in `src/`** outside its own test module. It is a complete,
  mode-checking (`0o077`) password-file reader with no command wired to it.
  Material to 6-02: the sync password today arrives on **stdin only**
  (`cli.rs:512`, `cli.rs:820`), and `local_keyfile` *refuses* when stdin is a
  terminal. A menu-bar surface that wants to push therefore has no non-stdin
  password path — this dead seam is the closest thing to one. 6-02 should
  decide deliberately: wire it, or refuse the action with "run it in a
  terminal" (D-02).
