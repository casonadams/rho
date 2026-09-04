# Changelog

## [0.2.1](https://github.com/casonadams/rho/compare/v0.2.0...v0.2.1) (2026-09-04)


### Features

* **auth:** add openrouter oauth pkce support and cli login options ([5c9617d](https://github.com/casonadams/rho/commit/5c9617d059b4fbfcb4509f0d563375ad0f143150))
* **auth:** split login menu into oauth and api key submenus ([04e35c9](https://github.com/casonadams/rho/commit/04e35c967f07fd10bf73dd2054d45225207a7991))
* **config:** restrict external instructions and skills to ~/.agents and update readme ([ea09cc9](https://github.com/casonadams/rho/commit/ea09cc909cd7158cee0901f7d6ae8546163a3d35))
* **mcp:** implement concurrent server loading and lazy background tool initialization ([c08cbee](https://github.com/casonadams/rho/commit/c08cbee42d60c0cad581395ea82c10edf36fd9ff))
* **provider:** classify oauth providers and credential strategies ([d1dfc46](https://github.com/casonadams/rho/commit/d1dfc4645d76d3bf36e7afc345f8dbeff55e3fe6))
* **tools:** make fd pattern argument optional to match all entries ([0431741](https://github.com/casonadams/rho/commit/0431741d1cee151b17a5ff48f723e683f3a03fb1))

## [0.2.0](https://github.com/casonadams/rho/compare/v0.1.7...v0.2.0) (2026-09-04)


### Features

* **antigravity:** add dynamic token provider with 401 force-refresh retry ([05d3299](https://github.com/casonadams/rho/commit/05d3299b1a6871e1389974c39a9b424f6b3a5a90))
* **antigravity:** add rolling quota fetching, parsing, and countdown formatting ([9907ae4](https://github.com/casonadams/rho/commit/9907ae4ae77c4a66d290b2a7d9ff83aaf45c543b))
* **antigravity:** Google OAuth login and Cloud Code Assist provider ([e2e4f4e](https://github.com/casonadams/rho/commit/e2e4f4e347ff01f3580f7016d65f9bdac9e37641))
* **antigravity:** wire shared auth store into engine model builder ([96f6eac](https://github.com/casonadams/rho/commit/96f6eaca694580910ebc9c78209d3b8946c79208))
* **bash:** add CI, GIT_TERMINAL_PROMPT, and PAGER env vars and expand read-only command list ([3bf3b7a](https://github.com/casonadams/rho/commit/3bf3b7a232a4c62864fce52fde005b848556f940))
* **bash:** preserve timeout/error output, add shell resolution, task abort guard, and binary sanitization ([4ca6237](https://github.com/casonadams/rho/commit/4ca62375ea2a2f78a56207636e1e4b2c8e67011e))
* **config:** increase default max_turns from 100 to 250 ([e37523f](https://github.com/casonadams/rho/commit/e37523f862922edf2dd350325068e627c4c4ed39))
* **context:** hierarchical AGENTS.md and skills discovery ([f7d0ffb](https://github.com/casonadams/rho/commit/f7d0ffb621bba43d78106c1842a31ad3f0e0f069))
* **engine:** add quota tracker caching, live refresh, and turn usage context tracking ([89cf8b1](https://github.com/casonadams/rho/commit/89cf8b1371e8689c200269205a2b850a813072a1))
* **engine:** gate tool-result images by provider capability ([5b16966](https://github.com/casonadams/rho/commit/5b16966e4f796c49670e1d6f546f2107f863c5f7))
* **fd:** add line counts, file size stats, line filtering, and sorting ([5face66](https://github.com/casonadams/rho/commit/5face6696fcc013e56fba211d6e736e84f4832ce))
* **input:** wire session actions to SessionTree, SessionResume, and SessionNew ([db678dd](https://github.com/casonadams/rho/commit/db678dd8fcb3bff472060a9e3a4abec3739f2fd9))
* **plugin:** inline select inputs, prefilled host prompts, vim modal nav ([5e2e93a](https://github.com/casonadams/rho/commit/5e2e93a6b99341e05e8fcf83fc888ba1df196855))
* **process:** add ProcessTreeGuard RAII lifecycle and preemptive event loop ([53ece34](https://github.com/casonadams/rho/commit/53ece340947bf8f656aff1d700aef670f4413e63))
* **process:** add ProcessTreeGuard RAII wrapper, Windows tree kill, and global tracking ([e8c509e](https://github.com/casonadams/rho/commit/e8c509ec56ae71b54164cfc39b93e419d8c89ab5))
* **read:** attach images to read results as inline tool-result blocks ([22a0991](https://github.com/casonadams/rho/commit/22a099143324ff6602be6d2823a6d1f0353aa7eb))
* **read:** port pi's image sniffing and resize pipeline as pure functions ([767afbc](https://github.com/casonadams/rho/commit/767afbc6ae0cf9550e99be2b4364bb849935ab20))
* **read:** port pi's truncateHead and actionable truncation UX ([4e55e8d](https://github.com/casonadams/rho/commit/4e55e8d86462a3f39e10f621233d20554b6a678c))
* **repl:** add double-escape tree shortcut and compaction notices ([6943ed0](https://github.com/casonadams/rho/commit/6943ed0583f35e3d04c4494b4ae9df6fc0fd2aaf))
* **repl:** add interactive /settings modal ([2d9d2e4](https://github.com/casonadams/rho/commit/2d9d2e43a81ae0a7bb90da62d2cc6ae42e785939))
* **repl:** add interactive session resume modal with live search ([3347655](https://github.com/casonadams/rho/commit/3347655e628cae7d53be0030e5d9dbf2b0499a50))
* **repl:** hydrate full conversation transcript and prompt history on resume ([fa4d8fd](https://github.com/casonadams/rho/commit/fa4d8fd2faa0f3ad1b3bff9cec782669ede00d15))
* **repl:** interactive conversation tree modal and checkpoint navigation ([aaa0df7](https://github.com/casonadams/rho/commit/aaa0df7cfc2b63af5dd7fb16388e4334b5b588f6))
* **repl:** modularize slash commands and surface live engine diagnostics in /session ([dcf85f8](https://github.com/casonadams/rho/commit/dcf85f81e2d9e2df8a3d7c4774601b41cae3b51f))
* **repl:** persist clipboard images as png and insert path ([2f540a1](https://github.com/casonadams/rho/commit/2f540a159f54a89abbd492c228399548693d4441))
* **repl:** support checkpoint labeling with Shift+L in tree modal ([831f0b9](https://github.com/casonadams/rho/commit/831f0b9925af1a073326e4d89deb90a042eb210e))
* **repl:** throttle live streaming redraws in user bash runner ([730a78a](https://github.com/casonadams/rho/commit/730a78adb7a208759900718f292016359ce09491))
* **repl:** wire bracketed paste and terminal paste events ([b2e6911](https://github.com/casonadams/rho/commit/b2e6911f5bfe87c9f7f0c7c52b168fa706fecc17))
* **repl:** wire OutputAccumulator and disk spooling to user bash runner ([18c5610](https://github.com/casonadams/rho/commit/18c5610b5521a9dfe962dc053128961fc680e5e0))
* **rg:** port pi's truncateLine and byte-capped grep output ([19f653f](https://github.com/casonadams/rho/commit/19f653f077f32f5a0d18490a0d9547e56fd719ba))
* **skills:** add lean and simplify built-ins plus disable_built_in_skills flag ([f048ed4](https://github.com/casonadams/rho/commit/f048ed412c1c11aa28d74861b9bb11be111334a2))
* **tokens:** generalize claude context window matching to 200k ([dfc7430](https://github.com/casonadams/rho/commit/dfc7430b4b1f4c25a562c8d06dbe89372bd7ac34))
* **tools:** add fd tool for bounded file discovery ([f6e4fd7](https://github.com/casonadams/rho/commit/f6e4fd70a209fff101d805004f628f15d93a9ff3))
* **tools:** add rg tool for bounded content search ([47b5085](https://github.com/casonadams/rho/commit/47b5085ad01f502d27348902034a09ebfe97f401))
* **ui:** add direct bash border mode and osc 133 semantic zones ([8a0423e](https://github.com/casonadams/rho/commit/8a0423e1481ca0965cb46f7bf383c55d7784105d))
* **ui:** add in-place running tool widget and streaming user bash runner ([65656d4](https://github.com/casonadams/rho/commit/65656d4697cd948b68e9f805a604e32cea3dcc3f))
* **ui:** add intra-line word diffing and syntax-highlighted read output ([b2023bf](https://github.com/casonadams/rho/commit/b2023bf6582c029b6d0e644c35c9f0e4dcd33de2))
* **ui:** add line numbers to diffs and previews, refine tool streaming and cards ([0660d1b](https://github.com/casonadams/rho/commit/0660d1bb6597085d6e01ea71ad0454b01b6df79f))
* **ui:** align skill read and invocation rendering with pi ([d497578](https://github.com/casonadams/rho/commit/d497578bb0e32e2ce764acb5d72ea3dcb6e7a965))
* **ui:** bound RunningTool output and pre-slice widget layout tail ([e2b1a2d](https://github.com/casonadams/rho/commit/e2b1a2def6ecde529e1cb6aa42a816d057de353b))
* **ui:** clean working spinner and collapse general file reads ([2576ca4](https://github.com/casonadams/rho/commit/2576ca4a112360d7c3f20813d588163e44735387))
* **ui:** collapse large pastes into atomic editor markers ([0445ed0](https://github.com/casonadams/rho/commit/0445ed039d3fdbe57903f835d80ecd386f2e24a4))
* **ui:** decouple Ctrl+T thinking toggle and add compact thinking placeholder ([db16517](https://github.com/casonadams/rho/commit/db165174601f4596c81ec6218290c79cb4aeb30b))
* **ui:** embed rho label in top editor divider ([1f8e659](https://github.com/casonadams/rho/commit/1f8e65903ba13d901e14770c8b1e12fd32b168de))
* **ui:** fuzzy-match selectors and merge provider catalogs from disk ([4f63610](https://github.com/casonadams/rho/commit/4f63610a6a8556005a2802cefa6153e607151117))
* **ui:** gate divider version label behind show_version config ([1acb564](https://github.com/casonadams/rho/commit/1acb5644360c4e33474cc6366d7b14b373b6cf26))
* **ui:** rename divider gating to show_label with opt-in branding ([dac6311](https://github.com/casonadams/rho/commit/dac6311a07727d0635ec8d564b9fe18972f2572b))
* **ui:** render tool outputs when expanded for search and custom tools ([70d976d](https://github.com/casonadams/rho/commit/70d976df28442a2c73e29528e008b9d43190691c))
* **ui:** show crate version in top divider label ([2396000](https://github.com/casonadams/rho/commit/2396000e1ee3f28b4d7997a603bc1adc9eaa055b))
* **ui:** simplify status notices to clean dim text matching pi ([5c884e7](https://github.com/casonadams/rho/commit/5c884e73ce46c9a463e001ef36a628e41a7cd662))
* **ui:** suppress widget card for fast non-streaming tools and route status to footer ([4bc1076](https://github.com/casonadams/rho/commit/4bc10762b5563670199845dd993cd44bc95a6e0e))
* **write,edit:** add atomic writes, CRLF normalization, whitespace mismatch hints, and write module split ([af29f4a](https://github.com/casonadams/rho/commit/af29f4afbaa9211eeb457863ec00bef825994bf3))


### Bug Fixes

* **platform:** serialize clipboard access; align rg output with rg ([9ae5de9](https://github.com/casonadams/rho/commit/9ae5de910f66c3414b47fd0e5d4baf84bea18daf))
* **plugin:** include input field in host select test fixtures ([7f2b0ce](https://github.com/casonadams/rho/commit/7f2b0cef1532001c05bb89995cbd483e45ef0aa0))
* **presentation:** handle multibyte char boundaries in summaries and string truncation ([99d8ef2](https://github.com/casonadams/rho/commit/99d8ef25d2850b9842956b6e03cde47be5d1c65c))
* **process:** kill whole process groups so descendants don't survive ([5b8b4b7](https://github.com/casonadams/rho/commit/5b8b4b7db53bc5a3dd87f4b0f1325bbfb37057d0))
* **repl:** guard /skills picker behind a real terminal ([7b6ce56](https://github.com/casonadams/rho/commit/7b6ce56f96014e53a13162ddc28ee640913f62bf))
* **ui:** add /settings to autocomplete and polish modal layout formatting ([dfa45e3](https://github.com/casonadams/rho/commit/dfa45e31bfa30a7cfd9b500469d384a851a4c82b))
* **ui:** eliminate visual bounce during fast tool execution via event batching and single-frame completion ([273facc](https://github.com/casonadams/rho/commit/273faccd55c45660c5ef8bbfad6b498476f5ba15))
* **ui:** improve markdown stream styling, task lists, and horizontal rules ([6dae443](https://github.com/casonadams/rho/commit/6dae443daf11c5fa34e8c10358751fa83ca76171))
* **ui:** match autocomplete descriptions to the footer dim ([38bea7f](https://github.com/casonadams/rho/commit/38bea7fac85e43a17f5f944a41bf3c2e1dbbcbe0))
* **ui:** normalize markdown and agent output spacing and decompose markdown modules ([0e9c1c0](https://github.com/casonadams/rho/commit/0e9c1c025517c43177b1fa424d7e1cb9a6ac2b5d))
* **ui:** single-frame tool completion and tab-safe output blocks ([f4100f6](https://github.com/casonadams/rho/commit/f4100f6894902e8a93d74070f025ec809702e6ae))
* **ui:** strip trailing markdown blanks and fix interactive layout cursor row ([edb898c](https://github.com/casonadams/rho/commit/edb898c9e7ae131883b957a8918885f5f439e117))


### Miscellaneous Chores

* **release:** keep workspace dependencies in sync ([a793530](https://github.com/casonadams/rho/commit/a793530f397c348a6f05c60558573d1971f44e54))

## [0.1.7](https://github.com/casonadams/rho/compare/v0.1.6...v0.1.7) (2026-09-03)


### Bug Fixes

* **provider:** configure no_proxy and pass api key to gemini client builder ([#7](https://github.com/casonadams/rho/issues/7)) ([7b24314](https://github.com/casonadams/rho/commit/7b243141bfcf81a367e9328701cec4abdc1206ca))

## [0.1.6](https://github.com/casonadams/rho/compare/v0.1.5...v0.1.6) (2026-09-02)


### Features

* **examples:** update node notifier, rag injector, and rust guard to use tui blocks and status footer ([93642a6](https://github.com/casonadams/rho/commit/93642a67903e792ebe828cba3dc68e8647a8ea65))
* **plugin:** support host/tools/list tool reflection, turn lifecycle hooks, and REPL slash command routing ([b9e58d7](https://github.com/casonadams/rho/commit/b9e58d79dbe52262a4967264abd65450389e0663))
* **plugin:** support host/ui/block, host/ui/set_status, RAG document injection, and streaming deltas ([25d246c](https://github.com/casonadams/rho/commit/25d246c86b2ec2dc2ea66841255b9e658acf2edf))


### Bug Fixes

* **ui:** enable vertical padding and transcript line spacing for plugin block cards ([29fb67b](https://github.com/casonadams/rho/commit/29fb67b7c1742acfd1565e0178713637fd9741c8))

## [0.1.5](https://github.com/casonadams/rho/compare/v0.1.4...v0.1.5) (2026-09-02)


### Features

* **examples:** enhance node notifier to emit real-time host notifications ([bbf77ca](https://github.com/casonadams/rho/commit/bbf77ca6eef2511ceedeff038b3cbade2308d2d7))
* **examples:** update python guard example to prompt for every tool call ([9eee18c](https://github.com/casonadams/rho/commit/9eee18c64e1e25d786f2e25b35dc88446bbd8649))


### Bug Fixes

* **examples:** match any rm -rf deletion in python guard example ([ba6a3c3](https://github.com/casonadams/rho/commit/ba6a3c332c802aca748a7089084879435312cc40))

## [0.1.4](https://github.com/casonadams/rho/compare/v0.1.3...v0.1.4) (2026-09-02)


### Features

* **plugin:** implement decoupled rig-native protocol, host bus, and daemon hook ([b7ebfe8](https://github.com/casonadams/rho/commit/b7ebfe8dfb846e57c05f5578fe758c2c353cec83))
* **plugin:** introduce native RhoPlugin trait, hook stack integration, and ecosystem documentation ([9c1876a](https://github.com/casonadams/rho/commit/9c1876a9a49ae88c40d2431252a636083ce1e783))
* **plugin:** introduce official rho-plugin-sdk crate ([2293092](https://github.com/casonadams/rho/commit/2293092401ca2851a5d668bc8696e50fc2881338))
* **plugin:** support dynamic plugin tools in engine builder and add display transformer pipeline ([6510902](https://github.com/casonadams/rho/commit/65109024f5eb9fce9f1da312e679ad1f9f890cbe))
* **plugin:** update REPL /plugin command, CLI inspection, and create-plugin skill for rig-native plugins ([97808f5](https://github.com/casonadams/rho/commit/97808f50f2baa5ed48b6f6c923b2569d3d182d4c))


### Bug Fixes

* **plugin:** make daemon stdout reader concurrent, extend timeout, and enforce fail-closed hooks ([0e6c783](https://github.com/casonadams/rho/commit/0e6c7834afa0e5284eed76f17516b6cc6c2793ab))

## [0.1.3](https://github.com/casonadams/rho/compare/v0.1.2...v0.1.3) (2026-09-02)


### Features

* **provider:** fetch real context windows for local ollama models ([c2f3bdd](https://github.com/casonadams/rho/commit/c2f3bdd34579f1dc1f008a34906d8a39b0ca3326))

## [0.1.2](https://github.com/casonadams/rho/compare/v0.1.1...v0.1.2) (2026-09-02)


### Bug Fixes

* **crates:** add repository and readme metadata ([1211bf1](https://github.com/casonadams/rho/commit/1211bf1e8378d26195070195d79c6ec9755780bc))

## [0.1.1](https://github.com/casonadams/rho/compare/v0.1.0...v0.1.1) (2026-09-02)


### Features

* **agent:** implement autonomous subagents with live streaming and add todo tool ([e4aa852](https://github.com/casonadams/rho/commit/e4aa85275fc872915382267403d4768312bf8cf0))
* **antigravity:** add runtime candidate fallbacks, exponential backoff, and 429/503 retry loop ([79f7d96](https://github.com/casonadams/rho/commit/79f7d96210b93e8e09af36bbd36ab34895776979))
* **antigravity:** parse group quota buckets and format remaining pool in footer ([39504d3](https://github.com/casonadams/rho/commit/39504d32737e39d263147f1a46c89af3949bef9b))
* **auth:** unified OAuth PKCE login, dynamic model discovery, and pi-styled modal UI ([d6ecd40](https://github.com/casonadams/rho/commit/d6ecd40c9878646a0191071fcc87a4c95ba248f9))
* **bash:** add tail truncation, temp spillover, and smooth preview rendering ([7a9b30a](https://github.com/casonadams/rho/commit/7a9b30a94e3e1a2cf353743aca78f7599bc10584))
* **builtin,host:** extract rho-plugin-builtin and remove legacy Extension machinery ([879a919](https://github.com/casonadams/rho/commit/879a91924e3b2b3427aeb7fb5f64d526d089cc0d))
* **cli:** align CLI flags and positional arguments with pi.dev ([aeb6ca4](https://github.com/casonadams/rho/commit/aeb6ca4f91c2bf6e18d1ac44aae4e84824d0789d))
* **core:** implement session tree DAG, prompt templates, and headless RPC mode ([4687483](https://github.com/casonadams/rho/commit/4687483db64354f97d9f24ffff0059cdbab6e8f7))
* **engine:** resume thinking spinner between tools and polish session modularization ([f2f8d2c](https://github.com/casonadams/rho/commit/f2f8d2c6d8bec680c6d8353e63f446cd23a8d1d8))
* **harness:** config-defined providers, /reload, and /export without recompile ([debb014](https://github.com/casonadams/rho/commit/debb014b5f8f563d6c71187a8bd0f4439c459173))
* **headless:** support headless build profile and structured NDJSON presenter ([d38f9b7](https://github.com/casonadams/rho/commit/d38f9b7312ea259534cd8e4a114a510c05f16b9d))
* initialize rust-ai agentic coding harness with terminal UI and tools ([4999fed](https://github.com/casonadams/rho/commit/4999fedc0c46a9335a3a27f3666ce9a42dfb3673))
* **intent:** clean question prompt formatting with line break and expand informational prompt detection ([92356f6](https://github.com/casonadams/rho/commit/92356f668a176b9cc420251cffc150e40a33b5c7))
* **intent:** persist and recover active tasks ([1ce3efa](https://github.com/casonadams/rho/commit/1ce3efa3d9a311f6fd4233be34e66d95486cc79b))
* **plugin:** add plugin lifecycle system, embedded skills, and custom oauth support ([973f2d1](https://github.com/casonadams/rho/commit/973f2d1662ef23f108ef405440a1ab0410e7f554))
* **plugin:** add supervised external adapters ([5f3e030](https://github.com/casonadams/rho/commit/5f3e030bae67938f7945af3cae13fcb6856fcf99))
* **plugin:** compose restrictive permission policies at dispatch ([a37b168](https://github.com/casonadams/rho/commit/a37b168496b2bc46283d0ab225a8c0e6dddb9468))
* **plugin:** enhance plugin platform with SDK server helpers, plugin config, and lifecycle events ([1ec6342](https://github.com/casonadams/rho/commit/1ec63420d8ef5efb6cfe81534001ac10680c86a2))
* **plugin:** establish capability platform foundation ([832ffa7](https://github.com/casonadams/rho/commit/832ffa78f78c77edcc69925e82e6c42c5c4a9b10))
* **plugin:** route providers through a neutral runtime ([863c41c](https://github.com/casonadams/rho/commit/863c41c5f711dad913792b7fa978d2164459aca1))
* **plugin:** route tools through capability dispatch ([1e577e4](https://github.com/casonadams/rho/commit/1e577e4df099774d181abc0117150dc9058b6561))
* **plugin:** scan cargo bin and path for plugin discovery ([cf64559](https://github.com/casonadams/rho/commit/cf64559fd3b604089b7e13d71b387694812ff543))
* **prompts:** organize tool prompt definitions into prompts/tools and link to registry ([df0b14b](https://github.com/casonadams/rho/commit/df0b14b45e4f071c4b9b4ae4abdfc868a07b1281))
* **provider:** add native google antigravity oauth subscription support ([5bfd779](https://github.com/casonadams/rho/commit/5bfd779003ebba553e2df647740162af03631279))
* **quota:** order 7d limit before 5h and format countdown without parentheses ([62d7cf8](https://github.com/casonadams/rho/commit/62d7cf81028b77bd06913cfa8628aab323741a8a))
* **repl:** add --continue flag, /session, /compact, /tree, and /rewind commands ([945c67e](https://github.com/casonadams/rho/commit/945c67ec503177ae59abb830be30255b44018371))
* **repl:** add dedicated terminal input reader ([72d6434](https://github.com/casonadams/rho/commit/72d6434e598712d4ad17cc644f228a9ac770c797))
* **repl:** add interactive editor input support ([94db02d](https://github.com/casonadams/rho/commit/94db02d06c2f9a042fbad6dd267b19d28b1cf1aa))
* **repl:** add slash-command completion and richer session display ([80d8874](https://github.com/casonadams/rho/commit/80d887493c5b3d2e2247f20ae1f097634079d324))
* **repl:** align slash commands with pi.dev ([77e2ff4](https://github.com/casonadams/rho/commit/77e2ff4ce833a1477b57896bec8cd5ebd3c959a1))
* **repl:** coordinate queued prompts during active runs ([07c82d0](https://github.com/casonadams/rho/commit/07c82d0c450990eaa52d0afb6ab95ee7cddd14aa))
* **repl:** dynamic model discovery and rich argument autocomplete ([9ab8fdf](https://github.com/casonadams/rho/commit/9ab8fdf61d20d05951d9d4d94a389db27ba56054))
* **repl:** enable live editor for TTY sessions ([b6958b2](https://github.com/casonadams/rho/commit/b6958b217bc0705f3243bc66c2c395a91438aca9))
* **repl:** instant fuzzy slash-command autocomplete menu ([e5dd69b](https://github.com/casonadams/rho/commit/e5dd69b6c71ce0376b040219a509e69d022cf621))
* **repl:** restore Tab and Enter as completion acceptance and chaining keys ([c2ff8e3](https://github.com/casonadams/rho/commit/c2ff8e346e578494ba6a92614b71c104a07c2f62))
* **repl:** route command output through renderer ([321b1c5](https://github.com/casonadams/rho/commit/321b1c556e153cdcddff2e3729f3b6eaaf7ce975))
* **repl:** support Tab for both completion acceptance and candidate cycling ([a4bc36a](https://github.com/casonadams/rho/commit/a4bc36a9781934affe61222f05fa055776fb5b37))
* **repl:** support Tab/Shift+Tab cycling through autocomplete dropdown ([5fe680b](https://github.com/casonadams/rho/commit/5fe680b6b0992b6b56aa147e333bfc101e31b141))
* **repl:** unify /model slash command with interactive Model Selector modal ([c96250a](https://github.com/casonadams/rho/commit/c96250aefbc5c20f71441e80404dd075849df0b2))
* **runtime:** add steering queue, MCP tool servers, and parallel tool execution ([6d891ed](https://github.com/casonadams/rho/commit/6d891ed4815243564bd4cfa7e93efa0a5ff48d7f))
* **sdk:** publish the presentation capability contract and ui resolution rules ([69cef8f](https://github.com/casonadams/rho/commit/69cef8fce706def512b1642622bc5173a657312e))
* **skills:** resolve declarative skill overrides across origins ([0f2722e](https://github.com/casonadams/rho/commit/0f2722eaa838b3b7844d77d342935157a46263c7))
* **subagents:** add sub-agent and multi-agent workflow capability plugin ([bf6ba6a](https://github.com/casonadams/rho/commit/bf6ba6aca2022206f40f98868b3189894cf73615))
* **tokens:** add offline token estimation, pre-flight context guardrails, and workspace refactor ([952af2c](https://github.com/casonadams/rho/commit/952af2c399946f1a74a4f846c52b99a86d70b449))
* **tools:** add web_search and web_fetch standard aliases ([1e36ca5](https://github.com/casonadams/rho/commit/1e36ca5aafdf9b82e9a3a4b7feecef06dd384856))
* **tools:** route user questions through UI port ([8f60d29](https://github.com/casonadams/rho/commit/8f60d29f7fc8c088ae4fd7b52e008d9e19f6a76a))
* **tools:** standardize on ask_user_question across prompts and schema ([0634f54](https://github.com/casonadams/rho/commit/0634f54e9a15afd14a925b0db032a02a5f361a5b))
* **ui:** add ANSI synchronized output buffering and clean up test expectations ([ccf8af5](https://github.com/casonadams/rho/commit/ccf8af57500c1503696de2169874254832d8a516))
* **ui:** add async updating tool blocks, streaming tail output, and ctrl+o expansion ([f1015f9](https://github.com/casonadams/rho/commit/f1015f97638d98224711bb995c6aa2077301aa1f))
* **ui:** add interactive denial feedback for bash commands and write diff previews ([393b057](https://github.com/casonadams/rho/commit/393b057b49504682ffd856825e4d035082b5fc26))
* **ui:** add interactive event port ([7b3cdf9](https://github.com/casonadams/rho/commit/7b3cdf9f4a9760341aff802624f636a0b8942093))
* **ui:** add shortcut keys, fuzzy model selector, and keybindings engine ([57f3f85](https://github.com/casonadams/rho/commit/57f3f859c1903bb0f55ce3e195931b7c50a2260b))
* **ui:** add tool diff previews, queued message display with Alt+Up dequeue, and input-frame styled modals ([c163cda](https://github.com/casonadams/rho/commit/c163cda86d8a5c8099d79b0d2c98174314b19398))
* **ui:** animate active footer status ([785f73a](https://github.com/casonadams/rho/commit/785f73a22eb8c3e50b9468fabb13a182973b5f7f))
* **ui:** clean denial reason formatting and enhance question option layout ([5a37513](https://github.com/casonadams/rho/commit/5a37513683b9fea68ca505e4e0d7685454bdddcf))
* **ui:** define interactive terminal state ([1dacbb4](https://github.com/casonadams/rho/commit/1dacbb4f54b134b654b9faae1718f2ea48121e56))
* **ui:** format diffs in diff code blocks, remove artificial indent, and support editable command verification ([6e1475c](https://github.com/casonadams/rho/commit/6e1475c7ebaf51a003ca32fb3fba659aa32748d2))
* **ui:** format interactive footer matching pi status-line plugin ([6719b12](https://github.com/casonadams/rho/commit/6719b126c22d92a33561e45e3bbd091166a59cb4))
* **ui:** reflow full transcript on terminal resize and ctrl+o global expand ([2267fb1](https://github.com/casonadams/rho/commit/2267fb15358e87e0988897fae51fc9d6ebe69c83))
* **ui:** render transcript blocks and semantic markdown ([d935fdf](https://github.com/casonadams/rho/commit/d935fdf77a04a5313ef4aab71071753161a851d4))
* **ui:** render working spinner above the input and strip it from the footer ([eedf5d2](https://github.com/casonadams/rho/commit/eedf5d2b009b423fca03ff366465a78d531a46c6))
* **ui:** request approvals through interactive port ([e174dd4](https://github.com/casonadams/rho/commit/e174dd4e84cc4a963145be701c42b632c8ab9088))
* **ui:** route renderer output through event port ([9a3bd9c](https://github.com/casonadams/rho/commit/9a3bd9c4c909008497ec0398e62b059ca0d0a9c0))
* **ui:** stream prose and thinking live word-by-word with clean vertical rhythm ([1d870e6](https://github.com/casonadams/rho/commit/1d870e6753964422bf613dde858279a29048b6ea))


### Bug Fixes

* address review findings for safety, concurrency, and validation ([79aeb18](https://github.com/casonadams/rho/commit/79aeb18e083ffebd82d29909719bb2fad8330e18))
* **antigravity:** capture and preserve thoughtSignature on tool calls with observation text fallback ([92c721d](https://github.com/casonadams/rho/commit/92c721d8d4be2905f613179b4cd3609428618ff3))
* **antigravity:** enforce strict turn role alternation, correct tool response naming, and target active model quota ([078de47](https://github.com/casonadams/rho/commit/078de4744dce6e41a80776cd6ff9b8bfe0a50f2e))
* **antigravity:** ensure root object schema without injecting type into nested properties ([bb745a5](https://github.com/casonadams/rho/commit/bb745a53e2cf6dbeb37c33c463fafec962d4f3af))
* **antigravity:** parse wrapped SSE response and ensure non-empty assistant turns ([70803a1](https://github.com/casonadams/rho/commit/70803a1de1cd01d20d60bdc7fcbce5ecbc7874ca))
* **antigravity:** propagate detailed provider errors and include standard system instructions ([fa3f283](https://github.com/casonadams/rho/commit/fa3f2839c374efcf25e4964a99a39f40d66cd091))
* **antigravity:** strip meta schemas from tool declarations, add toolConfig validated, and format output/error tool responses ([2542e06](https://github.com/casonadams/rho/commit/2542e06fe3ac1ffd269eeb49e241a4e5a064f784))
* **antigravity:** use application/json headers, auto-refresh tokens, and add models catalog fallback ([d38cbd6](https://github.com/casonadams/rho/commit/d38cbd67518da06c6e77fd58c463143ad6f68e36))
* **approval:** persist session approvals across turns and fix modal top border width ([26592e4](https://github.com/casonadams/rho/commit/26592e4a9b47ee5933b17380deb18b132590c683))
* **auth:** add antigravity to interactive login provider selection list ([a4d94c0](https://github.com/casonadams/rho/commit/a4d94c0c90d820dbb4f1af9f6548891e95562521))
* **auth:** allow login/logout for custom provider names; pin reload semantics ([9115b3a](https://github.com/casonadams/rho/commit/9115b3a0fca1a3fc6f016152ecdf0da9b06b013b))
* **config:** add AI_THINKING_LEVEL env var parsing to merge::apply_env_overrides ([eeb1874](https://github.com/casonadams/rho/commit/eeb1874acc8c44ff5cbecd40f8345b13ee49c910))
* **engine:** auto-select configured provider or local model on startup when default provider has no key ([badf3fb](https://github.com/casonadams/rho/commit/badf3fbab1622c172851ab9db6820d584951f16e))
* **engine:** remove silent model fallback in builder and rebuild engine on model cycle ([806dac8](https://github.com/casonadams/rho/commit/806dac8b193b972cb127680d2f0c2e465ab272c5))
* **engine:** strictly honor user selected model and provider without fallback ([6a39b7a](https://github.com/casonadams/rho/commit/6a39b7a19600f41de883bd7f01e86e0bd7bd7ffb))
* **intent:** accept boolean progress encodings ([497bbde](https://github.com/casonadams/rho/commit/497bbdee6fc483ffb54129d7b8c3689b890edb25))
* **intent:** prevent unverified completion claims ([44e857b](https://github.com/casonadams/rho/commit/44e857bab909e393869394da1743a555adb0abe2))
* **intent:** reconcile completion state ([defc354](https://github.com/casonadams/rho/commit/defc35490030d4568ec07146ad8ad7a01c155d7a))
* **intent:** skip tracking informational turns ([3d0cbc1](https://github.com/casonadams/rho/commit/3d0cbc162f5ffd34bbed1d9b9c14316d204c65bf))
* **repl:** align autocomplete keys exactly with pi.tui editor contract ([0fe349d](https://github.com/casonadams/rho/commit/0fe349d226ccd602615326e77aee5e88296b5a07))
* **repl:** always discover and list local Ollama models regardless of active provider ([9e58f68](https://github.com/casonadams/rho/commit/9e58f68f2eb2282de79b4a4d610a5ddc0e2fa735))
* **repl:** chain autocomplete dropdown on command acceptance ([758a801](https://github.com/casonadams/rho/commit/758a80102c1ee6cc22b976d5173a478b7bb3ce09))
* **repl:** prevent active input starvation ([649fb30](https://github.com/casonadams/rho/commit/649fb30f38cad6c7fdbd6e5b96502bec5fce9557))
* **repl:** restore boundary-aware input history ([619d892](https://github.com/casonadams/rho/commit/619d89255287ab5e43675e3752019e6103228d98))
* **subagents:** implement 4-tier model resolution cascade and test default_model ([46ce534](https://github.com/casonadams/rho/commit/46ce534c312bc138186a193ce3388613a3173ec2))
* **subagents:** unpin explore template model to allow default_model inheritance ([b0e1940](https://github.com/casonadams/rho/commit/b0e19409490e0c03f44a497165c0dc3b15a2eb0e))
* **todo:** make tool arguments parse reliably and inline generated schemas ([64fbcfa](https://github.com/casonadams/rho/commit/64fbcfab67eb03e725ec21852f8e05f1573b0d36))
* **tools:** normalize boolean subschemas in tool argument definitions ([037f6ab](https://github.com/casonadams/rho/commit/037f6abd53a91249c0d2a793d7384060b09d5ca5))
* **tui:** properly match Shift+Tab across all terminal BackTab and Shift+Tab key encodings ([6569f9b](https://github.com/casonadams/rho/commit/6569f9b7422c3a9bdaf9f896f977c70eaeebbbc2))
* **tui:** update thinking level in-place on footer and match Pi border colors ([c3e8187](https://github.com/casonadams/rho/commit/c3e8187c474aaf74aba67aad3661defc90734bbb))
* **ui:** add blank spacer line above spinner and sync cursor offset ([591ab8e](https://github.com/casonadams/rho/commit/591ab8e654ebebddd373dc84078f2e1ddb1fe056))
* **ui:** buffer terminal writes to eliminate footer and cursor flicker ([fef2018](https://github.com/casonadams/rho/commit/fef20184ae8cdba193389be1bbba84b7c0eb79e9))
* **ui:** calculate context remaining and LLM generation tokens/sec accurately ([3071c39](https://github.com/casonadams/rho/commit/3071c39be52b8a76198356799a0b9f9b3451460d))
* **ui:** correct context usage percentage calculation and formatting ([5b90705](https://github.com/casonadams/rho/commit/5b90705a750984c6fb73a0144de0c3930ce06ca3))
* **ui:** eliminate streaming flicker and anchor editor cursor position ([ca9da40](https://github.com/casonadams/rho/commit/ca9da40312f3b755ca31d83ba21b613d83a1c9ad))
* **ui:** enable direct text entry and auto-input mode for open-ended questions ([ca96475](https://github.com/casonadams/rho/commit/ca96475c5c7e838de1c6233418b326ff54368409))
* **ui:** format fetch on single line and simplify search tool display ([f45ac4f](https://github.com/casonadams/rho/commit/f45ac4f98f415b2394fe4d65ba49355d3c4e3d99))
* **ui:** hide hardware cursor during active model streaming ([014cff8](https://github.com/casonadams/rho/commit/014cff8772e83fc2baba1710a788f11b77f2b986))
* **ui:** keep hardware cursor visible at prompt input across streaming and idle states ([001d641](https://github.com/casonadams/rho/commit/001d64102968b313c1f51d21d5f7aebaa4d31b69))
* **ui:** keep streaming cursor relative ([7a14111](https://github.com/casonadams/rho/commit/7a141111821160f2430f5b8c8766d9eedb3611a9))
* **ui:** navigate active multiline drafts ([9e2078f](https://github.com/casonadams/rho/commit/9e2078f8fbaad0ef6bfb0a12b733e234068860aa))
* **ui:** preserve background color across inner ANSI resets in styled blocks ([8e96f5e](https://github.com/casonadams/rho/commit/8e96f5e3c69a84a9cc5160625ac69331d696a2de))
* **ui:** preserve full bash output and make tool finish rendering atomic ([082eead](https://github.com/casonadams/rho/commit/082eead0070dd8efcc3602fc79ec3d760f76cdb5))
* **ui:** preserve live editor cursor state ([80b8a5c](https://github.com/casonadams/rho/commit/80b8a5ca2b6c6b698c6634756f07e91810bfa768))
* **ui:** preserve non-table pipe text ([fe0f8fd](https://github.com/casonadams/rho/commit/fe0f8fdefe5e9cb3a912a960c12b6868f201a31d))
* **ui:** preserve streamed line suffixes ([5a5f8af](https://github.com/casonadams/rho/commit/5a5f8af57c99fea3cbe5bc017faea33de4069b2b))
* **ui:** preserve streamed output lines ([7566eeb](https://github.com/casonadams/rho/commit/7566eeb45982030fbebb3718d0bb2ae6dba87315))
* **ui:** preserve table row buffering during streaming to render formatted unicode tables ([99f4cc7](https://github.com/casonadams/rho/commit/99f4cc7d20e1c43035731df991ec7f73d4e50a91))
* **ui:** prevent duplicate assistant response output on transcript push ([ae0a368](https://github.com/casonadams/rho/commit/ae0a3689fcd04af04b8e5673178fb6f88c7d4ca1))
* **ui:** remove block horizontal padding and restore live mermaid rendering ([55963c0](https://github.com/casonadams/rho/commit/55963c043225b0b10f145c920d93c26774b16b0b))
* **ui:** render permission prompts and questions as modal overlay box ([31be32d](https://github.com/casonadams/rho/commit/31be32ddde972865a6f31e069ec4467be91c7755))
* **ui:** render working spinner above the input frame like pi ([7bd32ed](https://github.com/casonadams/rho/commit/7bd32ed94eaecccbbfd6c1d22b4767d0259ee82b))
* **ui:** separate active tool blocks from preceding transcript output ([072650b](https://github.com/casonadams/rho/commit/072650b5cb6a7d0ab236c4ea979ac092675868ef))
* **ui:** update footer state and redraw on turn completion ([54a7a58](https://github.com/casonadams/rho/commit/54a7a5896a73fc7c2346ce0a3b80dcbf614911e4))


### Performance Improvements

* remove unbounded hot paths found in performance audit ([517e33e](https://github.com/casonadams/rho/commit/517e33e4c3b4f04a89022b53863f003029d1646b))
* **ui:** coalesce streamed terminal output ([71b994e](https://github.com/casonadams/rho/commit/71b994ed62c3848295ab6c4e5ae7119191cc3c8f))
