# Changelog

## [0.5.0](https://github.com/NTBBloodbath/norgolith/compare/v0.4.0...v0.5.0) (2026-06-16)


### Features

* add configurable content collections and categoriesDir ([e6d1a5c](https://github.com/NTBBloodbath/norgolith/commit/e6d1a5cc75b7fc3c13e3945f0115d51631f68668))
* **build:** expose lith version with git commit hash for dev builds ([111ee70](https://github.com/NTBBloodbath/norgolith/commit/111ee70318e090c72f41d197fba20e4d4b09d0b8))
* **config:** add SiteConfig field validation ([af6a40f](https://github.com/NTBBloodbath/norgolith/commit/af6a40f230cb01a177c84b2572c1c69354156ac6))
* **dev:** config hot-reloading ([1728505](https://github.com/NTBBloodbath/norgolith/commit/1728505404b5c27661d9913910fcdeb7a5b179f9)), closes [#92](https://github.com/NTBBloodbath/norgolith/issues/92)
* **dev:** pre-render all pages into memory for instant responses ([485bbb0](https://github.com/NTBBloodbath/norgolith/commit/485bbb0f9e44ac09248be94c1deda303b0129c67))
* incremental builds via content-hash caching ([466cd8d](https://github.com/NTBBloodbath/norgolith/commit/466cd8d1d639a2cf4dd243be3376b3eae27e234c))
* use rust-norg performance increase branch (experimental) ([34f11c4](https://github.com/NTBBloodbath/norgolith/commit/34f11c4a6d023331c1a4deee528cb6532912d275))
* use XDG_CACHE_HOME for incremental build cache ([c2c6370](https://github.com/NTBBloodbath/norgolith/commit/c2c63707fc867c7dae98163572ec34d08eb0c623))


### Bug Fixes

* **build:** join validation errors with newline for readability ([3106055](https://github.com/NTBBloodbath/norgolith/commit/3106055c892de6c2e12e195345c3794b45eb0c59))
* **build:** log WalkDir errors instead of silently discarding ([412f0e1](https://github.com/NTBBloodbath/norgolith/commit/412f0e1fcfe791ff07ee0a7fdd27372ae4722633))
* **build:** only validate RSS templates as RSS ([7c73022](https://github.com/NTBBloodbath/norgolith/commit/7c7302216cdce40ee9ed8b278631bf1c967602fa))
* **build:** replace bare unwraps with proper error handling ([607e46d](https://github.com/NTBBloodbath/norgolith/commit/607e46d44032d9e55a08b645a327fd02755a0d64))
* **cache:** populate build cache for incremental builds ([4e6d4fe](https://github.com/NTBBloodbath/norgolith/commit/4e6d4fe3ca9bbfb05f3bad1b3818d18593eac98d))
* **config:** do not allow negative numbers in RSS ttl values ([9e3a7ee](https://github.com/NTBBloodbath/norgolith/commit/9e3a7eec6d745a88ba66f389cce15dea9eb2452c))
* **dev:** acquire posts lock once in category index handler ([f784044](https://github.com/NTBBloodbath/norgolith/commit/f784044bc0af9b676f602f8c354ffbd41f4dcd5a))
* **dev:** clearly log when livereload fails ([50da25d](https://github.com/NTBBloodbath/norgolith/commit/50da25d9b2656c999bcfe3fcd38f889fa382ab60))
* **dev:** don't crash dev server when browser can't open ([2b6a2de](https://github.com/NTBBloodbath/norgolith/commit/2b6a2de30dd6ceb63f9f60390003f3aa9a1d53f7))
* **dev:** posts list empty in templates due to collection key collision ([992c804](https://github.com/NTBBloodbath/norgolith/commit/992c804bd77de47e70cdf86c038154c3f7fcb85d))
* **dev:** posts list empty in templates due to collection key collision ([992c804](https://github.com/NTBBloodbath/norgolith/commit/992c804bd77de47e70cdf86c038154c3f7fcb85d))
* **dev:** treat zero receivers as `Ok(())` instead of `Err` in `send_reload()`. ([50da25d](https://github.com/NTBBloodbath/norgolith/commit/50da25d9b2656c999bcfe3fcd38f889fa382ab60))
* **dev:** uppercase Ok in send_reload ([fd81af1](https://github.com/NTBBloodbath/norgolith/commit/fd81af14a2cfa487432356abdb0ff81fbb071bf3))
* **dev:** use strip_prefix result directly instead of contains string check ([ddd2f90](https://github.com/NTBBloodbath/norgolith/commit/ddd2f9042631df6e220126f996f7c1af23f35ad9))
* **docs:** improve site layout and center the content ([f8e8ced](https://github.com/NTBBloodbath/norgolith/commit/f8e8ced214962208f5b04a0013396ca8453f5866))
* **fs:** remove redundant empty-dir check in find_in_previous_dirs ([2b6a2de](https://github.com/NTBBloodbath/norgolith/commit/2b6a2de30dd6ceb63f9f60390003f3aa9a1d53f7))
* **init:** typo in Norgolith ([d7009c6](https://github.com/NTBBloodbath/norgolith/commit/d7009c6cfa9eea5110048c9af877f05a13009dd0))
* **net:** eliminate TOCTOU port race in dev server ([f901bb3](https://github.com/NTBBloodbath/norgolith/commit/f901bb36b916d410244e717d71e0dd1d7a3eb1e7))
* **preview:** add percent-decoding to sanitize_path ([f1f3cc7](https://github.com/NTBBloodbath/norgolith/commit/f1f3cc791c0cd7b0f2649f47bf45aca6797bef64))
* **schema:** return `ConstraintViolation` schema error instead of panicking on invalid regex patterns ([071c23b](https://github.com/NTBBloodbath/norgolith/commit/071c23b0ad9fd01bf0d445d5b15e89754995eafa))
* **schema:** send warning message when a condition is absent from post metadata ([a446158](https://github.com/NTBBloodbath/norgolith/commit/a4461584419501367f27a12d4e4c8828a191b06f))
* **schema:** validate array item types against items definition ([f43ad49](https://github.com/NTBBloodbath/norgolith/commit/f43ad495966ef4cf2c86092f6ebbb6665781cd0b))
* **shared:** use starts_with for collection permalink matching ([03a56c5](https://github.com/NTBBloodbath/norgolith/commit/03a56c569603c9e63353993db0cbdc3ee5420dfc))
* **shared:** warn on invalid post date instead of silently sorting to epoch ([420e4cf](https://github.com/NTBBloodbath/norgolith/commit/420e4cf080c1ef7dbdb7d12e8c610ac0728a7ecb))
* **shared:** warn on metadata conversion errors instead of silently dropping ([70c87ea](https://github.com/NTBBloodbath/norgolith/commit/70c87ea388d37e668a2d8d7239cd183a3a7764b9))
* **tera:** escape HTML in TOC output to prevent XSS ([bbd6607](https://github.com/NTBBloodbath/norgolith/commit/bbd66074a4f29957e632323b272560ae763c0412))
* **tera:** replace panic-inducing unwraps with proper error propagation ([1d04c86](https://github.com/NTBBloodbath/norgolith/commit/1d04c860a87f01bf4771c3038987ff173addd6db))
* **theme:** handle root path in backup dir resolution ([988ecf2](https://github.com/NTBBloodbath/norgolith/commit/988ecf200916206e450fac9fdf6d5bc728b5ef2c))
* **theme:** move blocking I/O off tokio runtime ([6e9cc5f](https://github.com/NTBBloodbath/norgolith/commit/6e9cc5f959a37a4121af6ca184049283a535117d))
* **theme:** use to_string_lossy for non-UTF-8 filenames ([ae291a0](https://github.com/NTBBloodbath/norgolith/commit/ae291a03a22a957714a250c92c9ef489333dc371))


### Performance Improvements

* **build:** buffer rendered pages and write sequentially ([efbac38](https://github.com/NTBBloodbath/norgolith/commit/efbac382f2292c8773c7ad6d5ed7bb04c1978758))
* **build:** cache href regex with OnceLock ([1d86e9b](https://github.com/NTBBloodbath/norgolith/commit/1d86e9b7c39de3645fca0c47b879eda656d94ae4))
* **build:** migrate build.rs to sync rayon parallelism ([6c3e083](https://github.com/NTBBloodbath/norgolith/commit/6c3e0834a880709b35156137313039fd803f4664))
* **build:** use RwLock for build cache ([0f8d545](https://github.com/NTBBloodbath/norgolith/commit/0f8d545a8e4a6da2a1a80f4f6736ec1cf1f04e17))
* **cache:** avoid recomputing global hash on save ([b41e6f6](https://github.com/NTBBloodbath/norgolith/commit/b41e6f6d2417c74dde5b55bc8743ad2962a74ea7))
* eliminate double parsing for collection posts ([8ee9910](https://github.com/NTBBloodbath/norgolith/commit/8ee991023cf91167fb7eba9a890f4976ac66f2cc))
* optimize build pipeline with shared context, VecDeque, and parallel metadata ([0500ec7](https://github.com/NTBBloodbath/norgolith/commit/0500ec7078b8f530dd3930671707c4b0938d6a81))
* pass carryover tags by reference in HTML converter ([a7d8e25](https://github.com/NTBBloodbath/norgolith/commit/a7d8e257b75f03448ae8d19265fa033afe45fd74))
* skip HTML conversion for draft posts ([ec57c38](https://github.com/NTBBloodbath/norgolith/commit/ec57c3801069ba31d31cda760dce221dbb136314))
* skip HTML conversion for draft posts in dev server too ([c628321](https://github.com/NTBBloodbath/norgolith/commit/c62832127abeb3b8ab5181387d7c832a74289eee))

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
