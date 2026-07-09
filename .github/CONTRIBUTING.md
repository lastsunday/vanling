# Contributing

Welcome! Here's how to get started contributing to this project.

## Development Environment

This project uses **Lix** (a community fork of Nix) + **direnv** to manage the development environment. See the "Development Environment Setup" section in [AGENTS.md](../AGENTS.md) for details.

### Quick Start

```bash
# Install dependencies
pnpm install

# Start backend dev server
moon run server:run

# Start admin UI dev server (separate terminal)
moon run server-ui:dev

# Build docs
cd docs && mdbook serve
```

### Commit Convention

We use [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <description>
```

Common types: `feat`, `fix`, `refactor`, `test`, `docs`, `ci`, `chore`, `style`, `perf`, `build`.

Breaking changes use `feat!:` prefix or `BREAKING CHANGE:` footer.

### Before Submitting

- Rust: `cargo fmt && cargo clippy && cargo test`
- TypeScript: `moon run server-ui:typecheck`
- Make sure all existing tests pass

## Pull Request Process

1. Fork the repository
2. Create a feature branch: `git checkout -b feat/my-feature`
3. Commit your changes: `git commit -m 'feat: add some feature'`
4. Push to the branch: `git push origin feat/my-feature`
5. Open a Pull Request

## Code Style

- Rust: Edition 2024, snake_case for functions/variables, PascalCase for types
- TypeScript: camelCase for functions/variables, PascalCase for components/classes
- File naming: camelCase for `.tsx`/`.ts` files
- DB schema: snake_case for table and column names
- URI: snake_case for API endpoints

See [AGENTS.md](../AGENTS.md) in the repository root for full details on code conventions, error handling, and development workflow.
