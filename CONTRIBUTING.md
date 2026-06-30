# Contributing to MellowMesh

Thank you for your interest in contributing to MellowMesh! We welcome contributions from the community to help make MellowMesh the best coordination fabric for human-agent distributed work.

---

## 1. Getting Started

### Prerequisites
To build and test MellowMesh, you need the following tools:
*   **Rust Toolchain**: Rust 1.80 or higher is recommended (install via [rustup.rs](https://rustup.rs/)).
*   **C Compiler**: Required for compiling SQLite bindings (e.g., MSVC on Windows, `build-essential` on Debian/Ubuntu, or Xcode Command Line Tools on macOS).

### Repository Setup
1. Fork the repository on GitHub.
2. Clone your fork locally:
    ```bash
    git clone https://github.com/YOUR-USERNAME/mellowmesh.git
    cd mellowmesh
    ```
3. Add the upstream repository as a remote:
    ```bash
    git remote add upstream https://github.com/mellowmesh/mellowmesh.git
    ```

---

## 2. Development Workflow

### Compiling and Running
To build all crates in the workspace in debug mode:
```bash
cargo build
```

To run the daemon background service:
```bash
cargo run --bin mellowmeshd
```

To run the CLI client:
```bash
cargo run --bin mellowmesh -- --help
```

### Running Tests
We enforce comprehensive unit and integration testing. Make sure all tests pass before submitting a pull request:
```bash
cargo test
```

### Code Formatting
Ensure your code is formatted according to the standard Rust style:
```bash
cargo fmt --all --check
```
To auto-format your changes before committing, run:
```bash
cargo fmt --all
```

### Linting & Diagnostics
Run Clippy to check for common issues, code style improvements, and performance opportunities:
```bash
cargo clippy --all-targets --all-features -- -D warnings
```

---

## 3. Pull Request Guidelines

When submitting a pull request (PR):
1.  **Branching**: Create a new feature branch for your changes (e.g., `feature/add-connector-x` or `bugfix/issue-123`).
2.  **Commit Messages**: Keep commit messages clear, concise, and descriptive (e.g., `feat(daemon): add telemetry support`).
3.  **Tests**: Include unit tests for any new functionality or bug fixes.
4.  **Documentation**: Update relevant documentation, READMEs, or inline comments if you alter public APIs or introduce new features.
5.  **Checks**: Verify that all CI checks (formatting, clippy, and unit tests) pass locally.
