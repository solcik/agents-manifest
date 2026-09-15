{pkgs, ...}: {
  packages = [pkgs.cargo pkgs.rustc pkgs.rustfmt pkgs.clippy pkgs.git pkgs.hyperfine pkgs.alejandra pkgs.actionlint pkgs.release-plz pkgs.committed];

  git-hooks.hooks.committed = {
    enable = true;
    name = "Conventional commit message";
    entry = "${pkgs.committed}/bin/committed --config committed.toml --commit-file";
    stages = ["commit-msg"];
  };

  tasks = {
    "commits:check" = {
      exec = ''
        set -euo pipefail
        git log -1 --format=%B | committed --config committed.toml --commit-file -
      '';
      showOutput = true;
    };
    "quality:format".exec = "cargo fmt && alejandra devenv.nix";
    "quality:lint" = {
      exec = "cargo fmt --check && alejandra --check devenv.nix && cargo clippy --locked --all-targets -- -D warnings && actionlint";
      after = ["quality:format"];
      showOutput = true;
    };
    "test:all" = {
      exec = "cargo test --locked";
      after = ["quality:lint"];
      showOutput = true;
    };
    "manifest:validate" = {
      exec = "cargo run -- validate examples/skills.yaml";
      after = ["test:all"];
    };
    "skills:sync" = {
      exec = "cargo run --locked -- sync --json";
      showOutput = true;
    };
    "skills:check" = {
      exec = "cargo run --locked -- check --offline --json";
      showOutput = true;
    };
    "bench:core" = {
      exec = "cargo bench --bench core -- --sample-count 100";
      showOutput = true;
    };
    "bench:cli" = {
      showOutput = true;
      exec = ''
        cargo build --release
        hyperfine --shell=none --warmup 5 --runs 100 'target/release/agent-skills --help' \
          'target/release/agent-skills validate examples/skills.yaml --quiet'
      '';
    };
  };

  enterTest = ''
    cargo fmt --check
    alejandra --check devenv.nix
    cargo clippy --locked --all-targets -- -D warnings
    actionlint
    cargo test --locked
  '';
}
