# toon skill

## Purpose

Encode short agent summaries with schemas `fh.check`, `fh.run`, `fh.validate` and `fh.validate.case`.

## Inputs and outputs

- Schema version is `1.1.0`.
- Every schema header includes `precision`, which defaults to 4 significant figures.
- `Opts` sets row limits, precision and the comma or tab delimiter. These are library options, not implemented `fh` flags.
- Exact field values remain in VTU files. `fh check` and `fh validate` currently print plain text.

## Invariants the code must keep

- Only `wrap` calls `toon-format`.
- Tables have at most 4 fields. The default row limit is 20.
- Diagnostics put errors first. The `next` table has at most 3 rows.
- Truncated tables report omitted rows with `_more`.
- TOON holds summaries and query answers only. Full field data never goes into TOON (DESIGN.md section 8).

## Forbidden actions

- Do not treat rounded TOON values as exact field data or describe `fh results` as implemented.
- Write full field arrays into TOON (DESIGN.md section 8).

## Validation hooks it must pass

Run `cargo check -p fh-toon` and `cargo test -p fh-toon`. Schema tests compare output with snapshots.

## Examples

Use `Opts::default()` for 20 rows, 4 significant figures and comma delimiters.
