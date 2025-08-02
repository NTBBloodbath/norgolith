# Norgowind
Norgolith <3 TailwindCSS.

## Demo

If you want to see this theme in action, you can either go to [my blog](https://amartin.beer) or even the [official Norgolith documentation](https://norgolith.amartin.beer).

## Installation
```bash
lith theme pull github:NTBBloodbath/norgowind
```

> [!IMPORTANT]
>
> Currently, Norgowind requires the latest Norgolith commit in the master branch in order to work.

## Usage

> [!TIP]
>
> As Norgowind has been written using the standalone TailwindCSS CLI, you might want to use it if
> you plan to modify the CSS of the theme by hand. See the [Tailwind Reloading](#tailwind-reloading) section.

### Configuration
Besides the default `norgolith.toml` configuration options, Norgowind theme also requires the following configuration fields to be present:

```toml
# Custom additional configuration options with example values
[extra]
license = "GPLv2" # Optional
favicon_path = "/assets/norgolith.svg" # Fallback to the default norgolith favicon
footer_author_link = "https://github.com/NTBBloodbath" # Optional
enable_mermaid = true # If you want to use Mermaid.js for diagrams and charts

# Link_name = "url"
# e.g.
# blog = "/posts"
# GitHub = "https://github.com/NTBBloodbath/norgolith"
[extra.nav]

# Link_name = "url"
# GitHub = "https://github.com/NTBBloodbath/norgolith"
[extra.footer]
```

### Templates
Norgowind provides the following templates:
```
templates
├── partials
│   ├── footer.html  <- Footer content
│   └── nav.html     <- Header navbar
├── base.html        <- Main template which gets extended by any other template
├── categories.html  <- Categories list
├── category.html    <- Category posts list
├── default.html     <- Default template for all content
├── home.html        <- Homepage
├── post.html        <- Blog post
└── posts.html       <- Posts list
```

In order to use a certain template, use the `layout` metadata field in your content files, e.g. if
you are writing a blog post:
```norg
layout: post
```

> [!TIP]
>
> Remember that Norgolith expects your blog posts to reside in the `content/posts` directory.

### Additional styling
Norgowind adds certain additional styling classes for blockquotes (add them to your blockquotes
using `+html.class` weak carryover tags):
- `tip` (green)
- `note` (blue)
- `important` (violet)
- `warning` (yellow)
- `error` (red)

![blockquotes.png](https://github.com/user-attachments/assets/d45e2e97-5e3b-43cb-8077-a16f737259b9)

### Additional metadata fields
Norgowind also accepts and uses the following opt-in content metadata:

- `truncate`: configures the truncate characters length in the recent post cards.
- `truncate_char`: configures the truncate character, do not define it to use the default ellipsis. Leave it empty to disable the truncate character.

### Tailwind Reloading
By default, Tailwind's configuration in Norgowind will see content files, along with user and theme
templates. Each new class added to content using a weak carryover tag `+html.class` will
automatically be added to the styling file.

It is highly recommended to have the TailwindCSS CLI installed and run the following command during
development:
```sh
tailwindcss -i theme/assets/css/tailwind.css -o theme/assets/css/styles.min.css --minify --watch
```

## License
Norgowind is licensed under MIT license.
