<div align="center">

<img src="./res/norgolith_text.png" alt="Norgolith logo"/>

---

The monolithic Norg static site generator built with Rust. Leverage the power of [tree-sitter]
validation and [norg-pandoc] conversion to create stunning static sites from your Norg content.

</div>

## Features / Why use Norgolith?

Norgolith caters to both developers and content creators seeking a powerful and user-friendly
solution for crafting statis websites from Norg content. Here's what makes Norgolith stand out:

### For content creators seeking an easy-to-use conversion tool

- **Effortless Norg workflow**: write your content in Norg and let Norgolith handle the technical
  complexities. Seamlessly convert your Norg documents to clean and validated HTML.
- **Focus on content, not code**: leave the website maintenance and complexities to Norgolith. You
  can safely concentrate on creating compelling content for your audience without worrying about
  weird bugs or unexpected invalid syntax from reaching your production site.
- **Live preview**: See your Norg content rendered as HTML in real-time as you edit it, allowing for
  a smooth writing experience and easy iteration on your website's design.

### For developers who value validation and control

- **Robust syntax validation**: leverage the power of tree-sitter to catch errors in your Norg
  documents before conversion. This ensures clean, well-structured HTML output and avoids surprises
  later during the development process.
- **Modern Rust codebase**: contribute with ease! Norgolith boasts a clean, well-structured codebase.
  This allows you to easily understand the inner workings of Norgolith, contribute to its
  development and extend its functionality through plugins.
- **Active community engagement**: Norgolith fosters discussions, encourages bug reporting,
  welcomes feature requests and code contributions. Help us shape the future of Norgolith!

## Requirements

| Component |         Requirement          |
|-----------|------------------------------|
| Build     | C/C++ compiler, Rust >= 1.77 |
| Runtime   | pandoc                       |

## Usage

Compile the project using the `optimized` Cargo profile (recommended).

```
$ cargo build --profile optimized && ./target/optimized/norgolith --help

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

Run `cargo install --profile optimized --path .` to compile and install Norgolith in your `~/.cargo/bin` directory.

## Community

Join the Neorg community and get help or discuss about the project:

- [Discord server](https://discord.gg/T6EgTAX7ht)

## License

This project is licensed under the GNU General Public License v2 (GPLv2).
You can find the license details in the [LICENSE](./LICENSE) file.


[tree-sitter]: https://tree-sitter.github.io/tree-sitter/
[norg-pandoc]: https://github.com/boltlessengineer/norg-pandoc
