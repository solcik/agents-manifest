# Releases

## Workflow

[Release-plz](https://release-plz.dev/docs) derives versions from commits and generates changelogs through git-cliff.
The repository releases GitHub binaries, not crates.io packages.
`release-plz.toml` sets `git_only = true` because Cargo sets `publish = false`.

1. Merge feature and fix commits into `main`.
2. Review the automated release PR.
3. Approve its checks when GitHub requires workflow approval.
4. Merge the release PR.

Step 3 disappears once the repository holds a `RELEASE_PLZ_TOKEN` secret.
Add a fine-grained personal access token for this repository.
Give it write access to contents, pull requests, and workflows.
The `release-pr` job then opens the pull request as that person.
GitHub starts the checks without approval.
Without the secret the job falls back to `github.token`.
A maintainer then approves each run from the pull request page.

Automation creates the version tag and a draft GitHub release.
The reusable binary workflow runs formatting, Clippy, and tests.
It builds the Linux binary and generates SHA256SUMS.
It publishes the draft after all checks pass.
No workflow automatically merges release PRs.

## Commit and version rules

[`committed`](https://github.com/crate-ci/committed) checks local commit messages and new PR commits.
[`action-semantic-pull-request`](https://github.com/amannn/action-semantic-pull-request) validates PR titles before squash merges.
The title workflow reads metadata only.
It never checks out a pull request and never executes pull request code.
It runs on `pull_request`, because `pull_request_target` never starts for a release pull request.
Both checks permit the types listed below, plus `build`, `revert`, and `refactor`.
Commit summaries have a 100-character limit.

Use Conventional Commits for new changes:

| Commit | Effect |
| --- | --- |
| `feat: add capability` | Minor version increment |
| `fix: correct behavior` | Patch version increment |
| `perf: reduce object reads` | Patch version increment |
| `refactor: extract adapter` | Patch version increment |
| `feat!: remove an option` | Breaking change |
| `docs: explain options` | No release alone |
| `test: cover recovery` | No release alone |
| `ci: update checks` | No release alone |
| `chore: maintain tooling` | No release alone |

Breaking changes increment the minor version before 1.0.0.
After 1.0.0, breaking changes increment the major version.
Use `!` or a `BREAKING CHANGE:` footer to identify incompatible CLI behavior.
Library API checks cannot identify all CLI incompatibilities.
Reviewers must check arguments, exit statuses, output schemas, and manifest compatibility.
The first release uses the existing Cargo version, 0.1.0.
Older commits remain in the changelog without a history rewrite.
Maintenance commits appear when a release-worthy change creates a release.

## Contributor attribution

The generated changelog lists GitHub usernames from release commits.
Release notes use the same changelog body and contributor mentions.
The contributor list uses GitHub usernames, not email addresses.
Keep PR references in merged commit messages, such as `(#42)`.
GitHub adds this reference during a squash merge.
Use Conventional Commit prefixes in PR titles.
Release-plz resolves commit authors through the GitHub API when the template references `remote.username`.
This attribution does not list every reviewer or commit coauthor.
If GitHub returns no contributors, the template omits the contributor section.
Do not insert guessed usernames.
The list removes duplicate usernames and sorts the result.
The [upstream enrichment code](https://github.com/release-plz/release-plz/blob/main/crates/release_plz_core/src/changelog_filler.rs) defines the required template reference.

## Repository setup

Enable Actions under the repository settings.
Enable “Allow GitHub Actions to create and approve pull requests”.
The workflow requests explicit token permissions for each job.
It uses `GITHUB_TOKEN` without a personal access token.
Merge the automation into `main` before its first run.

GitHub does not run tag-push workflows for tags created with `GITHUB_TOKEN`.
GitHub also does not start `pull_request_target` for a pull request that GitHub Actions opens or updates.
A required check on that event stays unreported and blocks every release pull request.
The release workflow therefore calls the binary workflow directly.
Action references use version tags.
Release assets currently target Linux only.
The repository does not use a second version or changelog engine.
`commitlint` requires a separate Node development toolchain and duplicates the selected commit checks.
`cargo-dist` remains deferred until the project needs installers or additional distribution targets.
The current binary workflow publishes one Linux archive and checksums.
After these workflows merge, add `Validate PR title` and `Validate commit messages` to the required branch checks.

## Failed publication

If the binary workflow fails, the GitHub release remains a draft.
Correct the failure before a retry.
Run the “Release binaries” workflow with the existing version tag.
The retry requires the tag to match the Cargo package version.
It replaces draft assets and preserves the generated release notes.
The workflow refuses to overwrite a published release.
Do not delete tags to restart publication.
