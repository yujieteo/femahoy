# inp skill

## Purpose
- `.inp` reader and writer. Abaqus-shaped model. Template mapping (DESIGN.md sections 6.1, 6.3 and 11).
- Licence role: open API. Line budget: 9k, tests included (DESIGN.md section 11).

## Inputs and outputs
- Input: `.inp` files, the native input format (DESIGN.md section 6.1).
- Input: user template files, which map a user syntax onto the Abaqus-shaped model (DESIGN.md section 6.3).
- Output: the Abaqus-shaped model, in memory. Parts, assembly, instances, nodes, elements, sets, surfaces, materials, sections, steps, loads, boundary conditions, interactions and constraints (DESIGN.md section 6.1).
- Output: a `.inp` file from the writer.
- Template mapping format: not yet decided (DESIGN.md section 14).

## Invariants the code must keep
- Keyword names and meanings match Abaqus for the supported subset (DESIGN.md section 7.4).
- Each unsupported keyword fails with a clear error. The error names the keyword and the line number (DESIGN.md section 7.3).
- The model keeps the keyword structure of the input (DESIGN.md section 6.1).
- Each difference from Abaqus is listed in `docs/compatibility.md`. A test covers each difference (DESIGN.md section 7.4).
- Line budget: 9k lines, tests included (DESIGN.md section 11).

## Forbidden actions
- Copy text, `.inp` files, figures or numbers from Abaqus manuals into the repo (DESIGN.md section 10.1).
- Add a Dassault Systèmes file to the repo (AGENTS.md section 4).
- Add a dependency without leader approval. Every dependency needs a permissive licence (AGENTS.md section 3).

## Validation hooks it must pass
- M1 done-when: `fh check` works on all fixtures (DESIGN.md section 13).
- The M0 gate: `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, and the `tokei` budget check (AGENTS.md section 6).

## Examples
- `fh check <file.inp>` parses and checks a model, then prints a TOON summary (DESIGN.md section 6.1).
