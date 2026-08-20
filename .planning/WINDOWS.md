---
schema_version: 1
open_count: 3
waived_count: 0
fixed_count: 0
total_count: 3
last_updated: 2026-08-20T04:56:48.085Z
---

# Broken Windows Ledger

> Cross-phase defect register. `/gsd-ship` blocks while `open_count > 0`.
> Waive with `gsd-tools windows waive <id> "<reason>"` (reason required).
> Mark fixed with `gsd-tools windows fixed <id>`.

| id | phase | kind | file | line | description | status | reason | recorded_at | resolved_at |
|----|-------|------|------|------|-------------|--------|--------|-------------|-------------|
| 1 | 5 | unrun-verify | tests/live.rs |  | CAL-1 (does a private-repo release asset honour Range:) is still unrun after four phases — needs a real private repo and token | open |  | 2026-08-20T04:56:47.947Z |  |
| 2 | 5 | unrun-verify | tests/live.rs |  | CAL-5 (a torn upload's state, and whether digest is populated) is still unrun — needs a real private repo and a Contents:write token | open |  | 2026-08-20T04:56:48.017Z |  |
| 3 | 5 | deviation | README.md |  | README's Sync section does not mention sync pull; 5-08 was scoped away from README.md | open |  | 2026-08-20T04:56:48.085Z |  |

````json
[
  {
    "id": 1,
    "kind": "unrun-verify",
    "phase": "5",
    "file": "tests/live.rs",
    "line": null,
    "description": "CAL-1 (does a private-repo release asset honour Range:) is still unrun after four phases — needs a real private repo and token",
    "status": "open",
    "reason": "",
    "recorded_at": "2026-08-20T04:56:47.947Z",
    "resolved_at": null
  },
  {
    "id": 2,
    "kind": "unrun-verify",
    "phase": "5",
    "file": "tests/live.rs",
    "line": null,
    "description": "CAL-5 (a torn upload's state, and whether digest is populated) is still unrun — needs a real private repo and a Contents:write token",
    "status": "open",
    "reason": "",
    "recorded_at": "2026-08-20T04:56:48.017Z",
    "resolved_at": null
  },
  {
    "id": 3,
    "kind": "deviation",
    "phase": "5",
    "file": "README.md",
    "line": null,
    "description": "README's Sync section does not mention sync pull; 5-08 was scoped away from README.md",
    "status": "open",
    "reason": "",
    "recorded_at": "2026-08-20T04:56:48.085Z",
    "resolved_at": null
  }
]
````
