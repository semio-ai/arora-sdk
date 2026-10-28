# Registry identity

Status: draft, for review, with [configurations](proposal-configurations.md).
Date: 2026-10-26.

How scopes, packages, releases, pins and records are identified wherever
Studio, the open registry protocol
([semio_studio#616](https://github.com/semio-ai/semio_studio/pull/616)) and
Arora devices exchange them. The scheme keeps what Studio and `arora-index`
already store, follows the patterns of npm, crates.io, Maven and Go where they
apply, and adds one thing they lack: a package that moves to another scope or
registry keeps its consumers.

| | |
|---|---|
| **Names address** | `@scope/name` is how every package is looked up, depended on and pinned. The scope routes the request to one registry. |
| **Ids identify** | A package has an id its registry binds at its first publish. It survives renames, transfers and moves, and is checked, never looked up. |
| **Hashes verify** | Each artifact carries a SHA-256, which the resolver checks before anything is used or cached. |

A package is what a registry publishes under one name: a module with its
executables, a single type, a behavior, a configuration, a model. It wraps
nothing; a type's package is that type, released at each of its versions.
Records keep their UUIDs inside releases, as Arora's runtime identity. Nothing
looks a package up by a record's UUID.

---

## 1. What identifies what

| | Written as | Identified by | Where that lives |
|---|---|---|---|
| Scope | `@vizij` | Its handle, bound to one account, and a UUID that survives a rename of the handle | The registry's scope bindings; on Studio, `userHandles` |
| Package | `@vizij/animation` | Its id, bound by its registry; reached by its scoped name, and by the aliases its moves left | The registry's name claims and id bindings |
| Release | `@vizij/animation@1.2.0` | The package and the version, published once, with a SHA-256 per artifact | The release's metadata |
| Dependency | in a release's metadata | Scoped name, package id and range | The release's metadata |
| Pin | a lock entry | The name requested, the package id, the registry, the version, and each artifact key with its hash | A project's lock; a frozen configuration's `packages` |
| Record | `vizij.StepValues` | Its UUID and its version, under its parent scope's UUID | Inside a release's `record` artifact; indexed on the device |
| Registry keys | none | Studio's document id for a release | Inside the registry; it travels only as the opaque `locator` |

```toml
# A pin, in a frozen configuration's lock
[[package]]
name     = "@arora/behavior"
id       = "0b6e3f6c-5d1e-4a8f-9f2a-3c7d2e9b1a40"
registry = "studio"
version  = "1.4.0"

[package.artifacts]
record = "sha256:3b1f…c09a"
wasm   = "sha256:a7e2…41d6"
```

```yaml
# arora.Status, as its own package's release carries it
id:      325a5767-e344-4532-860e-0749bcf2e428
version: 1.4.0
parent:  61726f72-6100-0000-f01d-000000000000   # the arora scope
```

The pin says which package to fetch, where, and how to check it. The record's
UUID is how modules and behaviors name the type at runtime. A device learns
which package delivers `arora.Status` from the configuration's pins, fetches
it, and indexes the record by its UUID.

## 2. The rules

- **Packages go by name.** Every lookup, dependency and pin names a package
  by its scoped name, and a project maps each scope to one registry. A
  registry may keep its own keys; they travel only inside the opaque
  `locator`, and no client relies on them.
- **A registry binds a package's id at its first publish.** It mints one, or
  binds the one the publisher presents if no package holds it on that
  registry. An id another package holds is refused; whether the holder takes
  the publish is its owner's decision, and moving an id between owners goes
  through the registry's operator. A deleted package's id is never bound
  again. A package's id is never a record's UUID.
- **A move keeps the id.** A transfer to another scope, a rename within a
  scope and a move to another registry leave the old name pointing at the
  new one, carrying the id: an alias inside the registry, a redirect naming
  the target registry otherwise. Inside a registry an alias points at the
  package, so a later move updates it and it stays one hop. Across
  registries the resolver follows a bounded number of redirects, requires
  the id at every hop to be the id it set out with, and refuses a cycle.
- **A version is published once.** A release never changes after
  publication, except `deprecated`, which only steers range selection. The
  registry refuses a second release of a version with different content, and
  treats the same content as a no-op. Versions are `MAJOR.MINOR.PATCH` at
  1.0.0 or above, in any order: a fix on an older major is a release.
- **The resolver verifies.** It checks the bytes against the pinned hash
  before it uses or caches them, a release's id against the pin's, and each
  record's `parent` against the release's scope. One record UUID and version
  arriving with two different contents is a corrupt closure, and freezing
  refuses it. A source only transports.
- **Dependencies name and identify.** A release's dependencies are
  `{ scopedName, packageId, range }`: the name routes, the id checks what the
  name answers, and resolution walks names.
- **Records keep their UUIDs.** A record's references to other records stay
  `FrozenReference { id, version }`. A frozen configuration carries the pins
  of every package it draws records from, and the device indexes records by
  UUID from what it installed. It never asks a registry for a record by UUID.
- **Each type is a package of its own**, with its own semver, so a type's
  version is the version of the one release line that ships it. A module
  depends on the type packages it names, and a configuration picks one
  version of each type that every module accepts. A type defined inside
  another package, such as a profile inside its model, is reached through a
  dependency on that package.
- **A release carries its scope.** A release records the account that owned
  its package when it was published and the scope it was published under,
  and every record it carries has that scope as its `parent`. A later
  transfer does not change it.
- **Content describes itself.** What a consumer needs to load a release is in
  its bytes or its declared metadata, never in a registry-specific field.
  Studio's `contentId` is the one exception today, and a format change
  retires it
  ([STUDIO-402](https://linear.app/semio-ai/issue/STUDIO-402)).

## 3. Three parties

| Party | Does | Signs in |
|---|---|---|
| An author | Publishes packages and releases | As a person or a tool, with an account that owns the scope; on Studio, through its publish endpoint |
| A deployer | Resolves, freezes and applies a configuration | Not for public packages; as a person or a tool for private ones |
| A device | Fetches what its configuration pins, and runs it | Not for public packages. For private ones and for the Studio Bridge, as itself, after an enrollment that ties it to the account that manages it |

One sign-in crate serves them all, `semio-studio-session`, without merging
their rights: a tool's session carries the `tool` claim, a device's the
`device` claim and its id.

## 4. What this gives Studio and Arora

**Studio**

- Its identity model stays: a package is `publishedAssets/{id}`, names are
  claims and aliases pointing at it, and a transfer keeps the id. The id
  becomes visible, as `packageId`, in releases, aliases and pins.
- A pin refuses a different package that takes a freed name.
- A project's `dependencies` and `resolved` lock stay keyed by scoped name;
  each resolved entry gains the package id.
- The tokenless read path on main already resolves by name: a public
  `@scope/name`, its package, its releases and its bytes.

**Arora**

- Record UUIDs, scope ids and the shape of `FrozenReference` are unchanged.
- It works without Studio. A folder or a static host lays packages out by
  name, one index file per package with its id and releases (section 10).
- Every lookup names a scope, so a project's per-scope registry mapping
  routes every request.
- A record UUID copied from public source gains nothing: packages are
  claimed by name inside scopes their owners control, and every record's
  scope is checked against its release's.
- A package moved to another scope, under another name or to another
  registry keeps its consumers.

## 5. What Studio already has and plans

| Item | State | What it means here |
|---|---|---|
| Tokenless public reads | On main | With no token, a client resolves a public name through `scopedNames` (aliases followed), reads the package and its releases, and downloads the bytes, each step a plain HTTPS GET to Firestore's or Storage's REST endpoint. Pinned by the external-consumer tests in `rules.test.ts`. |
| Handle registry | On main | `userHandles` binds each handle to an account, user or organization, and only the server writes it. The rules resolve a scope's holder through it. |
| Transfer with alias (STUDIO-274) | On main | A transfer keeps the package's id and leaves the old name as a permanent alias. A rename within a scope, and the scope-wide handle rename, are to build. |
| Publish endpoint ([STUDIO-169](https://linear.app/semio-ai/issue/STUDIO-169)) | Decided, to build | The one publish path, the web app's included: scoped, revocable publish tokens, with OIDC trusted publishing later. It binds package ids, checks every record's scope, hashes artifacts before it writes the release, and refuses a different hash at an existing version. |
| Distribution identity (`07-engine-reconciliation.md` §1) | Recorded | Scoped name, release and content hash, with no table from UUIDs to hashes, so one artifact never has two answers to which bytes it is. A package id next to the hashes keeps that: the hash answers which bytes, the id which package. |
| Release dependency keys ([STUDIO-157](https://linear.app/semio-ai/issue/STUDIO-157)) | Backlog | Keyed by `publishedAssetId` today, so a dependency cannot name a package held elsewhere, such as on GitHub. They become `{ scopedName, packageId, range }`: the name reaches any registry. Every stored list is empty, so nothing migrates. |
| Types as registry assets ([STUDIO-154](https://linear.app/semio-ai/issue/STUDIO-154)) | Backlog | Each type is a package of its own, so its version is its own release's. |

## 6. Against other registries

| | This scheme | npm | crates.io | Maven Central | Go modules |
|---|---|---|---|---|---|
| Package address | `@scope/name` | `@scope/name` | `name` | `groupId:artifactId` | module path |
| Package identity | an id the registry binds | the name | the name; a numeric id stays internal | the coordinates | the path |
| Namespace owner | the scope's owner | the scope's user or org | first publisher of the name | verified groupId owner | domain or repository host |
| A version | published once, in any order | published once | published once | published once | fixed by the proxy and checksum database |
| Integrity | SHA-256 per artifact | SHA-512 `integrity` | SHA-256 `checksum` | checksum files per artifact | SHA-256 in `go.sum` |
| Lock entry | name, id, version, registry, hashes | resolved URL, integrity | name, version, source, checksum | none by default | `go.sum` |
| Dependencies | name, id and range in release metadata | name and range in `package.json` | name and requirement in the index | coordinates in the POM | path and version in `go.mod` |
| Publish credentials | scoped tokens, then OIDC | granular tokens, OIDC | scoped tokens, OIDC | user tokens | none; fetched from source |
| Withdrawal | deprecate | deprecate; unpublish only in narrow cases | yank | none | `retract` |
| Rename or move | alias or redirect carrying the id; consumers keep resolving | new package, old one deprecated | new crate | relocation POM, unchecked | new module path |
| 0.x and prereleases | not yet: 1.0.0 floor, prereleases planned | allowed | allowed | allowed | allowed |

The comparable registries look packages up by name for good reasons, and so
does this scheme: a name routes a lookup to one registry, which keeps public
lookalikes out of private builds
([Microsoft DevOps blog, 2021-02-09](https://devblogs.microsoft.com/devops/changes-to-azure-artifact-upstream-behavior));
a name is a path, so a registry can be plain files
([Maven repository layout](https://maven.apache.org/repositories/layout.html),
[Cargo registry index](https://doc.rust-lang.org/cargo/reference/registry-index.html));
a name is claimed inside a scope its owner controls
([npm scopes](https://docs.npmjs.com/cli/v10/using-npm/scope)); integrity
comes from hashes ([package-lock.json](https://docs.npmjs.com/cli/v10/configuring-npm/package-lock-json),
[Go modules reference](https://go.dev/ref/mod)). None of them lets a package
move to another scope or registry without its consumers changing their
dependency, and that is what the package id adds.

## 7. Where the code lives

| Component | Repo | Role |
|---|---|---|
| `arora-registry` | arora-sdk | Records in memory at runtime; the `RegistrySource` trait and its types |
| `arora-index` | arora-sdk | Asset management over any source: resolution, verification, cache, `DirSource` |
| `arora` | arora-sdk | Runs a device (`arora run`), signs in and enrolls devices through a provider (`arora auth login semio`), and hands the package verbs to `arora-pkg` ([configurations](proposal-configurations.md) 3.11) |
| `arora-pkg` | arora-sdk | The package verbs: `add`, `install`, `show`, `publish`, `deprecate`, `version`, `vendor`; folder registries and moves. It reaches Studio through the provider its user configuration names |
| `semio-studio-session` | studio-bridge | Sign-in to Studio, device enrollment, the session and its credential |
| `semio-studio-assets` | studio-bridge | Studio's `RegistrySource`: reads Studio's packages, publishes through the endpoint |
| `arora-studio-bridge-client` | studio-bridge | The device's live connection to Studio, on the same session |
| `arora-semio` | semio-cli | Studio's provider: fills the sign-in, registry, devices and tokens roles behind `arora`'s verbs |
| `semio` | semio-cli | Studio's sign-in and enrollment as commands of their own (`semio auth login`, `semio device enroll`), which work without `arora` |
| Rules and endpoint | semio_studio | Names, package ids, scopes, releases |

## 8. What changes, by repository

**arora-sdk** ([configurations](proposal-configurations.md) 3.3, 5)

| Item | Change |
|---|---|
| `RegistrySource` | `releases()` looks up by `ScopedName`; `PackageRef::Id` is an optional convenience a source may answer. |
| `Release` | Carries `package` (the id, `None` from a registry that keeps none), `scope`, `dependencies` and the opaque `locator`. |
| Placement | `PackageId`, `Dependency` and `Pin` in `arora-types`, since a configuration record embeds pins; the trait and the source-only types in `arora-registry`. |
| `Pin` | Name requested, package id, registry, version, artifact keys with hashes, locator. A pin finds its release by its name and version, following aliases and redirects, and checks the id and hashes; the locator is only a shortcut on the registry that made it. |
| Resolver checks | Release id against pin and dependency; record `parent` against the release's scope; two contents for one record id and version refused. |
| `arora-index` | Pins carry the id; `fetch` no longer requires the current name to equal the pinned one; `DirSource` reads the layout of section 10. |
| Record versions | Unchanged: a derived type keeps its declared version, which its own package releases. |
| Scopes | The fixed ids of `arora`, `vizij` and `semio` stay. |

**studio-bridge**

| Item | Change |
|---|---|
| `semio-studio-assets` | Reads public packages with no token and private ones with the caller's credential: a name through claims and aliases, then the package, its releases and its bytes. Reports `packageId`, the scope the release recorded, and dependencies with their names and ids. Publishes through the endpoint. Does not carry `contentId`. |
| `semio-studio-session` | Unchanged by identity: it signs authors, tools and devices in. |

**semio_studio**

| Item | Change |
|---|---|
| Package id | `packageId` on every published package, bound by a create-only `packageIds/{id}` document only the endpoint writes. The backfill reuses the document id where it is a UUID and mints one otherwise. |
| Aliases | Carry the package id. A rename within a scope joins the transfer as a move. |
| Release owner and scope | Set once at publish, by the endpoint. |
| Version uniqueness | Server-enforced: every publish goes through the endpoint. |
| Version order | None: a fix on an older major is a release. |
| Release dependencies | `{ scopedName, packageId, range }`. |
| Artifact keys | `record`, `header`, `wasm` and `native/<triple>`, as #616's S5a lists. |
| Scope UUIDs | Bound to their account; `@arora`, `@vizij` and `@semio` held by Semio's accounts with their fixed ids. |

**`arora-pkg` and `arora-semio`**: `arora publish` reaches the endpoint
through `arora-semio`'s registry role, with a publish token; `arora-pkg` also
binds ids in folder registries and writes the alias a move leaves.

## 9. Studio's publish endpoint

### Packages without a Studio project

Publishing from CI, with its configuration and a publish token, needs no
Studio project. Studio's database already allows it: a package's
`sourceProjectId` is optional provenance in its schema and types
(`docs/new-studio-sharing-tech/03-firestore-schema.md`, `01-types.md`), and
Studio's server code handles a package without one. Only the client create
rule, `isPublishBoundToSource`, requires a project, and client creates close
when the endpoint opens. The token carries the authority, and the endpoint
applies the checks Studio already makes:

| Case | Check |
|---|---|
| Always | The token is live and covers the package's scope. |
| A new name in a person's scope | The token's owner holds that handle, or a delegation from its holder. |
| A new name in an organization's scope | The token's owner meets the organization's publishing minimum. |
| A new name from a named project | Admin on that project, whose owner is the scope's owner, as the rules require today, so a collaborator publishing into another person's scope keeps working. |
| A new release | Admin on the package. The publishing minimum does not apply to releases. |
| A package id | Minted, or the presented one when no package holds it; otherwise refused, naming who to ask. |

On this path `sourceProjectId` is optional provenance, recorded only when the
named project passes the check above. Publishing from the web app keeps
today's project rule, which stops someone with view access to a project from
publishing its content as their own.

- A package created without a project has no source asset, so its releases
  come from CI or `arora publish`. Studio's release dialog says so instead of offering
  a release from a project.
- Until STUDIO-402 lands, a model or face release needs its root: a model's
  file supplies one; a face publish sends it.
- Tokens belong to the person who minted them, so an organization's CI
  publishes as that person, whose standing is checked on every request,
  until organization tokens exist.

### Reads and publishes

Reads use the tokenless path already on main, through Studio's source.
Publishes go through the endpoint only, the web app's included: the rules stop
admitting client creates of packages, name claims, id bindings, releases and
artifact uploads in the same deploy that opens the endpoint. A registry-shaped
read index waits for a measured need, such as a CDN or dependency resolution
that reads document by document cannot serve. A second factor for publishing
can follow on the same path.

### The work, and its expected size

Sizes come from the closest shipped work: the organization-invitations server
and UI PRs, and the published-asset transfer.

| PR | Contains | Expected size | Comparable |
|---|---|---|---|
| 1. Publish endpoint (server) | A framework-free `publish()` core in `packages/server`: a new package with its name claim, id binding and first release; a new release of an existing package; idempotency on name and version. Studio's first `onRequest` endpoint, authenticated by a bearer token, with artifact uploads the server hashes before it writes the release. A `publishTokens` collection closed to clients, with mint and revoke callables. Server, function, rules and parity tests. | +3,000 to +4,000 lines, 35 to 45 files, about half tests | #576 (+3,780 lines, 40 files); #457 (+2,690 lines, 40 files) |
| 2. The web app on the endpoint, rules closed | `usePublish` and `usePublishRelease` become thin callers, and the rules stop admitting client creates. The `packageId` backfill. | +1,500 to +2,500 lines, mostly rules tests | The rules side of #457 |
| 3. Tokens and CI-published packages (UI) | Mint a token (shown once), list and revoke tokens; the release dialog for a package without a source project. Browser tests. | +1,500 to +2,500 lines, 20 to 30 files | #578 (+3,724 lines, 47 files) |

PRs 1 and 2 deploy together, so no deploy runs with two publish paths. Both
change the security rules, so each gets Studio's two independent reviews.
Outside semio_studio, `semio-studio-assets` publishes through the endpoint
instead of Firestore's REST API, and `semio-studio-session` keeps the token
for `arora-semio`'s sign-in role; both are small.

## 10. Running your own registry

A team, a lab or a community can publish packages without Semio, with no
server, database or account: a registry is a folder, served as static files
when others need it.

**1. Choose the form.**

| Form | What it is | For |
|---|---|---|
| A folder | A directory on disk or a network share | One machine, a lab, offline use |
| A static host | The same folder over HTTPS: GitHub Pages, S3, nginx | A team, a public community index |
| A server | The protocol over HTTP, or a `RegistrySource` of its own | Access control, search, very large catalogues |

The second and the third start as the first.

**2. Lay it out.** A folder and a static host share one layout:

```text
my-registry/
├── index/
│   └── lab/
│       └── gaze.json        the package: its id, or the alias a move left, and every release
└── blobs/
    └── sha256/<hash>        the artifacts, by content
```

```json
{
  "name": "@lab/gaze",
  "id": "0b6e3f6c-5d1e-4a8f-9f2a-3c7d2e9b1a40",
  "releases": [
    {
      "version": "1.0.0",
      "kind": "module",
      "scope": "6c61622d-…",
      "artifacts": {
        "record": { "sha256": "3b1f…c09a", "size": 2210 },
        "header": { "sha256": "9d04…77e1", "size": 1830 },
        "wasm":   { "sha256": "a7e2…41d6", "size": 412512 }
      },
      "dependencies": [
        { "name": "@arora/status", "id": "…", "kind": "type", "range": "^1.4" }
      ],
      "deprecated": false
    }
  ]
}
```

A static host cannot list a directory, so a client finds a package's releases
in the one file it fetches by name. Blobs are named by their hash, so an
artifact shared by several releases or packages is stored once, and a mirror
copies the folder as it is.

**3. Publish.** `arora publish gaze --registry ./my-registry` builds, hashes
each artifact, runs the interface check, writes the blobs and adds the
release to `index/lab/gaze.json`. At a package's first publish it mints the
id, or binds the one the publisher presents if no package holds it in this
registry.

**4. Serve.** Push the folder to any static host. In a git-hosted registry, a
publish is a pull request, and review is the access control, as in
crates.io's index.

**5. Point projects at it.** A project maps the scope to the registry in its
`arora.toml`, and every lookup of that scope goes there and nowhere else:

```toml
[registries]
lab = "https://lab.example.org/arora/"

[scopes]
"@lab" = "lab"
```

**6. Keep packages private (optional).** Put the host behind
authentication, and name its provider in the user's own
`~/.config/arora/config.toml`, never in a project's file: `provider = "lab"`,
an `arora-lab` program on `PATH` that fills the sign-in role. `arora auth
login lab` runs it. A device fetches with its own credential.

**7. Look after it.**

- **A bad release** is deprecated, `arora deprecate @lab/gaze@1.1.0`: never
  edited or removed. A fix is a new version, a backport included.
- **A rename or a move to another scope** leaves the old name as an alias:
  `index/lab/gaze.json` becomes `{ "name": "@lab/gaze", "id": "…",
  "alias_of": "@lab/eyes" }`.
- **A move to another registry** leaves a redirect: the alias plus
  `"registry": "https://…"`. The resolver follows it, checks the id at every
  hop and refuses a cycle.
- **A mirror** copies the folder as it is: ids, hashes and aliases come
  along.

**Whatever the registry, the consumer verifies.** The resolver checks every
artifact against its SHA-256, the package id against the pin, and each
record's scope against its release. Against a pin, a compromised mirror can
withhold a package, never substitute one. A first resolution, or an update
to a version not yet pinned, trusts the hashes the registry its scope maps
to advertises, so a project maps a scope only to a registry or mirror it
trusts for that scope.

## 11. Open questions

- **A Studio scope's UUID**: derived from the owning account's id (a version 8
  UUID over SHA-256 of a fixed namespace and the id), so nothing is stored and
  nothing drifts, but a scope can then never pass to another account; or
  minted when the handle is claimed and bound in a stored document.
- **Prerelease tags**: planned, not specified.
- **Organization publish tokens**: until they exist, an organization's CI
  publishes as a person.

---

Sources: [configurations](proposal-configurations.md) §2.2, §3.2, §3.3, §5;
arora-sdk [#360](https://github.com/semio-ai/arora-sdk/pull/360)
`crates/arora-index/src/source.rs`, `resolver.rs`;
[semio_studio#616](https://github.com/semio-ai/semio_studio/pull/616)
`docs/arora-registries/README.md` C14, C18, C19, C21; semio_studio main
`docs/new-studio-sharing/07-engine-reconciliation.md` §1,
`packages/core/src/access/documents.ts`,
`packages/core/src/access/project-dependency-resolution.ts`,
`packages/core/src/hooks/use-publish.ts`,
`packages/server/src/published-asset-transfer.ts`,
`apps/studio/firestore.v2.rules`; Linear STUDIO-154, 157, 169, 274, 402,
ARORA-174. Hashes and the id in the examples are shortened or illustrative.
