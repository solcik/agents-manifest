# Global catalog verification

Route: Codex, openai-codex, gpt-5.6-sol, low reasoning.
The user requested post-rebuild verification on the HP notebook.
This session runs in the NixOS repository without temporary skill overrides.
Do not change the NixOS repository or HOME.
Do not commit, push, or start agents.

Inspect the startup skill catalog before reading any skill file.
Report every available skill and its declared path.
If the catalog is empty, report that exact observation.
Do not claim file absence solely from catalog absence.
Run hostname and readlink /run/current-system.
Run command -v agent-skills and agent-skills --version.
Check whether the three global discovery roots exist.
The roots are /home/solcik/.agents/skills, /home/solcik/.claude/skills, and /home/solcik/.config/opencode/skills.
Do not read configuration secrets or credentials.

Write only this external verification report through apply_patch:
/home/solcik/dev/github/solcik/agents-manifest/manifest-v1/docs/post-rebuild-global-catalog-result.md
Use sentences with at most 20 words.
