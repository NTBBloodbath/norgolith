# Norgowind
Norgolith <3 TailwindCSS.

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
> you plan to modify the CSS of the theme by hand.

### Configuration
Besides the default `norgolith.toml` configuration options, Norgowind theme also requires the following configuration fields to be present:

```toml
# Custom additional configuration options with example values
[extra]
license = "GPLv2" # Optional
footer_author_link = "https://github.com/NTBBloodbath" # Optional

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

## License
Norgowind is licensed under MIT license.
