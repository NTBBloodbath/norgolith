This branch holds the Norgolith documentation site source code. It is licensed under GPL-2.0
license.

Make sure to follow the steps listed in [this gist](https://gist.github.com/cobyism/4730490) to contribute
to the documentation site. For example:

```sh
# Once you ran `lith build` and everything looks okay
git add public && git commit -m "Commit body, follow semantic conventions"

# Push the public directory as a subtree to the `gh-pages` branch
git subtree push --prefix public origin gh-pages
```
