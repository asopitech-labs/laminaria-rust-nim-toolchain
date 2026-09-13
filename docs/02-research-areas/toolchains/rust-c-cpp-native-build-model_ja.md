# Rust/CargoにおけるC/C++ native依存のbuild model

## 結論

通常のCargo projectでは、`rustc`はC/C++ sourceをcompileしない。Cargoがpackage graphを解決し、package本体より前にhost用の`build.rs`をcompile・実行する。`build.rs`またはそこから呼ばれるtoolがC/C++ compiler、archiver、CMake等を起動してnative artifactを作り、link search path、library、linker argumentをCargoへ返す。Cargoはそれらを`rustc`へ渡し、`rustc`がtarget linkerを起動してRust objectとnative artifactを最終成果物へlinkする。[^cargo-build-scripts] [^rustc-linker]

したがって「`rustc`は最後にlinkするだけか」という問いには、C/C++に限れば概ねyesである。ただし実際のlink主体は`rustc`が起動するsystem linkerまたはcompiler driverであり、Cargo全体ではその前に任意programである`build.rs`内のnative build/discoveryが存在する。

## 標準的な実行順序

```text
Cargo dependency resolution
  -> build.rsとそのbuild-dependenciesをhost向けにcompile
  -> build.rsを実行
     -> bundled C/C++ sourceを外部compilerでcompile
     -> object/archiveを生成、またはsystem/prebuilt libraryを探索
     -> cargo::rustc-link-search / rustc-link-lib / rustc-link-argを出力
  -> Cargoがlink指示をrustcの-L / -l / -C link-argへ変換
  -> rustcがRust crateをcompile
  -> rustcがtarget linkerを起動
  -> native executable / library
```

Cargoは`build.rs`をpackage build直前に実行する。build scriptはbundled C libraryのbuild、system library探索、code generation、platform固有設定等を実行できる。出力された`cargo::` instructionの順序は`rustc`、さらにlinkerへ渡るargument順に影響する。[^cargo-build-scripts]

## C/C++ sourceのcompile主体

典型的な`cc` crateは、C/C++/assembly sourceからstatic archiveを作るためのCargo build dependencyである。`cc`自身がcompilerを内蔵するのではなく、platformの`cc`、Clang/GCC、MSVC等を検出して外部processとして起動する。C++は`Build::cpp(true)`で選び、`CXX`/`CXXFLAGS`やC++ standard libraryの選択が関与する。[^cc]

他の一般的な形は次のとおりである。

- `pkg-config`、vcpkg、platform framework等からprebuilt/system libraryを発見する。
- CMake、Meson、Autotools等を`build.rs`から起動してobject/archive/shared libraryを作る。
- `*-sys` crateがnative libraryの探索、source build、FFI declarationを集約する。
- bindgen等でRust declarationやadapter sourceを生成してからcompileする。

これらはCargoから見ると、多くの場合一つのbuild-script executionである。内部のheader dependency、translation unit、generator、compiler flag、archive memberはCargo package resolverのtyped nodeではない。

## Cargoからrustcへ渡るlink contract

build scriptは主に次を出力する。[^cargo-build-scripts]

```text
cargo::rustc-link-search=[KIND=]PATH
cargo::rustc-link-lib=[KIND[:MODIFIERS]=]NAME[:RENAME]
cargo::rustc-link-arg=FLAG
```

Cargoは`rustc-link-search`を`rustc -L`へ、`rustc-link-lib`を`rustc -l`へ、`rustc-link-arg`を`rustc -C link-arg`へ変換する。`rustc`の`-l`はstatic library、dynamic library、macOS framework等を指定できる。[^rustc-cli]

`rustc`はtargetごとに選ばれたlinkerを起動する。Unixで`cc`や`clang`がlinker driverとして選ばれる場合があるが、この段階で通常渡されるのはRust/C/C++の既存object/archiveであり、C/C++ sourceのcompileではない。[^rustc-linker]

`rlib`や`staticlib`ではnative static libraryをarchive内へbundleする場合があり、最終binary linkまでnative objectの取り出しが遅延する。dynamic libraryの場合は最終linkでdependencyが記録され、実体のloadはruntime loaderが担う。[^rust-reference-link]

## FFIとC++固有の境界

Rustの`unsafe extern "C"` blockは外部function/staticのsymbolとcalling conventionを宣言するが、その実装sourceをcompileしない。`#[link]`または`-l`はnative libraryをlink対象へ加える。[^rust-reference-link]

C++ではさらに次が必要になる。

- name manglingを避ける`extern "C"` wrapper、またはC++ ABIを理解するbridge
- template/inline/header-only codeの明示的instantiationまたはadapter translation unit
- compilerとstandard library ABIの整合
- exception、RTTI、constructor/destructor、ownership/lifetime contract
- `libstdc++`/`libc++`等と正しいlink order

`cc` crateはC++ compileとstandard-library linkを補助できるが、RustがC++ semanticsを解析・解決するわけではない。[^cc]

## Cargoが解くgraphと解かないgraph

Cargo resolverは主にRust packageのversion requirement、feature、normal/build/dev dependency、target-specific dependencyを解き、結果を`Cargo.lock`へ固定する。feature resolverはtarget dependencyやbuild dependencyのfeature unificationにも固有規則を持つ。[^cargo-resolver]

Cargoの`package.links`は、同じnative libraryをlinkするpackageの重複を制約し、build script metadataを直接dependentへ`DEP_*`として渡す。しかしmetadata伝播は原則としてimmediate dependentまでであり、C/C++全体の推移的artifact graphを表現するものではない。[^cargo-links]

```text
Cargoが明示的に解く層
  package / version / feature / target dependency / build dependency

build.rs内部へ隠れやすい層
  C/C++ source / header / generated unit / native compiler / flags
  object / archive / shared library / symbol / ABI / link order
  system-library discovery / platform runtime
```

よって次の三つは同一ではない。

```text
Cargo package graph resolution
  != C/C++ native artifact graph resolution
  != final native executable link closure
```

## LAMINARIAへの含意

LAMINARIAがecosystem横断closureを解くには、`build.rs`を単一opaque actionとして実行しただけでは不十分である。少なくとも`NativeSource`、`Header`、`GeneratedUnit`、`NativeCompile`、`Object`、`Archive`、`SharedLibrary`、`AbiConstraint`、`RequiredSymbol`、`ProvidedSymbol`、`LinkOrder`、`RuntimeLibrary`、`FinalLink`を識別し、host/target、compiler identity、flags、producer provenanceを保持する必要がある。

直接的な実行可能テストは、最終native executableを起動するだけでなく、その結果へ必要な全native artifactとproducerが解決済みgraphに存在し、未申告のopaque build fallbackがなかったことを検証する。

## Sources

[^cargo-build-scripts]: Rust Project, “[Build Scripts — The Cargo Book](https://doc.rust-lang.org/cargo/reference/build-scripts.html).” Accessed 2026-09-13.
[^cc]: rust-lang/cc-rs, “[cc crate documentation](https://docs.rs/cc/latest/cc/).” Accessed 2026-09-13.
[^rustc-linker]: Rust Project, “[Codegen Options — linker and linker-flavor](https://doc.rust-lang.org/rustc/codegen-options/index.html#linker).” Accessed 2026-09-13.
[^rustc-cli]: Rust Project, “[rustc Command-line Arguments — `-l`](https://doc.rust-lang.org/rustc/command-line-arguments.html#-l-link-the-generated-crate-to-a-native-library).” Accessed 2026-09-13.
[^rust-reference-link]: Rust Project, “[External blocks and the `link` attribute — The Rust Reference](https://doc.rust-lang.org/reference/items/external-blocks.html#the-link-attribute).” Accessed 2026-09-13.
[^cargo-resolver]: Rust Project, “[Dependency Resolution — The Cargo Book](https://doc.rust-lang.org/cargo/reference/resolver.html).” Accessed 2026-09-13.
[^cargo-links]: Rust Project, “[The `links` Manifest Key — The Cargo Book](https://doc.rust-lang.org/cargo/reference/build-scripts.html#the-links-manifest-key).” Accessed 2026-09-13.
