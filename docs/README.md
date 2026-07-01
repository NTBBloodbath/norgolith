# Norgolith Documentation

This directory contains the source code for the [Norgolith documentation site](https://norgolith.dev).

## Development

From the repository root, enter the dev shell:

```sh
nix develop   # or direnv if configured
```

Build the `lith` binary:

```sh
cargo build --release
```

Then from this directory, start the dev server:

```sh
mprocs
```

This runs `lith dev` and the Tailwind CSS watcher in parallel.

## Building for Production

```sh
lith build
```

Output goes to `public/`.

## Deployment

Deployment is handled manually via a custom script to my personal VPS. If the site isn't up-to-date
with the source code, please open a new issue about it or look for an existing one to avoid
duplication and I'll take a look as soon as I can.
