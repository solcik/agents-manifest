# Releases

## Workflow

[Release-plz](https://release-plz.dev/docs) derives versions from commits and generates changelogs through git-cliff.
The repository releases GitHub binaries, not crates.io packages.
`release-plz.toml` sets `git_only = true` because Cargo sets `publish = false`.

1. Merge feature and fix commits into `main`.
2. Review the automated release PR.
3. Approve its checks when GitHub requires workflow approval.
4. Merge the release PR.

Automation creates the version tag and a draft GitHub release.
The reusable binary workflow runs formatting, Clippy, and tests.
It builds the Linux binary and generates SHA256SUMS.
It publishes the draft after all checks pass.
No workflow automatically merges release PRs.

## Commit and version rules

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

## Repository setup

Enable Actions under the repository settings.
Enable “Allow GitHub Actions to create and approve pull requests”.
The workflow requests explicit token permissions for each job.
It uses `GITHUB_TOKEN` without a personal access token.
Merge the automation into `main` before its first run.

GitHub does not run tag-push workflows for tags created with `GITHUB_TOKEN`.
The release workflow therefore calls the binary workflow directly.
Action references use version tags.
Release assets currently target Linux only.

## Failed publication

If the binary workflow fails, the GitHub release remains a draft.
Correct the failure before a retry.
Run the “Release binaries” workflow with the existing version tag.
The retry requires the tag to match the Cargo package version.
It replaces draft assets and preserves the generated release notes.
The workflow refuses to overwrite a published release.
Do not delete tags to restart publication.
