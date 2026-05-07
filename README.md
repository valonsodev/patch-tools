# patch-tools

> [!CAUTION]
> This tool runs arbitrary scripts. Do not use this if you don't know what you are doing. Do not run scripts you do not trust.

## What This Is

`patch-tools` is a CLI for loading APKs (bundles too) into Morphe, searching and inspecting app code, generating fingerprints, and running Kotlin scripts against those loaded packages. 

It's a Rust CLI tool with a bundled Kotlin scripting engine wrapping [`morphe-patcher`](https://github.com/MorpheApp/morphe-patcher) (think `jadx-revanced` but applying patches too.)



![Demo](.github/demo.gif)

## How To Use

Start in an empty working directory and scaffold the script files:

```sh
patch-tools scaffold
```

Then read the generated [`AGENTS.md`](patch-tools/templates/AGENTS.md), even if you are a human.
It is the practical guide for how to write and iterate on `main.kts`.

The common loop is:

```sh
patch-tools daemon start
patch-tools load path/to/app.apk
patch-tools run main.kts
```

## Available Commands

Global option:

- `--format <markdown|human>`: set the output format (defaults to `markdown`)

Commands:

- `daemon start [--apk <path>...]`: start the daemon and optionally preload APKs
- `daemon stop`: stop the daemon
- `daemon status`: show daemon status
- `load <apk_path>`: load an APK, APKM, or XAPK
- `unload [apk]`: unload a package by package name, package/version, or internal ID; omit when exactly one APK is loaded
- `run <script_path> [--install] [--device <serial>]`: run a `.kts` script and optionally install patched APKs with `adb`
- `scaffold`: create a `main.kts` and `AGENTS.md` in the current directory
- `fingerprint [apk] <method_id> [-n, --limit <count>]`: generate method fingerprints; omit `apk` when exactly one APK is loaded
- `class-fingerprint [apk] <class_id> [-n, --limit <count>]`: generate class fingerprints; omit `apk` when exactly one APK is loaded
- `search <query...> [-n, --limit <count>]`: fuzzy search methods across loaded APKs by name
- `smali [apk] <method_id>`: print a method's smali code; omit `apk` when exactly one APK is loaded
- `completion <shell>`: generate shell completions

---

> [!WARNING]
> - A lot (like a lot) of the code is AI-generated, this was originally a complex KMP app that blew out of proportion so I scaled it down to match what I actually wanted with the help of AI.
> - This is `AI first` in the sense that the main user I targeted for this tool is AI agents, it works good for humans though.
> - This is currently Unix-only. It relies on Unix sockets for daemon communication, so Windows is not supported, use WSL.
> - The CLI weight is mostly the embedded kotlin compiler, nothing can be done about that.
> - Combined with the setup in https://github.com/valonsodev/patch-explore I've had really good results having agents patch apps completely autonomously in 2 steps.
> - Maintaining this is definitely not a priority for me, I made this to scratch my own itch and share it with whoever might find it useful. Feel free to PR, fork, or do whatever you want with the code.
> - Do not expect active maintenance or semantic versioning. This is a dev tool, and releases are tied to Morphe library releases.
> - Some Smali and XML diffs will probably do some wonky things.

## License

This project is licensed under `GPL-3.0-only`. See [LICENSE](LICENSE) for the full terms and [NOTICE](NOTICE) for third-party notices.

## Credits
- [MorpheApp](https://github.com/MorpheApp) for the patcher and other things used in the engine
- [hoo-dles](https://github.com/hoo-dles) for most of the advanced examples used in the `AGENTS.md` of the scaffold command and making cool patches
- [syndiff](https://github.com/marcocondrache/syndiff) for the diffing code that I heavily modified and adapted for smali and XML diffing
