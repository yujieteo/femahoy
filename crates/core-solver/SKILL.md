# core-solver skill

## Purpose

Solve solid models with `C3D8`, `C3D8I` and `C3D10`. The crate includes linear and Newton increment paths.

## Inputs and outputs

- Input is `fh_inp::Model` or an `.inp` file for `fh-solve`.
- Output includes VTU/PVD results and an `fh.run` TOON summary. Text and legacy VTK formats are optional.
- `fh-solve` uses protocol v2 and supports restart files.
- For new models, use `C3D10` by default. Use hex8 (`C3D8I`, `C3D8`) only for extruded parts.

## Invariants the code must keep

- `element::Kind` accepts only `C3D8`, `C3D8I` and `C3D10`. The parser accepts `C3D10M`, but this solver rejects it.
- Each element needs a material through `*SOLID SECTION`.
- A rejected Newton increment discards trial state before a cutback.
- TOON defaults to 4 significant figures. The `precision` header states the setting. VTU files retain exact values.
- Line budget: 30k lines, tests included (DESIGN.md section 11).
- Contact cases compare integrated values only: total contact force, contact area or radius, and peak pressure (DESIGN.md section 10.3).

## Forbidden actions

- Do not infer contact, plasticity or NLGEOM support from the generic material and contribution interfaces.
- Use a Haiku model for numerical work in this crate. Stop and report to the leader (AGENTS.md section 2).
- Compare node-by-node contact status in a test (DESIGN.md section 10.3).
- Weaken, skip or delete a validation case. Change a tolerance only with a written reason and leader approval (AGENTS.md section 4).
- Copy text, `.inp` files, figures or numbers from Abaqus manuals (DESIGN.md section 10.1).
- Add a dependency without leader approval. Every dependency needs a permissive licence (AGENTS.md section 3).

## Validation hooks it must pass

Run `cargo check -p fh-core-solver` and `cargo test -p fh-core-solver`. Run `fh validate` and convergence checks for changed solver paths.

## Examples

`fh validate --convergence validation/cases/cantilever` checks the cantilever convergence path.
