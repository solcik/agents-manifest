# Project skill verification result

The startup skill catalog contains `codebase-design`.
The harness exposes its path through the `r2` skill root.
The expanded catalog path is `/home/solcik/dev/github/solcik/agents-manifest/manifest-v1/.agents/skills/codebase-design/SKILL.md`.
This discovery comes from the startup catalog, before any filesystem read.
The later manual read confirms the installed content.
File existence alone does not prove startup discovery.

The user selected Codex, openai-codex, gpt-5.6-sol, low reasoning, and dangerous mode.
These route details come from the user request.
This assessment does not independently verify the runtime model.

## Skill use

I read the complete generated [SKILL.md](../.agents/skills/codebase-design/SKILL.md) before the design assessment.
I also read the complete [DEEPENING.md](../.agents/skills/codebase-design/DEEPENING.md).
I also read the complete [DESIGN-IT-TWICE.md](../.agents/skills/codebase-design/DESIGN-IT-TWICE.md).
These references link back to the same skill vocabulary.
The alternative-interface workflow applies when the user requests alternative designs.
This verification request does not activate that workflow.
I did not start sub-agents.

The skill defines depth as leverage at the interface.
I assessed caller knowledge, hidden behavior, and invariants instead of implementation length.
I used the deletion test to assess `Transaction`.
I included ordering constraints and error modes in its interface.
I used the two-adapter rule to assess the `Source` seam.
I used the test-surface principle to select recovery evidence.

## SourceReference

Evidence: [src/manifest.rs](../src/manifest.rs), lines 45–74 and 169–204.
`SourceReference` is a Value Object with private identity fields.
Its constructor validates the source, revision, and path before construction.
Its constructor normalizes revision case.
Deserialization uses the same constructor through `TryFrom<RawSourceReference>`.
The interface derives cache keys and Git object selectors from the validated identity.
Evidence: [src/manifest.rs](../src/manifest.rs), lines 319–366.
Reference validation rejects unsafe paths, unsupported source forms, and incomplete hexadecimal revisions.
This design gives locality to identity validation.
Callers do not need to repeat those checks for each constructed reference.

## Source

Evidence: [src/source.rs](../src/source.rs), lines 44–47 and 306–332.
`Source` defines an interface with two operations: `file` and `skill`.
The `Sync` requirement forms part of that interface.
`GitSource` supplies the production adapter at this seam.
Evidence: [tests/synchronisation.rs](../tests/synchronisation.rs), lines 16–61.
`MemorySource` supplies a test adapter for the same interface.
Two concrete adapters make this a real seam under the skill's terminology.
Evidence: [src/source.rs](../src/source.rs), lines 73–201 and 209–304.
The Git adapter hides command configuration, timeout handling, cache coordination, and revision verification.
The production dependency uses external repositories.
Tests substitute an in-memory adapter instead of remote access.
This arrangement follows the skill's external-dependency guidance.

## Planner

Evidence: [src/plan.rs](../src/plan.rs), lines 197–367 and 369–423.
`Planner` accepts a validated manifest and a `Source` dependency.
Its interface offers construction, worker configuration, check configuration, and `build`.
Its implementation resolves bundles, checks collisions, bounds content, reconciles outputs, and derives publication operations.
The worker count must remain between 1 and 64.
The `check` flag changes how reconciliation treats local changes.
Evidence: [src/plan.rs](../src/plan.rs), lines 167–194.
These constraints belong to the interface despite their absence from the constructor's types.
`Planner` returns a `Plan` instead of publishing generated outputs.
Evidence: [src/cli.rs](../src/cli.rs), lines 127–143.
The CLI passes that plan to `Transaction` only for synchronization.

## Deep module: Transaction

Evidence: [src/transaction.rs](../src/transaction.rs), lines 79–215.
`Transaction` is a deep module for recoverable filesystem publication.
Its interface has three entry points: `new`, `recover`, and `apply`.
Construction requires an exclusive `ProjectLock` capability.
The caller must recover an existing journal before applying a nonempty plan.
The implementation stages content, verifies fingerprints, writes a durable journal, replaces outputs, and records commitment.
Evidence: [src/transaction.rs](../src/transaction.rs), lines 217–356.
Private methods hide replacement details, journal validation, rollback, and cleanup.
Recovery preserves conflicting edits and backups through explicit conflict errors.
These error modes remain part of the interface.

The deletion test supports this depth assessment.
Without this module, callers must implement staging, journal durability, rollback, and conflict preservation.
Its interface gives leverage through one publication operation and one recovery operation.
Its implementation gives locality to recovery rules.
The established Unit of Work pattern describes its publication responsibility.

Evidence: [src/transaction.rs](../src/transaction.rs), lines 436–481.
Existing tests call `recover` through the interface.
They cover interrupted replacement phases, repeated recovery, committed output, and preservation of user edits.
I inspected these tests but did not execute them.
Local filesystem dependencies use temporary directories in these tests.
They do not require an additional external seam.

## Shallow interface risk: mutable Plan records

Evidence: [src/plan.rs](../src/plan.rs), lines 97–109 and 125–159.
`Plan` exposes mutable `operations` and `report` fields.
`Operation` exposes its path, fingerprints, and payload independently.
The private `Plan::new` derives the report from operations.
Public fields let callers bypass that relationship after construction.
Callers can also assemble records with inconsistent payloads and fingerprints.

Evidence: [src/transaction.rs](../src/transaction.rs), lines 121–177 and 259–286.
`Transaction` checks ownership, journal constraints, and staged fingerprints before replacement.
These checks reduce publication risk from inconsistent records.
They do not restore report consistency.
Evidence: [src/cli.rs](../src/cli.rs), lines 133–140.
Publication consumes `plan.operations`, while output reports consume `plan.report`.
Therefore, inconsistent records can describe different changes from those selected for publication.
This is an interface risk, not an observed CLI failure.
The current CLI uses the planner's derived report.

The skill expands the interface beyond public methods.
That definition exposes the caller's obligation to preserve relationships among these records.
The records offer limited behavior relative to those obligations.
This creates a shallow interface risk beside otherwise deep modules.
Private records with read-only access can concentrate those invariants inside the module.
This is a design recommendation only.

## Other startup skills

The startup catalog also exposes unrelated global skills.
Their catalog roots remain distinct from the project skill root.

| Skill | Expanded catalog path |
| --- | --- |
| imagegen | `/home/solcik/.codex/skills/.system/imagegen/SKILL.md` |
| openai-docs | `/home/solcik/.codex/skills/.system/openai-docs/SKILL.md` |
| plugin-creator | `/home/solcik/.codex/skills/.system/plugin-creator/SKILL.md` |
| skill-creator | `/home/solcik/.codex/skills/.system/skill-creator/SKILL.md` |
| skill-installer | `/home/solcik/.codex/skills/.system/skill-installer/SKILL.md` |
| plugin-management:plugin-management | `/home/solcik/.codex/plugins/cache/openai-curated-remote/plugin-management/0.1.0/skills/plugin-management/SKILL.md` |

I did not apply these unrelated skills.
Project skill discovery does not remove global catalog entries.

## Verification limits

I inspected `.envrc`, `devenv.nix`, and `Cargo.toml`.
The project declares `skills:check` as `cargo run --locked -- check --offline --json`.
Its documented entry point is `direnv exec . devenv tasks run skills:check`.
I did not run this optional command.
Environment activation and Cargo execution can write caches or build files outside the permitted report.
No offline-check result is claimed.

I did not read secret files or print credentials.
I did not change source code, manifests, generated skills, or HOME.
The workspace already contained modified `.gitignore` and `devenv.nix` files.
It also contained untracked `.agents/` content and project skill documentation.
I preserved those existing changes.
I wrote only this report through `apply_patch`.
