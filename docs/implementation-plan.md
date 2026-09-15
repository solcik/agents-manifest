# Project skills implementation plan

Date: 2026-09-15.

Specification: [project skill synchronisation](specification.md).

## Implementation status

Steps 1 through 7 have local implementations.
The public repository is `solcik/agents-manifest`.
The project uses its own devenv environment.
The local test suite passes 39 tests.
CI defines Linux checks with stable Rust and Rust 1.89.
macOS CI and release builds remain deferred for now.
Release publication remains a manual draft-release workflow.

Hermes discovery remains unverified.
The CLI rejects Hermes and `all` targets.
Repository registration exists in the NixOS feature lane.
Host packaging and the existing command migration remain deferred.

## Step 1: define the contract

Write the version one specification.
Implement a strict manifest JSON Schema.
Record target isolation limitations and unresolved Hermes discovery behavior.
Validate the schema against accepted and rejected examples.

## Step 2: establish the portable repository

Confirm the public CLI repository name.
Create its repository container with the host Git wrappers.
Read its repository instructions before edits.
Create a feature lane within that container.
Scaffold a project-owned Rust development environment.
Copy the specification, schema, and plan into the CLI repository.
Keep curated content in `solcik/agent-skills`.

## Step 3: implement manifest policy

Add Rust types with unknown-field rejection.
Implement URL, revision, path, target, and name validation.
Implement `validate` without network access.
Test invalid revisions, parent paths, duplicate names, and target widening.
Test the documented exit statuses.

## Step 4: implement source resolution

Add a Git transport adapter with noninteractive authentication.
Support scoped host wrappers without embedding credential handling.
Cache repositories by source identity and immutable revision.
Resolve bundle files and selected skill trees.
Reject nested bundles, symlinks, submodules, and invalid frontmatter.
Use temporary local repositories for transport tests.

## Step 5: implement the planner

Expand bundle entries into one desired skill set.
Detect all name and filesystem collisions.
Inspect tracked files and ownership hashes.
Implement `plan` and deterministic content hashes.
Verify Hermes paths against pinned source before enabling its adapter.
Reject unsupported target isolation.

## Step 6: implement safe synchronisation

Add project locks, staging, publication journals, and recovery.
Publish canonical files and supported harness copies.
Maintain exact ignore patterns and ownership metadata.
Remove only unmodified owned output.
Test stale output, user edits, retrieval failure, and interrupted publication.

## Step 7: implement checks and distribution

Implement online and offline `check` modes.
Add Linux CI checks.
Defer macOS builds until the user requests them.
Run formatting, linting, and acceptance tests.
Prepare release binaries and checksum generation.
Document installation and an opt-in devenv task.

## Step 8: integrate NixOS

Rename the host mute command through an explicit compatibility migration.
Package the portable CLI from an immutable repository revision.
Register the CLI repository for git-sync and agent directory access.
Preserve personal global reconciliation as a separate host responsibility.
Run focused checks and all required publication checks.

## Later work

Add skills.sh discovery imports after the core contract passes acceptance tests.
Add private bundle documentation after private transport tests pass.
Add `always` adapters only after each harness's behavior has a verified contract.
Define global bundles separately from project declarations.
