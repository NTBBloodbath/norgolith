# Changelog

## [0.5.0](https://github.com/norgolith/core/compare/norgolith-v0.4.0...norgolith-v0.5.0) (2026-07-01)


### Features

* add SEO (sitemap.xml, robots.txt) and OpenGraph meta tags ([d24c924](https://github.com/norgolith/core/commit/d24c924d94e28c1118f690e1f88ec094fefbff82))
* **sdk:** implement bridge functions and working register_plugin! macro ([48f3f9b](https://github.com/norgolith/core/commit/48f3f9b1eeec076be6b87c36773944395c4ee8e5))


### Bug Fixes

* **plugin:** harden plugin system ([4253d30](https://github.com/norgolith/core/commit/4253d304cc98788a10389252621b7efb6606131c))
* **plugin:** remove double JSON extraction in hook handlers ([cb39e6c](https://github.com/norgolith/core/commit/cb39e6c1db29e05ccb8ce02fd19a6a3bc93dcf06))
* rewrite plugin list output with vertical per-plugin layout ([3df145b](https://github.com/norgolith/core/commit/3df145b9087c4d30b47c8eb179686a136a7fcdbb))


### Refactoring

* **workspace:** migrate to monorepo with core/ and sdk/ crates ([3e27e27](https://github.com/norgolith/core/commit/3e27e273181d2f01af4d2c5b9057b32432f15d1a))


### CI

* fix release-please manifest and add SDK publishing ([14523a0](https://github.com/norgolith/core/commit/14523a067264e5673aaa41774096e2d7571a6805))


### Miscellaneous

* bump core to 0.5 ([ab3fa46](https://github.com/norgolith/core/commit/ab3fa46fd549e9a26772b3e9a1b3a113760907b5))

## [0.4.0](https://github.com/NTBBloodbath/norgolith/compare/v0.3.2...v0.4.0) (2026-05-15)


### Features

* auto-discover and render all XML templates as feeds ([11ba1e7](https://github.com/NTBBloodbath/norgolith/commit/11ba1e775c7466365bb39dafcd8ed5630099d805)), closes [#111](https://github.com/NTBBloodbath/norgolith/issues/111)
* **build:** add styled per-step output to lith build command ([9471821](https://github.com/NTBBloodbath/norgolith/commit/94718219ca4f9b97b4e644031d07eaf616c403b6))
* **dev:** add colored, compact request logging to dev server ([ca22a39](https://github.com/NTBBloodbath/norgolith/commit/ca22a3954eef0f477a894989e63b1d1a93d826e2))
* **dev:** add Ctrl-D for graceful development server shutdown ([39932de](https://github.com/NTBBloodbath/norgolith/commit/39932de1fc78f92b072be7643f7f4a616b3af361))
* **dev:** higher padding between HTTP Path and HTTP status indicator ([609dd76](https://github.com/NTBBloodbath/norgolith/commit/609dd76b4592f8ceac97bc888066f0149a3c8a33))
* **dev:** resolve symlinks in watched site paths ([140a173](https://github.com/NTBBloodbath/norgolith/commit/140a173e0d45f5466e3e30cfa5e8c80ee3781901))


### Bug Fixes

* **converter:** handle NorgAST::List variant introduced in latest rust-norg ([8066641](https://github.com/NTBBloodbath/norgolith/commit/80666411eeca341ba29491bd4533caa4b6954441))
* **converter:** repair unreachable panic for NorgAST::List with Quote type ([8066641](https://github.com/NTBBloodbath/norgolith/commit/80666411eeca341ba29491bd4533caa4b6954441))

## [0.3.2](https://github.com/NTBBloodbath/norgolith/compare/v0.3.1...v0.3.2) (2026-05-06)


### Bug Fixes

* **build:** pin tracing-subscriber to 0.3.19 and update dependencies ([e9bfe6c](https://github.com/NTBBloodbath/norgolith/commit/e9bfe6c72d6e18960dc55052b4974da972232b48))
* **build:** remove `String::leak()` in `minify_css_asset` ([fa19733](https://github.com/NTBBloodbath/norgolith/commit/fa1973378cfc7bb629e342e513104ef4f799b947))
* **clippy:** avoid owned PathBuf allocation in posts filter comparison ([acf7b62](https://github.com/NTBBloodbath/norgolith/commit/acf7b6252f864604fb3d19850091555c3417b844))
* keep public .git directory during build ([6fd806d](https://github.com/NTBBloodbath/norgolith/commit/6fd806db4d9d34d1a4315ff81376d40f2f424821))
* load XML templates (rss.xml) from theme/templates ([069d958](https://github.com/NTBBloodbath/norgolith/commit/069d95814b539f0bbc7c2ac78a604683aafc7923))
* **schema:** correct array min/max constraint operators and add tests ([d1cf410](https://github.com/NTBBloodbath/norgolith/commit/d1cf410326b157f0e12326192dadd3e4cbe5b873))
* **shared:** handle sourceless Tera errors in category render functions ([78a66f3](https://github.com/NTBBloodbath/norgolith/commit/78a66f3e44894577d3da719d34e137d7c6e63aa2))
* **shared:** properly pass the layout name when failing to render a template in `render_norg_page` ([15b7a48](https://github.com/NTBBloodbath/norgolith/commit/15b7a48e90868a567af4e3584ff36c60d6fc8ae4))
* **shared:** sort posts by `created` field using RFC3339 date parsing ([69b0855](https://github.com/NTBBloodbath/norgolith/commit/69b08550ce194185132df6edd65f2ad2e6314c22))
