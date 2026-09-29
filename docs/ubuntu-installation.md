# Install agent-skills on Ubuntu

The `v0.4.0` release provides one `x86_64` Linux archive.
Its binary requires glibc 2.39, which Ubuntu 24.04 provides.
Use the source build below for Ubuntu 22.04 or ARM64.

## Ubuntu 24.04 or newer on x86-64

Install the download tools and Git:

```sh
sudo apt-get update
sudo apt-get install -y ca-certificates curl git tar
uname -m
```

Continue only if `uname -m` prints `x86_64`.
Download the [v0.4.0 release](https://github.com/solcik/agents-manifest/releases/tag/v0.4.0) and verify its checksum:

```sh
set -e
version=v0.4.0
archive=agent-skills-x86_64-unknown-linux-gnu.tar.gz
mkdir -p "$HOME/Downloads/agent-skills-$version" "$HOME/.local/bin"
cd "$HOME/Downloads/agent-skills-$version"
curl -fL --retry 3 -O "https://github.com/solcik/agents-manifest/releases/download/$version/$archive"
curl -fL --retry 3 -O "https://github.com/solcik/agents-manifest/releases/download/$version/SHA256SUMS"
sha256sum -c SHA256SUMS
tar -xzf "$archive"
install -m 755 agent-skills "$HOME/.local/bin/agent-skills"
export PATH="$HOME/.local/bin:$PATH"
agent-skills --version
```

The checksum command must report `OK`.
Add the `PATH` export to your shell startup file for later sessions.

## Ubuntu 22.04 or ARM64

Install the native build tools:

```sh
sudo apt-get update
sudo apt-get install -y build-essential ca-certificates curl git
```

Install Rust 1.89 or newer through [rustup](https://rustup.rs/).
Build the same release from its Git tag:

```sh
set -e
rustc --version
mkdir -p "$HOME/Downloads"
git clone --branch v0.4.0 --depth 1 https://github.com/solcik/agents-manifest.git "$HOME/Downloads/agents-manifest-v0.4.0"
cd "$HOME/Downloads/agents-manifest-v0.4.0"
cargo build --release --locked
mkdir -p "$HOME/.local/bin"
install -m 755 target/release/agent-skills "$HOME/.local/bin/agent-skills"
export PATH="$HOME/.local/bin:$PATH"
agent-skills --version
```

The [README](../README.md) covers manifests and CLI commands.
