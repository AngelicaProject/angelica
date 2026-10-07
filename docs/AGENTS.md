# Documentation instructions

These instructions apply to files under `docs/`.

Before editing documentation, read `docs/README.md` and `docs/development/documentation.md`.

## Navigation

- Every top-level documentation section has a `README.md` index.
- Add new documents to the relevant section index.
- Update `docs/README.md` when a new topic changes top-level navigation or task routing.
- Prefer moving a rule to its owning document and linking to it over duplicating the rule elsewhere.

## Content

- Describe current behavior, explicit intended contracts, and contributor-relevant guidance.
- Use direct, impersonal technical language and sentence case.
- Avoid promotional language, implementation diaries, agent process commentary, and historical narration that does not help maintainers understand the current system.
- Do not name third-party products as examples; describe the kind of tool instead ("a mod manager", "a graphics injector"). Attribution required by a license is the exception.
- State constraints concretely. Give examples when a rule requires judgment.
- Document the system and its contracts. Do not record how a design was chosen, the alternatives considered, status markers, or plans; those belong in pull requests and issues.

## Links and structure

- Use relative links inside a repository and full GitHub URLs for the other Angelica repositories.
- Keep headings descriptive and stable.
- Keep indexes concise: explain what each document owns rather than repeating its contents.
- When adding, moving, or deleting a document, verify inbound index links remain correct.

## Changes coupled to code and data

When a change modifies product behavior, architecture boundaries, formats, compatibility guarantees, testing requirements, or release requirements, update the canonical documentation in the same pull request.
