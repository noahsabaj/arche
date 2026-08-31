# Pending C2 negative sources

These source-only rows remain owned by C2 but are not executable
expectations. They deliberately have no `Arche.toml`: current C2 authority
cannot yet guarantee that they terminate as source `Rejected` rather than
compiler-authority `Blocked`.

- `trait002-borrowed-value-arithmetic.arc`
- `trait002-map-for.arc`
- `trait002-map-iter-for.arc`
- `trait002-map-missing-eq.arc`
- `trait002-primitive-clone.arc`
- `trait002-raw-array-for.arc`
- `trait002-str-equality.arc`
- `trait002-unbounded-postfix-method.arc`
- `trait002-vec-for.arc`
- `type002-spawn-borrowed-query-component.arc` — currently checks
  completely under C2; its rejection is owned by the C3 borrow
  authority, so it stays here until that slice claims or relocates it.

`type002-map-remove-owned-key` was promoted to an executable vector once
the mismatch renderer spelled canonically.
