# Design draft: UI plugin surface for `mh ui` (2026-07-28)

**Status: draft for review. Not a contract, not ratified, not scheduled.**

This is a design note, not a specification. Nothing in it is implemented and
nothing in it constrains any current surface. It exists so the eventual design
work starts from measured ground rather than from a blank page, and so the
reasons a smaller shape was chosen are recorded while they are still fresh.

The accepted ordering (`docs/development/admin-viewer-ui-adr-2026-07-02.md` and
the downstream roadmap) puts a UI-plugin contract **after** the October review.
This draft does not propose moving that. It proposes what to build when the
question is opened, and — more usefully — records what measurement already
ruled out.

## Why a plugin surface rather than more UI in core

The motivation is dependency containment, and it is the same argument that
produced the stdio plugin protocol.

`mh ui` today serves a fixed management screen built from a string embedded in
the Rust source. It executes no third-party code and has no build step. That is
cheap to keep and cheap to publish.

A richer viewer — an image gallery, a lightbox, sorting and filtering — is
where the weight is. Putting it directly in core means core carries it forever
as packaged UI behaviour, and if it ever wants a framework, core acquires a
JavaScript build step. There is a concrete, already-measured cost to that: the
release-hardening license inventory and CycloneDX SBOM walk Cargo and Python
package metadata only, so JavaScript compiled into the binary appears in
**neither**. The published artifact's license ledger would silently diverge from
what it actually contains.

Keeping the viewer outside as a plugin avoids that: core's SBOM stays honest,
the dependency belongs to whoever installs the plugin, and the ADR condition of
no runtime Node dependency in the distribution is preserved without argument.

This is the same trade the discovery protocol makes. It is worth being precise
about what it is *not*, though — see "What this trades away" below.

## Measured findings that shape the design

These come from prototyping two candidate consumers against a draft contract.
They are recorded because each one removes an option.

**A dependency-free viewer needs almost none of the contract.** A single
self-contained HTML page reading the database directly already produces a
working card grid, thumbnail view and full-screen lightbox with zoom, keyboard
navigation and deep links — using only `<dialog>`, CSS scroll-snap and pointer
events, with no third-party code. If the plugin brings no dependency, the
plugin boundary buys little.

**The boundary earns its keep exactly when the plugin brings weight.** The
static-asset facility measured zero consumers when both prototypes were
dependency-free, and that reads as "unnecessary" only because the prototypes
were the case that does not need it. A plugin shipping a bundled framework needs
asset serving as its first requirement. The correct reading is that asset
serving and the plugin boundary stand or fall together.

**Two consumers of different kinds do not share a contract.** A viewer and an
agent bridge were prototyped together. They shared two of three data-shape
operations and *no* UI surface at all: the agent bridge used no manifest, no
mount, no navigation, no asset serving. Designing one contract for both produced
a contract where each half was dead weight for the other consumer. The agent
side is better served by a command-line read path — which is what
`docs/read-commands.md` now describes.

**The real consumer is the downstream dashboard.** The roadmap's stated purpose
for this contract is decomposing that dashboard into a generic shell plus
private panels. A survey of its routes found roughly four fifths of them are
private-domain screens. **The contract's shape should be settled against those
panels, not against a viewer or an agent.** Until that inventory exists, any
contract is fitted to the wrong consumer.

## Proposed minimum, when the question opens

Three capabilities. Not four.

| capability | what core provides |
| --- | --- |
| asset serving | serve a plugin's files under a namespaced route |
| data | the plugin's client fetches the existing read API; no new data channel |
| registration | a navigation entry and a route |

Deliberately **not** in a first version:

- Manifest-declared queries executed by the host and injected into the page.
  Prototyping this produced three fail-closed defects — a sample configuration
  that failed only past a certain row count, an entry path that escaped its
  plugin directory, and duplicate declarations silently overwriting each other.
  Every one of them disappears if the plugin simply fetches the read API.
- A new version namespace. Reusing `record_schema_version` for a read projection
  is wrong — that constant describes the record shape plugins *send*, and a
  read projection is a different shape. Either a separate identifier or none.
- Any write path, arbitrary SQL, cross-plugin communication, or a settings UI.
- Reserved extension points for future use. The protocol's own discipline is to
  add nothing speculatively, not even a placeholder.

## The trust question, which is the real cost

This is the part that needs the most care, and it is not a dependency question.

`mh ui` currently executes no third-party code. A UI plugin runs JavaScript in
the operator's browser at the shell's own origin, which means it can reach every
route the shell can — including the management token embedded in the page when
`--manage` is active — and it can talk to any external host.

So the trade is not "risk removed" but "risk moved": from supply chain into
runtime isolation. That is a defensible trade, but only if the isolation is
designed rather than assumed. At minimum, before any plugin executes:

- the management token must not be readable from a plugin document
- the plugin document needs a policy restricting where it may send data
- `SECURITY.md` needs a section stating plainly that installing a plugin means
  granting local code execution

A related exposure, independent of plugins: a viewer that renders remote images
makes the operator's browser contact third-party hosts directly, disclosing IP
and user agent. Acceptable for local viewing, but it is a new outbound path that
the current text-only UI does not have, and it should be stated rather than
discovered.

## What this trades away

Honesty requires naming the cost of the containment argument. Moving a
dependency out of core does not make it unowned — whoever installs the plugin
still has to trust and maintain it. What changes is that core's published
artifact and its license ledger stay accurate, and consumers who do not want the
viewer do not carry it. That is a real gain, but it is narrower than
"dependency-free".

## Open questions for whoever picks this up

1. What do the downstream private panels actually require? Until that inventory
   exists the contract is being fitted to the wrong consumer.
2. Same-origin with a restrictive policy, or a sandboxed frame? The second is
   safer and forecloses some UI patterns. This should be decided by trying the
   gallery in a sandboxed frame once in a real browser — the prototype
   verification so far used a DOM shim, which does not model sandbox semantics.
3. Does the existing read API cover a viewer's needs, or does a viewer need
   filtering the read path does not have?
4. If the October review rules the project a no-go, is a shipped plugin surface
   retained or removed? Cheaper to answer before building than after.

## Status of the prototypes referenced here

The dependency-free viewer exists as a scratch artifact outside this repository
and is not part of it. It embeds real collected data — third-party titles,
performer names and image URLs — and therefore **must not be committed here**
under any circumstances. Any example that ever lands in this repository must be
generated from synthetic fixtures.
