# AGENTS.md - Rust Project Guidelines for AI Agents

Guidelines for AI agents working on this Rust codebase.

## Code Style

### Compact Code, Clear Structure

**Write the most compact code possible while maintaining clear structure.** Avoid bloat.

- Prefer concise expressions: combinators (`map`, `and_then`, `unwrap_or_else`) over verbose `match`/`if let` chains when readability is equal
- Don't introduce intermediate variables unless they meaningfully clarify intent
- Don't wrap single-use logic in helper functions just for the sake of naming it
- Avoid boilerplate abstractions (traits, builders, newtypes) unless they earn their cost through reuse or safety
- Collapse simple logic: one-liner closures, chained iterators, and `?` propagation are preferred over multi-line equivalents
- Don't add blank lines between tightly related statements
- Remove dead code, unused imports, and unused parameters immediately
- If a struct has one field, question whether it needs to exist
- If a function has one call site and no reuse potential, consider inlining it

The goal is the fewest lines that a reader can still follow at a glance. Density is good; cleverness at the expense of clarity is not.

### No Inline Comments

**Do not add inline comments to implementation code.**

- Write self-documenting code with clear naming
- Use doc comments (`///` and `//!`) for public API documentation
- Inline comments (`//`) and block comments (`/* */`) within function bodies are not allowed

**Allowed exceptions:**
- `// TODO:` with actionable description
- `// SAFETY:` for `unsafe` blocks (required by convention)
- `// FIXME:` or issue references like `// See #123`

### Naming Conventions

- **Functions/methods**: `snake_case`, verb phrases (`send_message`, `parse_header`)
- **Types/traits**: `PascalCase`, noun phrases (`ConnectionPool`, `MessageParser`)
- **Constants/statics**: `SCREAMING_SNAKE_CASE` (`MAX_RETRIES`, `DEFAULT_TIMEOUT`)
- **Modules**: `snake_case`, singular (`transport`, `config`)
- **Type parameters**: single uppercase letter or short `PascalCase` (`T`, `Item`, `Conn`)

### Avoid Duplicate Code

**Do not copy-paste logic; extract shared behavior.**

- If the same or nearly identical logic appears in two or more places, extract it into a single function, method, or shared helper
- Prefer a small, well-named helper over repeated blocks; the Rule of Three: after the second repetition, consider extracting
- Parameterize differences (inputs, options) rather than maintaining parallel copies that drift over time
- Balance with the compact-code rule: do not extract one-off logic "just in case"; extract when duplication already exists or is about to be introduced

### Function Design

- Keep functions short and focused (< 50 lines preferred)
- Use early returns to reduce nesting
- Accept borrowed types in parameters (`&str` over `String`, `&[T]` over `Vec<T>`)
- Return owned types when the caller needs ownership

## Error Handling

- Use `Result<T, E>` for all fallible operations
- Define specific error types with `thiserror` (or manual `impl`)
- Propagate errors with `?`
- Never use `.unwrap()` or `.expect()` in library/production code
- `.unwrap()` is acceptable in tests

```rust
fn connect(&mut self) -> Result<()> {
    let addr = self.resolve_address()?;
    self.stream = Some(TcpStream::connect(addr)?);
    Ok(())
}
```

## Type Safety

- Use newtypes for domain concepts (`struct Port(u16)` instead of bare `u16`)
- Make invalid states unrepresentable through the type system
- Prefer enums over booleans for multi-state values
- Use `Option<T>` explicitly instead of sentinel values

## S.O.L.I.D Principles

**Follow S.O.L.I.D principles when feasible.** Do not force abstractions where they add no value; apply them where they improve testability, flexibility, or clarity.

- **S**ingle responsibility: one clear purpose per type or module; split when a type or module gains unrelated concerns
- **O**pen/closed: prefer extension via traits and composition over modifying existing types; use `impl Trait` and trait objects where behavior should be pluggable
- **L**iskov substitution: subtypes (trait impls) must be usable where the supertype is expected without breaking callers
- **I**nterface segregation: narrow traits over "god" traits; callers should not depend on methods they do not use
- **D**ependency inversion: depend on traits (abstractions) rather than concrete types in signatures and struct fields where swapping implementations is useful

Balance these with the compact-code rule: avoid boilerplate abstractions that do not earn their cost.

## Module Organization

```
src/
├── lib.rs              # Public API re-exports, crate-level docs
├── error.rs            # Error types
├── feature.rs            # Error types
├── feature/
│   ├── primary_type.rs # One primary type per file
│   └── helpers.rs
examples/
├── tcp_master.rs, serial_master.rs
├── ncs_mcuboot.rs      # feature mcuboot-example
└── ncs/                # MCUboot RPC over SMSP (not lib API) — see examples/ncs/AGENTS.md
```

- `lib.rs` should primarily re-export; keep logic in submodules
- One primary type or closely related group per file
- Use `mod.rs` to define the module's public surface
- Example-only stacks (e.g. NCS MCUboot RPC) stay under `examples/`; do not export from `lib.rs`

## Documentation

### API-Only Documentation Rule

**Only the public API surface gets doc comments. Internal code must not have documentation.**

- `///` doc comments are required on items exported from `lib.rs` (the crate's public API)
- `//!` module-level docs are only for modules that form part of the public API
- **Do not** add `///` to `pub(crate)`, private functions, internal structs, or helper methods
- **Do not** add `//!` to internal module files
- Internal code should be self-explanatory through naming and structure alone

### Public API Doc Comments

Every public API item must have `///` doc comments including:

- Brief description (first line, used in summary listings)
- `# Errors` section if the function returns `Result`
- `# Panics` section if the function can panic
- `# Examples` with runnable doctests when practical

```rust
/// Parse a configuration file from the given path.
///
/// # Errors
///
/// Returns an error if the file cannot be read or contains
/// invalid syntax.
pub fn parse_config(path: &Path) -> Result<Config> {
    // ...
}
```

## Testing

### Organization

- Unit tests: `#[cfg(test)] mod tests` in the same file
- Integration tests: `tests/` directory
- One test per behavior, descriptive names: `test_rejects_invalid_input`

### No Trivial Tests

**Do not generate trivial unit tests.** A test is trivial if it merely restates the implementation or verifies something the compiler already guarantees.

Do **not** write tests that:
- Assert a constructor returns the values it was given
- Assert a getter returns what a setter just set
- Assert `Default::default()` produces default values
- Assert simple enum variant matching or struct field access
- Mirror the implementation line-for-line with no real assertion

Do write tests that:
- Verify meaningful behavior, logic, or state transitions
- Cover error conditions and edge cases
- Test boundary values, overflow, and wraparound
- Validate invariants that aren't obvious from the type signatures

### Test Quality

- Use helper functions for setup and common assertions
- Keep individual tests focused on one behavior
- `.unwrap()` is fine in tests for brevity

## Performance

- Prefer `&[u8]` over `Vec<u8>` in function signatures
- Avoid unnecessary allocations and clones
- No premature optimization; measure first
- Prefer clarity over micro-optimizations

## Feature Flags

- Gate optional functionality behind Cargo features
- Keep the default feature set minimal
- Test with `--all-features` to catch compilation issues

## Common Patterns

### Error Propagation

```rust
fn operation(&mut self) -> Result<()> {
    self.validate()?;
    self.transport.send(&self.buffer)?;
    Ok(())
}
```

### Builder Pattern

Use builders for types with many optional parameters:

```rust
let config = Config::builder()
    .timeout(Duration::from_secs(30))
    .retries(3)
    .build()?;
```

### Converting Options to Results

```rust
let value = self.field.ok_or_else(|| {
    Error::NotInitialized("field".into())
})?;
```

## Pre-commit Checklist

Run these before every commit:

```bash
cargo fmt
cargo clippy --all-features -- -D warnings
cargo test --all-features
cargo doc --all-features --no-deps
```

## Essential Commands

```bash
cargo check --all-features        # Type-check without building
cargo build --all-features         # Full build
cargo test --all-features          # Run all tests
cargo clippy --all-features -- -D warnings  # Lint
cargo fmt                          # Format
cargo doc --open                   # Build and open docs
cargo build --release --all-features  # Release build
```

## Adding Features Checklist

- [ ] Implementation (no inline comments)
- [ ] Doc comments on public API items only (not internal code)
- [ ] Unit tests for new logic
- [ ] Integration tests if applicable
- [ ] Update README if user-facing
- [ ] Update or add examples if relevant
- [ ] All commands in pre-commit checklist pass
