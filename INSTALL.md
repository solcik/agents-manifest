# Installation

`agent-skills` needs Git at runtime.
Download archives from the [GitHub releases](https://github.com/solcik/agents-manifest/releases).
Choose a release that contains the archive for your platform:

| Platform | `target` |
| --- | --- |
| Linux x86-64 | `x86_64-unknown-linux-gnu` |
| Linux ARM64 | `aarch64-unknown-linux-gnu` |
| macOS Intel | `x86_64-apple-darwin` |
| macOS Apple silicon | `aarch64-apple-darwin` |

Release `v0.4.0` contains only the Linux x86-64 archive.
That binary requires glibc 2.39, which Ubuntu 24.04 provides.
Use the source build below on Ubuntu 22.04 until a newer release contains a compatible archive.

## Install a release archive

On Ubuntu, install the download tools and Git:

```sh
sudo apt-get update
sudo apt-get install -y ca-certificates curl git tar
```

Set `version` to a release tag that contains your target.
The example values install `v0.4.0` on x86-64 Ubuntu 24.04 or newer.
For another platform, replace both `version` and `target` before running the commands.

```sh
set -eu
version=v0.4.0
target=x86_64-unknown-linux-gnu
archive="agent-skills-$target.tar.gz"
mkdir -p "$HOME/Downloads/agent-skills-$version" "$HOME/.local/bin"
cd "$HOME/Downloads/agent-skills-$version"
curl -fL --retry 3 -O "https://github.com/solcik/agents-manifest/releases/download/$version/$archive"
curl -fL --retry 3 -O "https://github.com/solcik/agents-manifest/releases/download/$version/SHA256SUMS"
awk -v archive="$archive" '$2 == archive { found=1; print } END { if (!found) exit 1 }' SHA256SUMS > "$archive.sha256"
if [ "$(uname -s)" = Darwin ]; then
  shasum -a 256 -c "$archive.sha256"
else
  sha256sum -c "$archive.sha256"
fi
```

The checksum command must report `OK`.
Release `v0.4.0` has no attestation.
If the selected release provides an attestation, verify provenance with GitHub CLI before extraction:

```sh
gh attestation verify "$archive" --repo solcik/agents-manifest
```

Then install the binary:

```sh
tar -xzf "$archive"
install -m 755 agent-skills "$HOME/.local/bin/agent-skills"
export PATH="$HOME/.local/bin:$PATH"
agent-skills --version
```

Add the `PATH` export to your shell startup file for later sessions.

## Build from source

Install Rust 1.89 or newer through [rustup](https://rustup.rs/).
Install Git and a native C compiler.
On Ubuntu, run:

```sh
sudo apt-get update
sudo apt-get install -y build-essential ca-certificates curl git
```

On macOS, install the Xcode Command Line Tools with `xcode-select --install`.
Then build the selected release tag:

```sh
set -eu
version=v0.4.0
mkdir -p "$HOME/Downloads" "$HOME/.local/bin"
git clone --branch "$version" --depth 1 https://github.com/solcik/agents-manifest.git "$HOME/Downloads/agents-manifest-$version"
cd "$HOME/Downloads/agents-manifest-$version"
cargo build --release --locked
install -m 755 target/release/agent-skills "$HOME/.local/bin/agent-skills"
export PATH="$HOME/.local/bin:$PATH"
agent-skills --version
```

The [README](README.md) covers manifests and CLI commands.
