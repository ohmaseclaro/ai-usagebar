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

## From 6-07 (out of scope — pre-existing, not caused by this plan)

- `cli::rekey` (src/sync/cli.rs) calls `prompt.passphrase("")` **twice** — once
  for "The CURRENT sync password" and once for "The NEW sync password". Both
  therefore print `TtyPrompt`'s *"Press Enter to take the generated
  passphrase, or type your own now."*, which is wrong at both: neither ask has
  a generated alternative, and at the CURRENT one an empty answer is submitted
  as the password. 6-07 added `SetupPrompt::existing_passphrase` for exactly
  this shape of ask and it fits the CURRENT one verbatim; the NEW one wants a
  third variant or a `what: &str` label. Left alone because a sibling plan is
  editing narration in this area and a half-fix reads worse than none.
