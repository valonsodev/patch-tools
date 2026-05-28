# patch-tools

> [!CAUTION]
> This tool runs arbitrary scripts. Do not use this if you don't know what you are doing. Do not run scripts you do not trust.

## What This Is

`patch-tools` is a CLI for working with APKs (bundles too) through Morphe. You load packages into a
daemon, then search and inspect their code, generate fingerprints, and run Kotlin scripts that patch
them.

It's a Rust CLI tool with a bundled Kotlin scripting engine wrapping [`morphe-patcher`](https://github.com/MorpheApp/morphe-patcher) (think `jadx-revanced` in a CLI but applying patches too).

![Demo](.github/demo.gif)

## Requirements

- A **JVM 17+** (JDK or JRE). The engine runs on an embedded JVM through JNI, so one must be present
  at runtime — set `JAVA_HOME` if it isn't auto-discovered. Tune the JVM with `MORPHE_JAVA_OPTS`
  (default `-Xmx4g`).
- **`adb`** on your `PATH`, only if you use `run --install` / `--device`.

## Install

Grab a prebuilt binary from the [Releases](https://github.com/valonsodev/patch-tools/releases)
page:

- `patch-tools-linux-x86_64`
- `patch-tools-macos-aarch64`
- `patch-tools-windows-x86_64.exe`

Or build from source ([`just`](https://github.com/casey/just) + a Rust toolchain + JDK 17):

```sh
just install   # builds the engine JAR, then `cargo install --path patch-tools`
```

## How To Use

The core idea: you write a Kotlin script (`main.kts`), and `patch-tools run` evaluates it against
the APKs you've loaded into the daemon. The script defines fingerprints and patches; the engine
runs them through Morphe and reports the results (or writes out a patched APK).

Start in an empty working directory and scaffold the script files:

```sh
patch-tools scaffold
```

Then read the generated [`AGENTS.md`](patch-tools/templates/AGENTS.md), even if you are a human.
It is the practical guide for how to write and iterate on `main.kts`.

The common loop is:

```sh
patch-tools daemon start       # start the long-lived background process
patch-tools load path/to/app.apk   # load an APK into it (stays loaded across runs)
patch-tools run main.kts       # evaluate the script; iterate on main.kts and re-run
```

The daemon keeps running (one per working directory) until you stop it with `patch-tools daemon
stop` — it won't exit on its own. Each one holds an APK and a JVM in memory (a few GB; see
`MORPHE_JAVA_OPTS`), so it's on you to close them when done. If a daemon is wedged, use your OS
process tools to kill it. On Unix-like systems:

```sh
pkill -f 'patch-tools internal-daemon'
```

On Windows PowerShell:

```powershell
Stop-Process -Name patch-tools -Force
```

You iterate by editing `main.kts` and re-running `patch-tools run main.kts`, inspecting the diff
each time, until the patch does what you want. Once you're happy, install it onto a connected
device:

```sh
patch-tools run main.kts --install
```

> [!NOTE]
> `--install` requires exactly one connected `adb` device unless you pass `--device <serial>`.
> It queries the Android current user and installs the patched APK for that user only, not for all
> users on the device.

## How The Engine Works

The engine processes whatever your script's **final expression** evaluates to. End the file with
`MyFingerprint` to process one item, or `listOf(MyFingerprint, myPatch)` to process several;
`null`/`Unit` means nothing to do. Patches (`BytecodePatch`, `ResourcePatch`, `RawResourcePatch`)
are applied, `Fingerprint`s are matched, and anything else is reported via `toString()`.

A minimal `main.kts` that finds a method and forces it to return `true`:

```kotlin
import app.morphe.patcher.Fingerprint
import app.morphe.patcher.extensions.InstructionExtensions.addInstructions
import app.morphe.patcher.literal
import app.morphe.patcher.patch.bytecodePatch

object PremiumCheckFingerprint : Fingerprint(
    filters = listOf(literal(0)),
)

val forcePremiumPatch = bytecodePatch("Force premium") {
    execute {
        PremiumCheckFingerprint.methodOrNull?.addInstructions(
            0,
            """
                const/4 v0, 0x1
                return v0
            """.trimIndent(),
        )
    }
}

// Final expression: the fingerprint is matched, the patch is applied.
listOf(PremiumCheckFingerprint, forcePremiumPatch)
```

`print` and `println` are shadowed inside scripts: instead of going to the console, their output is
captured by the engine and shown in the run results, associated with the item that produced it.

Rather than hand-writing fingerprints, use `patch-tools fingerprint <method_id>`: it analyzes a
method you've located in a loaded APK and emits a ready-to-paste `Fingerprint` declaration that
matches it. See the scaffolded `AGENTS.md` for the full workflow.

## Available Commands

`patch-tools --help` and `patch-tools <command> --help` are authoritative.

Global option:

- `--format <markdown|human>`: set the output format (defaults to `markdown`)

Commands:

- `daemon start [--apk <path>...]`: start the daemon and optionally preload APKs
- `daemon stop`: stop the daemon
- `daemon status`: show daemon status
- `load <apk_path>`: load an APK, APKM, or XAPK
- `unload [apk]`: unload a package by package name, package/version, or internal ID; omit when exactly one APK is loaded
- `run <script_path> [--save[=PATH]] [--install] [--device <serial>]`: run a `.kts` script; bare `--save` writes patched APKs into the current directory using the engine's filename, `--save=foo.apk` renames the output (single APK only), `--save=dir/` writes into that directory, `--install` installs them with `adb`
- `scaffold`: create a `main.kts` and `AGENTS.md` in the current directory
- `fingerprint [apk] <method_id> [-n, --limit <count>]`: generate method fingerprints; omit `apk` when exactly one APK is loaded
- `class-fingerprint [apk] <class_id> [-n, --limit <count>]`: generate class fingerprints; omit `apk` when exactly one APK is loaded
- `common-fingerprint <apk> <method_id> <apk> <method_id>... [-n, --limit <count>]`: generate fingerprints shared by equivalent methods across APKs
- `search <query...> [-n, --limit <count>]`: fuzzy search methods across loaded APKs by name
- `map <old_apk> <method_id> <new_apk> [-n, --limit <count>]`: rank methods in another loaded APK by similarity to a source method
- `smali [apk] <method_id>`: print a method's smali code; omit `apk` when exactly one APK is loaded
- `completion <shell>`: generate shell completions

---

> [!WARNING]
> - A lot (like a lot) of the code is AI-generated, this was originally a complex KMP app that blew out of proportion so I scaled it down to match what I actually wanted with the help of AI.
> - This is `AI first` in the sense that the main user I targeted for this tool is AI agents, it works good for humans though.
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
