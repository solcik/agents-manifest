# Project skill verification

Route: Codex, openai-codex, gpt-5.6-sol, low reasoning.
The user selected this route and dangerous mode.

This is a read-only discovery and skill-use test.
Do not change source code, manifests, generated skills, or HOME.
Write only docs/project-skill-verification-result.md through apply_patch.

The Rust CLI installed codebase-design from the pinned project manifest.
The generated skill lives at .agents/skills/codebase-design/SKILL.md.
Use $codebase-design to inspect the CLI design.
Read the complete generated SKILL.md before applying its instructions.
Read every reference that the skill requires.

First report whether the startup skill catalog contains codebase-design.
Record its catalog path if the harness exposes that path.
Distinguish catalog discovery from a manual file read.
Do not claim discovery solely because the file exists.

Inspect SourceReference, Source, Planner, and Transaction.
Apply the skill terminology to one deep module and one shallow interface risk.
Record exact source paths and evidence for each conclusion.
Explain how the skill influenced your assessment.
Report any unrelated global skills still visible in the startup catalog.
Run the project CLI offline check through direnv exec if useful.
Never print credentials or read secret files.
Follow repository instructions and write sentences with at most 20 words.
