# Documentation

The **[OneNote Viewer Master Plan](MASTER-PLAN.md)** is the canonical project
entry point. Start there for the mission, non-negotiable outcomes, deliverables,
current phase, execution order, definition of success, and document authority.

The documentation is organized by how stable a decision is and who consumes
it. Specifications define required behavior. Architecture describes the
boundaries that implement it. ADRs record why consequential choices were made.
Plans may change as evidence arrives.

## Product and Architecture

- [Master plan](MASTER-PLAN.md)
- [Product requirements](specs/product-requirements.md)
- [Repository layout](architecture/repository-layout.md)
- [System architecture](architecture/system-architecture.md)
- [Technology stack ADR](decisions/0001-technology-stack.md)
- [ONEPKG extraction ADR](decisions/0002-onepkg-extraction.md)
- [Reusable component ADR](decisions/0003-reusable-components.md)

## Specifications

- [OneNote input and parsing profile](specs/onenote-format.md)
- [Persisted feature inventory](specs/feature-matrix.md)
- [Search behavior](specs/search.md)
- [Desktop UI requirements](specs/desktop-ui.md)
- [Public integration API](specs/public-api.md)
- [Test corpus specification](specs/test-corpus.md)

The parsing profile is an implementation guide layered on top of the archived
Microsoft specifications. It does not duplicate every bit-field table. For
wire-level details, the versioned PDFs in `references/microsoft/` are
normative.

## Delivery and Risk

- [Roadmap and milestone gates](plans/roadmap.md)
- [Reusable backup-folder loader plan](plans/backup-folder-loader.md)
- [Potential limitations](limitations.md)
- [Current remaining work](REMAINING-WORK.md)
- [Release build and artifact guide](RELEASES.md)
- [Project and third-party licensing](../THIRD-PARTY-NOTICES.md)
- [Corresponding-source information](../SOURCE-CODE.md)
- [Documentation baseline audit](plans/completion-audit.md)
- [Reference manifest and provenance](references/README.md)

The roadmap is the detailed execution schedule beneath the master plan.
Remaining work records implementation and release gaps. The completion audit
records how the original documentation baseline was assembled; it is
historical evidence, not a current project plan or implementation status.

## Documentation Rules

1. Use relative links for repository documents and stable upstream URLs for
   external sources.
2. State whether a claim is normative, observed, inferred, or provisional.
3. Put irreversible or cross-cutting decisions in a numbered ADR.
4. Keep limitations visible even when a workaround exists.
5. Update the feature matrix and corpus case together when adding format
   support.
6. Never commit private notebooks. Only deliberately constructed, licensed
   fixtures belong in `fixtures/`.
7. Update the master plan whenever scope, architectural direction, current
   phase, delivery order, or the definition of success changes.
