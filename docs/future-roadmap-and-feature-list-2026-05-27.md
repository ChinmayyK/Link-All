# Link All Future Roadmap and Feature List

Date: May 27, 2026

## Purpose

This document turns the current audit context into a forward-looking roadmap.

It is meant to answer five planning questions:

- what we should build next
- what we should fix before adding more surface area
- which features are core roadmap items versus stretch bets
- where platform parity matters most
- how to sequence the work so the product becomes easier to trust, easier to use, and more resilient

This roadmap was derived from the repo state plus a set of 2026-05-27 audit
documents that have since been removed from the tree. They remain available in
git history:

```
git show v1.2.8:docs/full-stack-friction-fault-tolerance-connectivity-audit-2026-05-27.md
git show v1.2.8:docs/functionality-audit-2026-05-27.md
git show v1.2.8:docs/deep-ui-ux-journey-audit-2026-05-27.md
git show v1.2.8:docs/cross-platform-feature-parity-audit-2026-05-27.md
```


## Product Direction

Link All should keep leaning into the same core thesis:

- local-first
- zero-cloud
- cross-platform
- timeline-first when appropriate
- explicit trust
- premium native feel

The next phase should not optimize for feature count.

It should optimize for:

1. trustworthiness
2. recoverability
3. predictable file and clipboard behavior
4. platform parity on the core workflows
5. connectivity that feels durable under real network conditions

## Roadmap Principles

### 1. Foundations before expansion

If a feature makes the product look more capable but also makes the system harder to trust, it should wait.

### 2. One mental model per workflow

Users should not have to learn multiple variants of:

- pairing
- sending
- receiving
- recovering

### 3. Shared core, thin platform interpretation

The Rust core should own:

- health state
- trust state
- delivery state
- reconnect state
- transfer state

Platform apps should present those states, not reinvent them.

### 4. Feature freeze on non-core experiments until baseline reliability is stronger

New experimental features should be deprioritized until:

- trust is consistent
- Windows command/backend drift is fixed
- recovery UX is strong
- transfer status is accurate

## Executive Priority Order

If we need a single prioritized list for planning, this should be it:

1. Windows contract and trust fixes
2. Shared system health state end-to-end
3. File-transfer correctness and adaptive reliability
4. One canonical pairing model
5. Repair-first diagnostics on every GUI platform
6. Platform parity for core clipboard/file workflows
7. Better onboarding and re-entry
8. Admin, metrics, and support tooling
9. Power-user and ecosystem features
10. Experimental features

## Roadmap Phases

## Phase 1: Reliability Foundation

Target outcome:

- the core workflows stop lying
- the major contract mismatches are removed
- users can recover from the most common failure modes

### Must-ship items

- Fix the Windows IPC/backend contract drift.
- Fix the Windows trust approval flow to use device IDs and pending pairing state.
- Implement full `SystemHealthState` emission and consumption across core, Android, macOS, and Windows.
- Correct file-transfer progress math and ACK sizing.
- Wire adaptive link telemetry into the transfer path.
- Remove or isolate implicit trust upgrade behavior.
- Fix the macOS clipboard-image send command mismatch.
- Fix Android file receive destination handling and post-download open behavior.

### User-facing features in this phase

- reasoned connectivity statuses
- real repair actions in diagnostics
- accurate transfer progress, pause, resume, and completion states
- predictable trust prompts

### Exit criteria

- no major UI action points to a stale backend command
- users can tell why sync is not working
- file transfers report truthfully and recover predictably
- pairing behavior is conceptually consistent

## Phase 2: Core Workflow Parity

Target outcome:

- the main Link All promise feels equally strong on every supported GUI platform

### Must-ship items

- Standardize file-send flows around path/URI-based transfer wherever possible.
- Standardize incoming file destination behavior across platforms.
- Bring Windows diagnostics to parity with macOS and Android.
- Add timeline item pin/delete parity where missing.
- Add peer-specific send targeting consistently across platforms.
- Make onboarding proof-based and target-device-scoped.
- Add a clear "Finish setup" re-entry path for users who skip onboarding.

### User-facing features in this phase

- same mental model for pairing on macOS, Android, and Windows
- same mental model for "send clipboard", "send file", and "re-send from history"
- consistent receive locations and completion notifications
- first-run flows that verify real success, not just UI progression

### Exit criteria

- core clipboard/file workflows are comparable across macOS, Android, and Windows
- onboarding proves real success
- Windows no longer feels like a partial implementation

## Phase 3: Product Maturity

Target outcome:

- Link All feels like a polished, dependable cross-device utility rather than a smart technical tool

### Must-ship items

- Add device-health and recovery summaries to the dashboard.
- Add support/export bundle for diagnostics and bug reports.
- Add transfer history with better metadata and search.
- Add connectivity history and last-known failure reasons.
- Add richer per-device settings:
  - auto-apply policy
  - sync pause
  - auto-connect
  - trust details
  - preferred receive behavior

### User-facing features in this phase

- "why did this fail?" visibility
- easier support/debug flows
- stronger control over how each device behaves

### Exit criteria

- the app explains its own state well
- debugging common failures does not require code or logs
- the product is configurable without becoming confusing

## Phase 4: Expansion and Differentiation

Target outcome:

- Link All starts winning not only on reliability, but on delight and unique value

### Candidate items

- cross-device quick context on more platforms
- richer clipboard templates and snippets
- drag-to-device targeting
- shared pasteboard collections
- smarter file receive routing
- device groups and send scopes
- continuity features beyond clipboard/files
- enterprise/power-user controls

### Rule for this phase

Do not start these until Phases 1 and 2 are substantively complete.

## Feature List by Area

## A. Connectivity and Recovery

### Immediate

- Shared `SystemHealthState` lifecycle
- explicit reconnect states
- clear "discovery vs trust vs transport" breakdown
- network-change recovery surfacing
- diagnostics CTAs that actually repair

### Next

- connectivity history timeline
- last-known-good endpoint visibility
- reconnect cooldown/circuit-breaker UI
- inline firewall / local-network guidance

### Later

- support bundle export
- self-check wizard
- network quality score in UI

## B. Trust and Pairing

### Immediate

- one pairing model only
- no implicit trust upgrade in normal flows
- device-ID-based trust actions everywhere
- shared SAS/PIN ceremony

### Next

- canonical identity card on every platform
- full fingerprint expand/copy/share flows
- clearer paired/unpaired/pending states

### Later

- trusted device groups
- advanced security mode for stricter re-verification

## C. Clipboard Experience

### Immediate

- accurate delivery/applied states
- consistent timeline-first behavior
- fix platform-specific send mismatches

### Next

- better pending/applied indicators
- per-device auto-apply policy UI
- send target clarity
- richer clipboard previews

### Later

- saved snippets/templates UI
- smart actions on clipboard content types
- device-aware clipboard routing rules

## D. File Transfer

### Immediate

- fix progress math
- fix ACK size policy
- real adaptive chunk sizing
- consistent completion/open behavior
- standardize receive destination

### Next

- transfer queue UI
- transfer retry affordances
- richer progress detail:
  - speed
  - ETA
  - reconnecting
  - verifying

### Later

- folder send support
- resume after app restart
- better duplicate-file naming and conflict strategy
- transfer prioritization

## E. Diagnostics and Repair

### Immediate

- repair-first diagnostics surfaces
- one primary fix per failure state
- direct rescan/restart/reconnect actions

### Next

- proactive warnings before failure
- guided repair checklists
- OEM-specific Android restriction education

### Later

- automated self-healing suggestions
- exportable diagnostic snapshot

## F. Platform Parity

### Immediate

- Windows parity for diagnostics and trust
- macOS command/backend alignment
- Android large-file path improvement

### Next

- Windows timeline management parity
- Android/macOS/Windows pairing parity
- shared receive-location messaging

### Later

- Linux parity decision:
  - headless-first product, or
  - GUI parity investment

## G. Power User and Admin Features

### Next

- support bundle export
- richer CLI diagnostics
- per-peer policy editing
- connectivity metrics

### Later

- team/admin mode
- config sync/import-export
- managed device profiles

## Platform-Specific Roadmap

## macOS

### Priority

- fix clipboard-image IPC mismatch
- improve state truthfulness in dashboard
- strengthen diagnostics from passive to repair-first
- preserve leadership as the reference desktop experience

### Future features

- richer quick context targeting
- stronger menu-bar recovery flows
- better transfer dashboard

## Android

### Priority

- surface health state
- improve file receive/open correctness
- move large-file flows toward URI/path-based transfer
- make onboarding truly verified

### Future features

- better restricted-background education
- richer incoming transfer actions
- stronger per-device control UI

## Windows

### Priority

- fix IPC contract drift
- fix trust flow
- fix devices/status parsing
- move large-file sending to path-based transfer
- stabilize diagnostics as a real recovery surface

### Future features

- timeline pin/delete parity
- stronger tray-to-dashboard continuity
- re-evaluate camera broadcast only after baseline parity

## Linux

### Priority

- decide product stance clearly

### If Linux remains headless-first

- improve CLI repair workflows
- improve notifications and diagnostics output
- improve docs for pairing and recovery

### If Linux becomes a parity client

- build a minimal tray + diagnostics + device-management shell

## Feature Freeze and Deprioritized Items

The following should be deprioritized until the foundation phases are stronger:

- new experimental continuity features
- broader camera-stream expansion
- visually ambitious but behaviorally shallow UI work
- platform-specific side bets that widen parity drift

## Not-Now List

These are valid ideas, but should not be treated as near-term roadmap work:

- cloud relay modes
- account systems
- heavy collaboration features
- multi-user shared spaces
- enterprise packaging beyond diagnostics/admin basics

They would dilute the current local-first advantage before the core utility is fully hardened.

## Suggested Delivery Milestones

## Milestone 1

Theme:

- "Trust the basics"

Deliver:

- Windows contract fixes
- Windows trust fix
- health-state model end-to-end
- transfer math fixes
- macOS IPC mismatch fixes

## Milestone 2

Theme:

- "Recover without guessing"

Deliver:

- diagnostics repair actions
- standardized receive locations
- onboarding verification upgrade
- reconnect-state visibility

## Milestone 3

Theme:

- "Parity on daily workflows"

Deliver:

- Windows diagnostics parity
- timeline parity
- consistent send-target behavior
- consistent file-send architecture

## Milestone 4

Theme:

- "Polish and control"

Deliver:

- per-device policies
- support bundle export
- richer transfer and connectivity history

## Definition of Done for Roadmap Work

A roadmap item should be considered done only when:

- the core contract exists
- every intended platform shell consumes that contract correctly
- the UI copy matches actual system truth
- the recovery path is explicit
- the workflow is covered by at least one smoke/integration test where appropriate

## Success Metrics

We should evaluate this roadmap against product outcomes, not just merged PRs.

Key success signals:

- fewer silent failure modes
- higher first-run completion with real verified success
- fewer repeated trust prompts
- better reconnect success after Wi-Fi changes
- fewer file-transfer retries caused by misleading status or path confusion
- reduced platform drift across macOS, Android, and Windows

## Bottom Line

The next roadmap for Link All should be about making the product dependable first, then expansive.

That means:

- fewer conceptual models
- tighter platform contracts
- stronger diagnostics
- better transfer truthfulness
- clearer recovery

If we execute in that order, Link All can become not just a technically impressive cross-device tool, but a product people are comfortable relying on every day.
