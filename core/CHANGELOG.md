# Changelog

All notable changes to this project will be documented in this file.

This project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
## [0.4.0](https://github.com/norgolith/core/releases/tag/norgolith-v0.4.0) `norgolith` - 2026-06-30

### Added
- add SEO (sitemap.xml, robots.txt) and OpenGraph meta tags
- *(sdk)* implement bridge functions and working register_plugin! macro

### Fixed
- rewrite plugin list output with vertical per-plugin layout
- *(plugin)* harden plugin system
- *(plugin)* remove double JSON extraction in hook handlers

### Other
- small readme updates
- ditch `optimized` Cargo profile
- [**breaking**] rename binary from `norgolith` to `lith`
- move to GPLv2 from GPLv3
- Initial commit
