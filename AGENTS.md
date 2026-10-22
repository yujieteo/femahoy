# femahoy: Rules for Build Agents

This file is for the agents that build femahoy itself. The product design is in `DESIGN.md`.

---

## 1. Dependencies

Do not add a dependency unless the owner approves it. Every dependency must be under MIT, Apache-2.0, BSD, ISC, Zlib, or a similar permissive licence. Never link GPL or AGPL code.

## 2. Forbidden

- Copy text, `.inp` files, figures, or numbers from the Abaqus manuals.
- Add any Dassault Systèmes file to the repo.
- Use "Abaqus" as a product name or in a product name. Abaqus is a trademark of Dassault Systèmes.
- Put secrets, API keys, or tokens in code, fixtures, logs, or commits.
- Weaken, skip, or delete a validation case or a test so that a change can pass.
- Change the tolerance of an existing case without a written reason in its fixture file and the approval of the owner. A new case with a written tolerance budget in its fixture file needs no approval. Reason: a wider tolerance can hide a fault that an existing case found. A new case has no earlier result to hide.

## 3. Bugs

A bug fix adds a test only when the bug can come back. Write that test first. Show the failing output. Fix the bug. Show the passing output.
