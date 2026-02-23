{ pkgs ? import <nixpkgs> {} }:

pkgs.mkShell {
  buildInputs = with pkgs; [
    rustc
    cargo
    rust-analyzer
    clippy
    rustfmt
    pkg-config
    openssl
    git

    # For LLM CLI backends
    pkgs.nodejs_22
    pkgs.nodePackages.npm
  ];

  shellHook = ''
    export RUST_BACKTRACE=1
    export NPM_CONFIG_PREFIX=$HOME/.npm-global
    export PATH=$HOME/.local/bin:$NPM_CONFIG_PREFIX/bin:$PATH
    export LD_LIBRARY_PATH=${pkgs.openssl.out}/lib:$LD_LIBRARY_PATH

    # Allow claude CLI to run inside Claude Code session
    unset CLAUDECODE

    mkdir -p $NPM_CONFIG_PREFIX

    # Install codex if not present
    if ! command -v codex &> /dev/null; then
      echo "Installing Codex CLI..."
      npm i -g @openai/codex
    fi

    # Install gemini CLI if not present
    if ! npm list -g @google/gemini-cli &> /dev/null; then
      echo "Installing Gemini CLI..."
      npm i -g @google/gemini-cli
    fi

    # Add finna to PATH if built
    if [ -f ~/dev/finna/target/release/finna ]; then
      export PATH=~/dev/finna/target/release:$PATH
    fi

    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  noggin dev shell - Your codebase's noggin"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo ""
    echo "Rust toolchain:"
    echo "  rustc: $(rustc --version)"
    echo "  cargo: $(cargo --version)"
    echo ""
    echo "Development commands:"
    echo "  cargo init        - Initialize Rust project"
    echo "  cargo build       - Build the project"
    echo "  cargo run         - Run noggin"
    echo "  cargo test        - Run tests"
    echo "  cargo clippy      - Lint"
    echo ""
    echo "LLM CLI tools:"
    if command -v claude &> /dev/null; then
      echo "  ✓ claude"
    else
      echo "  ✗ claude (install at ~/.local/bin/claude)"
    fi
    if command -v codex &> /dev/null; then
      echo "  ✓ codex"
    else
      echo "  ✗ codex (will install on first use)"
    fi
    if command -v npx &> /dev/null && npm list -g @google/gemini-cli &> /dev/null; then
      echo "  ✓ gemini"
    else
      echo "  ✗ gemini (will install on first use)"
    fi
    if command -v finna &> /dev/null; then
      echo "  ✓ finna"
    else
      echo "  ✗ finna (build ~/dev/finna first)"
    fi
    echo ""
    echo "Available for multi-model analysis:"
    echo "  - Claude (architecture, patterns)"
    echo "  - Codex (code conventions, idioms)"
    echo "  - Gemini (dependencies, structure)"
    echo ""
  '';
}
