# Changesets

Each PR that changes what users get adds a changeset: run `pnpm changeset`, pick the bump
(`patch` for fixes, `minor` for features) and write one plain sentence for the changelog.
Changes users won't notice (CI, docs, refactors) can use `pnpm changeset --empty`.

See [CONTRIBUTING.md](../CONTRIBUTING.md#versioning-and-releases) for the release flow.
