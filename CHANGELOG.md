# Changelog

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
