<div align="center">

<img src="./res/norgolith_text.png" alt="Norgolith logo"/>

---

The monolithic Norg static site generator built with Rust. Leverage the power of [tree-sitter]
validation and [norg-pandoc] conversion to create stunning static sites from your Norg content.

</div>

## Requirements

### Build requirements

- C/C++ compiler (required by `tree-sitter` norg parsers)
- Rust `>= 1.77` (latest stable release)

### Runtime requirements

- pandoc

## Usage

Compile the project using the `optimized` Cargo profile.

```sh
$ cargo build --profile optimized && \
  ./target/optimized/norgolith --help

The monolithic Norg static site generator

Usage: norgolith <COMMAND>

Commands:
  init   Initialize a new Norgolith site (WIP)
  serve  Build a site for development (WIP)
  build  Build a site for production (WIP)
  help   Print this message or the help of the given subcommand(s)

Options:
  -v, --version  Print version
  -h, --help     Print help
```

## Install

Run `cargo install --path .` to compile and install the project in `~/.cargo/bin` :)

## License

This project is licensed under [GPLv2](./LICENSE).


[tree-sitter]: https://tree-sitter.github.io/tree-sitter/
[norg-pandoc]: https://github.com/boltlessengineer/norg-pandoc
