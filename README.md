<div align="center">

<img src="./res/norgolith_text.png" alt="Norgolith logo"/>

---

The monolithic Norg static site generator built with Rust. Leverage the precision of [rust-norg]
syntax validation with the power of our in-house Norg-to-HTML conversion library to create stunning
static sites from your Norg content with an unparalleled performance.

</div>

## 🌟 Features / Why use Norgolith?

Norgolith caters to both developers and content creators seeking a powerful and user-friendly
solution for crafting static websites from Norg content. Here's what makes Norgolith stand out:

### ✍️ For content creators seeking an easy-to-use conversion tool

- **Effortless Norg workflow**: write your content in Norg and let Norgolith handle the technical
  complexities. Seamlessly convert your Norg documents to clean and validated HTML.
- **Focus on content, not code**: leave the website maintenance and complexities to Norgolith. You
  can safely concentrate on creating compelling content for your audience without worrying about
  weird bugs or unexpected invalid syntax from reaching your production site.
- **Live preview**: See your Norg content rendered as HTML in real-time as you edit it, allowing for
  a smooth writing experience and easy iteration on your website's design.

### ⚙️ For developers who value validation and control

- **Robust syntax validation**: leverage the power of the [rust-norg] parser to catch errors in your Norg
  documents before conversion. This ensures clean, well-structured HTML output and avoids surprises
  later during the development process.
- **Modern Rust codebase**: contribute with ease! Norgolith boasts a clean, well-structured codebase.
  This allows you to easily understand the inner workings of Norgolith, contribute to its
  development and extend its functionality through plugins.
- **Active community engagement**: Norgolith fosters discussions, encourages bug reporting,
  welcomes feature requests and code contributions. Help us shape the future of Norgolith!

## 📝 Requirements

| Component | Requirement  |
|-----------|--------------|
| Build     | Rust >= 1.77 |

## 📚 Usage

Compile the project using the `optimized` Cargo profile (recommended).

```
$ cargo build --profile optimized && ./target/optimized/norgolith --help

The monolithic Norg static site generator

Usage: norgolith <COMMAND>

Commands:
  init   Initialize a new Norgolith site
  serve  Build a site for development
  new    Create a new asset in the site (e.g. 'new -k content post1.norg' ->
         'content/post1.norg') and open it using your preferred system editor
  build  Build a site for production
  help   Print this message or the help of the given subcommand(s)

Options:
  -v, --version  Print version
  -h, --help     Print help
```

## ⚡ Install

Run `cargo install --profile optimized --path .` to compile and install Norgolith in your `~/.cargo/bin` directory.

## ❄️ Developing and testing with Nix

The Norgolith repository includes a Nix flake for development and testing purposes in the root directory. This section outlines how to
use the Nix flake for these workflows.

<details>
<summary>Usage</summary>

### Building Norgolith

```sh
# For extra verbosity add '--show-trace -Lv'
nix build .
```

This command builds Norgolith using Nix and places the executable in the `result` directory.

### Build and run Norgolith:

```sh
# For extra verbosity add '--show-trace -Lv'
nix run .
```
This command builds Norgolith the same way the `nix build` command would (including the `result`
directory symlink), and then proceeds to run the project.

### Development shell

```sh
# For extra verbosity add '--show-trace -Lv'
nix develop .
```

This command creates a development shell pre-configured with all the dependencies required to build
and test Norgolith. Inside the development shell, you can directly work on the source code and test
changes.

### Nix-direnv integration (optional)

For a more convenient development experience, consider using
[nix-direnv](https://github.com/nix-community/nix-direnv). With the `nix-direnv` integration,
entering the project directory will automatically activate the development shell defined in the
flake.

</details>

## 🚀 Community

Join the Neorg community and get help or discuss about the project:

- [Discord server](https://discord.gg/T6EgTAX7ht)

## 💌 Supporting Norgolith

Developing and maintaining open-source projects takes time and effort. If you find Norgolith
valuable and would like to support its continued development, here are some ways you can help:

- **Star this repository on GitHub**: this helps raise awareness and shows the project is actively
  maintained.
- **Contribute code or documentation**: we welcome contributions from the community.
- **Spread the word**: let others know about Norgolith if you think they might benefit from it.
- **Financial Support (Optional)**: if you'd like to offer financial support, you can consider using
  my Ko-fi page (linked in the repository). Any amount is greatly appreciated and helps me invest
  further time in Norgolith's development.

## 📖 License

This project is licensed under the GNU General Public License v2 (GPLv2).
You can find the license details in the [LICENSE](./LICENSE) file.


[rust-norg]: https://github.com/nvim-neorg/rust-norg
