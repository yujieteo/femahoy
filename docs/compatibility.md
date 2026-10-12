# Compatibility

This file lists how femahoy differs from Abaqus input conventions.

## Rule

- Keyword names and meanings match Abaqus for the supported subset.
- Every difference is listed in this file.
- A test covers each difference.
- Tests use femahoy `.inp` files. No Abaqus manual file is in the repo.

## Differences

| Keyword | Difference | Test |
|---|---|---|

## Keywords for M1

M1 supports these keywords first:

- `*NODE`
- `*ELEMENT`
- `*NSET`
- `*ELSET`
- `*MATERIAL`
- `*ELASTIC`
- `*STEP`
- `*STATIC`
- `*BOUNDARY`
- `*CLOAD`
