# Morphe Script Agent Guide

These instructions apply to the scaffolded `main.kts` in this directory.

## Core model

- `main.kts` is the script Morphe evaluates.
- Utility helpers are available directly from the engine-bundled `app.morphe:morphe-patches-library`. These are the methods you get from `import app.morphe.util.*`.
- Morphe re-evaluates `main.kts` while processing items, so keep it deterministic.
- Morphe tracks returned items by list index.

## Command reference

### `patch-tools daemon start`

Starts the background daemon process.

- `--apk <path>`: optional, repeatable preload Android package path. Supported inputs are `.apk`,
  `.apkm`, and `.xapk`. Each provided package is loaded after the daemon starts.

### `patch-tools daemon stop`

Stops the running daemon.

### `patch-tools daemon status`

Shows daemon status and currently loaded packages.

### `patch-tools load <apk_path>`

Loads an Android package into the running daemon.

- `apk_path`: filesystem path to the `.apk`, `.apkm`, or `.xapk` file to analyze or patch.

### `patch-tools unload [apk]`

Unloads one loaded package from the daemon.

- `apk`: optional APK selector. This can be the package name, package/version label, or internal APK ID.
  Omit it only when exactly one APK is loaded.

### `patch-tools run <script_path>`

Executes a Kotlin script against the currently loaded packages.

- `script_path`: path to the `.kts` script, usually `main.kts`.
- `--install`: after a successful run, save and install patched APKs through `adb`.
- `--device <serial>`: optional target device serial for `adb` install flows.

### `patch-tools scaffold`

Creates scaffold files in the current directory:

- `main.kts`
- `AGENTS.md`

### `patch-tools fingerprint [apk] <method_id>`

Generates candidate fingerprints for one method.

- `apk`: optional APK selector. Omit it only when exactly one APK is loaded.
- `method_id`: method selector. Usually the smali `unique_id`, but Java-signature form may also
  work in many flows.
- `-n <limit>` or `--limit <limit>`: maximum number of generated fingerprints to return.

### `patch-tools class-fingerprint [apk] <class_id>`

Generates class-scoped fingerprint candidates.

- `apk`: optional APK selector. Omit it only when exactly one APK is loaded.
- `class_id`: class selector. Usually a smali class type like `Lfoo/Bar;` or a fully qualified
  Java class name.
- `-n <limit>` or `--limit <limit>`: maximum number of generated results to return.

### `patch-tools common-fingerprint <apk> <method_id> <apk> <method_id>...`

Generates candidate fingerprints shared by equivalent methods across loaded packages.

- Pass at least two APK/method pairs.
- Use `map` first when you need help finding the equivalent method in a newer APK.
- `-n <limit>` or `--limit <limit>`: maximum number of generated fingerprints to return.

### `patch-tools search <query...>`

Searches methods across loaded packages using fuzzy matching.

- `query...`: one or more search terms. Multiple arguments are joined with spaces.
- `-n <limit>` or `--limit <limit>`: maximum number of results to return per loaded package.

### `patch-tools map <old_apk> <method_id> <new_apk>`

Ranks methods in another loaded package by similarity to a source method.

- `old_apk`: source APK selector.
- `method_id`: source method selector.
- `new_apk`: target APK selector.
- `-n <limit>` or `--limit <limit>`: maximum number of similar methods to return.

### `patch-tools smali [apk] <method_id>`

Prints the smali body for one method.

- `apk`: optional APK selector. Omit it only when exactly one APK is loaded.
- `method_id`: method selector.

## Rules

1. Import helper APIs directly in `main.kts`, for example `import app.morphe.util.*`.
2. Define fingerprints and patches as named top-level declarations.
3. End `main.kts` with one item or `listOf(...)`.
4. Do not depend on randomness, timestamps, network calls, or mutable global state.
5. Use fingerprints as the only way to target bytecode methods.
6. Give each targeted method its own fingerprint.
7. Keep patches focused. If responsibilities differ, split them.
8. If you return 2 or more patches, Morphe runs them as one combined patch item.
9. If you are working toward one specific goal, return only the relevant fingerprint or patch while iterating so terminal output stays focused.
10. Do not try to invent or hand-write fingerprints from scratch when you have a target method. Use `patch-tools fingerprint [apk] <method_id>` to generate candidates first, then adapt the generated result if needed. You can omit `[apk]` when exactly one APK is loaded.

## Workflow

1. `patch-tools daemon start`
2. `patch-tools load path/to/app.apk`
   Supported inputs are also `path/to/app.apkm` and `path/to/app.xapk`.
3. `patch-tools search onCreate`
4. `patch-tools smali <method-id>` when one APK is loaded, or `patch-tools smali <apk> <method-id>` when multiple APKs are loaded.
5. `patch-tools fingerprint <method-id>` when one APK is loaded, or `patch-tools fingerprint <apk> <method-id>` when multiple APKs are loaded.
6. Convert good candidates into named `Fingerprint` declarations. Do not skip this command and guess the fingerprint yourself if you already have a target method.
7. While fixing one patch or fingerprint, temporarily return only that item from `main.kts`.
8. Start with `methodOrNull` while exploring.
9. Once stable, restore the final return list and run `patch-tools run main.kts`.

## Important clarifications

- The Kotlin declaration name is the readable script label.
- `Fingerprint(name = "...")` is match criteria for the real target method name.
- Do not use `name = "Readable label"` as display-only metadata.
- A scaffolded directory contains `main.kts` and `AGENTS.md`. Morphe supports `.apk`, `.apkm`, and `.xapk` inputs. The user should have the relevant package available locally; otherwise you cant verify your code with `patch-tools`.
- If `patch-tools run main.kts` does not print the expected diff for the patch you are working on, the patch is not working yet. Do not stop at a clean run with no relevant diff. Keep iterating until the expected diff appears.

## Writing guidance

- Always use `object Name : Fingerprint(...)` for fingerprints.
- Prefer `val somePatch = bytecodePatch("Readable patch name") { ... }`.
- Keep patch logic inside `execute { ... }`.
- Use `println(...)` for temporary execution-time logs while iterating. Morphe captures script-authored output and shows it under script output.
- Use `methodOrNull` until the match is proven stable.
- Add comments when they explain non-obvious patch intent.
- Never write patch logic that depends on minified or obfuscated member names such as single-letter
  method or field names. Avoid injecting smali that hardcodes calls like `->a(...)`, `->b(...)`,
  or similarly unstable references unless the target name is demonstrably stable and not obfuscated.
- Do not hardcode registers as a general strategy. Derive them from surrounding instructions,
  method signatures, or stable structure whenever possible. Simple self-contained returns like
  `const/4 v0, 0x1` followed by `return v0`, or `const-string v0, "..."` followed by
  `return-object v0`, are fine when the injected code fully owns that register usage.

## Patch options

Patch options are for user-facing configuration, not for internal script plumbing.

Use patch options when:

- the patch has multiple valid user choices, such as subscription tier or mode
- the patch changes risky behavior and you want an explicit opt-in toggle
- a resource patch needs a user-supplied value like package name, host, label, or ID
- one reusable patch should support a small number of stable variants without forking the patch

Avoid patch options when:

- the value is only useful while you are debugging
- the patch should have one clear deterministic behavior
- the choice can be inferred directly from the matched code
- the option would expose implementation noise rather than a meaningful user decision

Common patterns:

- `stringOption`: choose one of a few labeled string values, or accept validated text
- `booleanOption`: enable or disable a risky or optional behavior
- define options near the top of the patch so `execute` or `finalize` can read them
- use `required = true` for choices that must always resolve explicitly
- if the option accepts free text, validate it
- use readable labels in `values = mapOf("Shown to user" to "actual_value")`

Good uses from real patches:

- choosing a premium variant like `SUPER` vs `MAX`
- toggling whether provider names or permissions should be rewritten
- asking for a replacement package name while validating the format

Bad uses:

- adding a toggle for temporary logging
- exposing a register index or raw opcode choice to users
- adding options just to avoid creating a second focused patch

For hardcoded payload values such as fake signatures, JSON blobs, token strings, IDs, or long
replacement text:

- if the value is fixed patch logic, prefer a named constant over repeating the literal inline
- if the value is long or semantically important, a constant is usually clearer than embedding it
  directly inside `addInstruction(...)` or `addInstructions(...)`
- if the value is something a user may reasonably want to choose or customize, consider a patch
  option instead of a constant
- do not turn purely internal implementation details into options

## Sandbox note

If you are working inside a sandbox, ask for permission before running `patch-tools` commands.

## API Lookup

Do not guess the API surface of `morphe-patcher` or the utility methods from `import app.morphe.util.*`.

Before you use either library, pull the exact versions Morphe is pinned to and inspect the real source:

1. Pull `morphe-patcher` at `{{MORPHE_PATCHER_VERSION}}`.
   `git clone --depth 1 --branch {{MORPHE_PATCHER_VERSION}} https://github.com/MorpheApp/morphe-patcher.git`
2. Pull `morphe-patches-library` at `{{MORPHE_PATCHES_LIBRARY_VERSION}}`.
   This repository contains the methods exposed by `import app.morphe.util.*`.
   `git clone --depth 1 --branch {{MORPHE_PATCHES_LIBRARY_VERSION}} https://github.com/MorpheApp/morphe-patches-library.git`
3. Read the exact source and signatures you plan to use before writing or changing a patch.
4. If the API shape is unclear, stop and inspect the source instead of inferring names, overloads, or behavior from memory.

## Examples

### Fingerprint-only script

```kotlin
import app.morphe.patcher.Fingerprint
import app.morphe.patcher.StringComparisonType
import app.morphe.patcher.string

object ExampleFingerprint : Fingerprint(
    filters = listOf(
        string("SSL", StringComparisonType.CONTAINS),
    ),
)

ExampleFingerprint
```

### Bytecode patch driven by a fingerprint

```kotlin
import app.morphe.patcher.Fingerprint
import app.morphe.patcher.extensions.InstructionExtensions.addInstructions
import app.morphe.patcher.literal
import app.morphe.patcher.patch.bytecodePatch

object PremiumCheckFingerprint : Fingerprint(
    filters = listOf(
        literal(0),
    ),
)

val forcePremiumPatch = bytecodePatch("Force premium") {
    execute {
        PremiumCheckFingerprint.methodOrNull?.let { method ->
            method.addInstructions(
                0,
                """
                    const/4 v0, 0x1
                    return v0
                """.trimIndent(),
            )
        }
    }
}

listOf(PremiumCheckFingerprint, forcePremiumPatch)
```

### Resource patch

```kotlin
import app.morphe.patcher.patch.resourcePatch
import org.w3c.dom.Element

val exampleResourcePatch = resourcePatch(
    name = "Set debuggable flag",
    description = "Adds android:debuggable=true to the application tag.",
) {
    execute {
        document("AndroidManifest.xml").use { document ->
            val applicationNode = document
                .getElementsByTagName("application")
                .item(0) as Element

            if (!applicationNode.hasAttribute("android:debuggable")) {
                document.createAttribute("android:debuggable")
                    .apply { value = "true" }
                    .let(applicationNode.attributes::setNamedItem)
            }
        }
    }
}

exampleResourcePatch
```

### Compatibility and option pattern

```kotlin
import app.morphe.patcher.patch.bytecodePatch
import app.morphe.patcher.patch.stringOption

val configurablePatch = bytecodePatch(
    name = "Configurable patch",
    description = "Example patch with a user option.",
) {
    compatibleWith("com.example.app"("1.2.3"))

    val mode by stringOption(
        key = "mode",
        default = "safe",
        values = mapOf("Safe" to "safe", "Aggressive" to "aggressive"),
        title = "Mode",
        description = "Patch behavior.",
        required = true,
    )

    execute {
        println("Selected mode: $mode")
    }
}

configurablePatch
```

### Resource patch with validated text and boolean options

```kotlin
import app.morphe.patcher.patch.Option
import app.morphe.patcher.patch.booleanOption
import app.morphe.patcher.patch.resourcePatch
import app.morphe.patcher.patch.stringOption
import org.w3c.dom.Element

lateinit var packageNameOption: Option<String>

val changePackageNamePatch = resourcePatch(
    name = "Change package name",
    description = "Renames the app package with optional manifest rewrites.",
    default = false,
) {
    packageNameOption = stringOption(
        key = "packageName",
        default = "Default",
        values = mapOf("Default" to "Default"),
        title = "Package name",
        description = "Replacement package name.",
        required = true,
    ) {
        it == "Default" || it!!.matches(Regex("^[a-z]\\w*(\\.[a-z]\\w*)+$"))
    }

    val updatePermissions by booleanOption(
        key = "updatePermissions",
        default = false,
        title = "Update permissions",
        description = "Rewrite package-scoped permission names in the manifest.",
    )

    finalize {
        document("AndroidManifest.xml").use { document ->
            val manifest = document.getElementsByTagName("manifest").item(0) as Element
            val oldPackage = manifest.getAttribute("package")
            val newPackage =
                if (packageNameOption.value == "Default") "$oldPackage.morphe"
                else packageNameOption.value!!

            manifest.setAttribute("package", newPackage)

            if (updatePermissions == true) {
                // Apply optional follow-up rewrites here.
            }
        }
    }
}
```

### Bytecode patch with a user-visible variant option

```kotlin
import app.morphe.patcher.patch.bytecodePatch
import app.morphe.patcher.patch.stringOption

enum class PremiumVariant {
    SUPER,
    MAX,
}

val enablePremiumPatch = bytecodePatch(
    name = "Enable Premium",
    description = "Enables premium features with a selectable tier.",
) {
    val premiumVariant by stringOption(
        key = "premiumVariant",
        default = PremiumVariant.SUPER.name,
        values = mapOf(
            "Super" to PremiumVariant.SUPER.name,
            "Max" to PremiumVariant.MAX.name,
        ),
        title = "Tier",
        description = "Choose which subscription tier to emulate.",
        required = true,
    )

    execute {
        val selected = enumValueOf<PremiumVariant>(premiumVariant!!)
        // Patch behavior can branch on the selected tier.
    }
}
```

### Nested class fingerprint

```kotlin
import app.morphe.patcher.Fingerprint
import com.android.tools.smali.dexlib2.AccessFlags

object IsPremiumDateValidFingerprint : Fingerprint(
    classFingerprint = Fingerprint(
        accessFlags = listOf(AccessFlags.PUBLIC, AccessFlags.CONSTRUCTOR),
        strings = listOf("userDataSharedPrefs"),
        parameters = listOf("L"),
    ),
    returnType = "Z",
    parameters = listOf("L"),
)
```

Use `classFingerprint` when the method itself is too ambiguous but the surrounding class is easy to
identify.

### Custom fingerprint for exact method and class rules

```kotlin
import app.morphe.patcher.Fingerprint
import com.android.tools.smali.dexlib2.AccessFlags

private const val EXTENSION_CLASS_DESCRIPTOR = "Lapp/morphe/extension/shared/Utils;"

object GmsCoreSupportFingerprint : Fingerprint(
    accessFlags = listOf(AccessFlags.PRIVATE, AccessFlags.STATIC),
    returnType = "Ljava/lang/String;",
    parameters = listOf(),
    custom = { method, classDef ->
        method.name == "getGmsCoreVendorGroupId" &&
            classDef.type == EXTENSION_CLASS_DESCRIPTOR
    },
)
```

Use `custom` when strings, literals, and signature data still leave too many candidates, or when
you need to express exact method/class logic.

### Programmatic fingerprint builder

```kotlin
import app.morphe.patcher.Fingerprint

fun getMainOnCreateFingerprint(
    activityClassType: String,
    targetBundleMethod: Boolean = true,
): Fingerprint {
    require(activityClassType.endsWith(';'))

    val fullClassType = activityClassType.startsWith('L')

    return Fingerprint(
        returnType = "V",
        parameters = if (targetBundleMethod) listOf("Landroid/os/Bundle;") else listOf(),
        custom = { method, classDef ->
            method.name == "onCreate" &&
                if (fullClassType) classDef.type == activityClassType
                else classDef.type.endsWith(activityClassType)
        },
    )
}
```

Use a fingerprint factory when one stable rule needs to be reused across several app-specific class
targets.

### Compatibility declaration

```kotlin
import app.morphe.patcher.patch.AppTarget
import app.morphe.patcher.patch.Compatibility
import app.morphe.patcher.patch.bytecodePatch

val enablePrimePatch = bytecodePatch(
    name = "Enable Prime",
    description = "Enables subscription-only features.",
) {
    compatibleWith(
        Compatibility(
            name = "Nova Launcher",
            packageName = "com.teslacoilsw.launcher",
            targets = listOf(AppTarget("81042 (8.5.1)")),
            appIconColor = 0xDA4624,
        ),
    )

    execute {
        // Patch logic.
    }
}
```

Use `compatibleWith` when the patch is intended for a specific app or known versions. It is
metadata and guardrail information, not a replacement for good fingerprints.

## Advanced bytecode patch examples

These are real-world patterns. Use them when simple early-return patches are not enough.

### Derive a parameter type and return type from matched methods

```kotlin
execute {
    val licenseTypeClass = PaidLicenseFingerprint.method.parameters[1].type
    val lifetimeDurationInstance = LifetimeDurationFingerprint.classDef.staticFields.first()

    val getPaidLicenseMethod = ImmutableMethod(
        GetPlusStateFingerprint.classDef.type,
        "getPaidLicense",
        null,
        GetPlusStateFingerprint.method.returnType,
        AccessFlags.STATIC.value,
        null,
        null,
        MutableMethodImplementation(7)
    ).toMutable().apply {
        addInstructions(0, """
        new-instance v0, ${PaidLicenseFingerprint.classDef.type}
        const-string v1, ""
        sget-object v2, $licenseTypeClass->Personal:$licenseTypeClass
        sget-object v3, $lifetimeDurationInstance
        const/4 v4, 0x1
        const/4 v5, 0x3
        const-string v6, ""
        invoke-direct/range {v0 .. v6}, ${PaidLicenseFingerprint.method}
        return-object v0
    """.trimIndent())
    }

    GetPlusStateFingerprint.classDef.methods.add(getPaidLicenseMethod)

    GetPlusStateFingerprint.method.addInstructions(0, """
        invoke-static {}, $getPaidLicenseMethod
        move-result-object v0
        return-object v0
    """.trimIndent())
}
```

Why this matters:

- `parameters[1].type` avoids hardcoding an enum or class descriptor.
- `method.returnType` keeps the helper method compatible with the matched target.
- `${PaidLicenseFingerprint.method}` reuses the real constructor signature instead of manually
  rebuilding it.

### Extract a `FieldReference`, resolve its type, then discover methods on that type

```kotlin
execute {
    PurchaseItemsCtor.apply {
        val sputInstr =  this.instructionMatches[2].getInstruction<Instruction21c>()
        val purchasableItemField = sputInstr.getReference<FieldReference>()!!
        val purchasableItemType = classDefBy(purchasableItemField.type)

        val itemSetMethod = PurchasableItemSetFingerprint.match(purchasableItemType).originalMethod
        val itemGetMethod = PurchasableItemGetFingerprint.match(purchasableItemType).originalMethod

        this.method.addInstructionsWithLabels(
            this.method.instructions.size - 1,
            """
                invoke-virtual {v0}, ${purchasableItemField.type}->${itemGetMethod.name}()Z
                move-result v0
                if-nez v0, :end
                sget-object v0, ${this.classDef.type}->${purchasableItemField.name}:${purchasableItemField.type}
                invoke-static {}, ${GetAppFingerprint.classDef.type}->${GetAppFingerprint.method.name}()${GetAppFingerprint.method.returnType}
                move-result-object v1
                const/4 v2, 0x1
                invoke-virtual {v0, v1, v2}, ${purchasableItemField.type}->${itemSetMethod.name}(Landroid/content/Context;Z)V
            """.trimIndent(),
            ExternalLabel("end", this.method.instructions.last())
        )
    }
}
```

Why this matters:

- pulls the real `FieldReference` out of matched instructions
- resolves the field type into a class and fingerprints methods on that resolved class
- injects discovered method names and return type instead of hardcoding obfuscated members

### Discover obfuscated fields from usage sites instead of hardcoding names

```kotlin
internal fun ClassDef.fieldFromToString(subStr: String): FieldReference {
    val toString = this.toStringMethod()
    val strIndex = toString.indexOfFirstInstructionOrThrow() {
        this.opcode == Opcode.CONST_STRING &&
                getReference<StringReference>()?.string?.contains(subStr) ?: false
    }
    val field = toString.getInstruction<ReferenceInstruction>(strIndex + 2).getReference<FieldReference>()
    return field ?: throw PatchException("Could not find field: $subStr")
}
```

```kotlin
execute {
    val hasPlusField = UserFingerprint.classDef.fieldFromToString("hasPlus")
    val subscriberLevelField = UserFingerprint.classDef.fieldFromToString("subscriberLevel")

    val isPaidField = UserIsPaidFieldUsageFingerprint.method.let {
        val isPaidIndex = it.indexOfFirstInstructionOrThrow(Opcode.IGET_BOOLEAN)
        it.getInstruction<ReferenceInstruction>(isPaidIndex).getReference<FieldReference>()!!
    }
    val hasGoldField = UserHasGoldFieldUsageFingerprint.method.let {
        val hasGoldIndex = it.indexOfFirstInstructionOrThrow(Opcode.IGET_BOOLEAN)
        it.getInstruction<ReferenceInstruction>(hasGoldIndex).getReference<FieldReference>()!!
    }
}
```

Why this matters:

- field names can stay obfuscated or unstable while field usage patterns remain stable
- you can often recover a usable `FieldReference` from `iget`, `iput`, `sget`, `sput`, or nearby
  builder code in `toString()`

### Find a method invocation by reference, then reuse the discovered result register

```kotlin
EnterServerInsertedAdBreakStateFingerprint.method.apply {
    val playerIndex = indexOfFirstInstructionOrThrow() {
        opcode == Opcode.INVOKE_VIRTUAL && getReference<MethodReference>()?.name == "getPrimaryPlayer"
    }
    val playerRegister = getInstruction<OneRegisterInstruction>(playerIndex + 1).registerA

    addInstructions(
        playerIndex + 2,
        """
            invoke-static { p0, p1, v$playerRegister }, Lapp/morphe/extension/primevideo/ads/SkipAdsPatch;->enterServerInsertedAdBreakState(Lcom/amazon/avod/media/ads/internal/state/ServerInsertedAdBreakState;Lcom/amazon/avod/media/ads/internal/state/AdBreakTrigger;Lcom/amazon/avod/media/playback/VideoPlayer;)V
            return-void
        """
    )
}
```

Why this matters:

- the patch does not assume which `v` register receives the `move-result-object`
- it anchors on a stable `MethodReference` name and then reads the real register from bytecode

### Read a register from an existing instruction and inject into that exact register

```kotlin
execute {
    val emptySignalRef = BasicLoginFingerprint.method.let {
        it.getInstruction<Instruction21c>(
            it.indexOfFirstInstruction(Opcode.SGET_OBJECT)
        ).reference
    }

    LoginStateFingerprint.method.apply {
        val signalNullCheckIndex =
            this.indexOfFirstInstruction(LoginStateFingerprint.stringMatches.last().index, Opcode.INVOKE_STATIC)
        val signalParamReg = this.getInstruction<Instruction35c>(signalNullCheckIndex).registerC

        this.addInstruction(
            0, "sget-object v$signalParamReg, $emptySignalRef"
        )
    }
}
```

Why this matters:

- it reuses a live object reference discovered elsewhere in the app
- it writes into the exact argument register already used by the target invoke

### Inspect method instructions to patch the final return object register

```kotlin
ShouldInterceptRequestFingerprint.method.apply {
    val returnObjReg = getInstruction<OneRegisterInstruction>(instructions.size - 1).registerA

    addInstructions(instructions.size - 1, """
        invoke-static { p2, v$returnObjReg }, Lapp/morphe/extension/windy/premium/EnablePremiumPatch;->patchAppJavascript(Landroid/webkit/WebResourceRequest;Landroid/webkit/WebResourceResponse;)V
    """.trimIndent())
}
```

Why this matters:

- useful when you need to observe or mutate the object about to be returned
- avoids assuming the return object is always in `v0`

### Walk the whole app, inspect references, preserve registers, and branch on return type

```kotlin
fun transformStringReferences(transform: (str: String) -> String?) = classDefForEach {
    val mutableClass by lazy {
        mutableClassDefBy(it)
    }

    it.methods.forEach classLoop@{ method ->
        val implementation = method.implementation ?: return@classLoop

        val mutableMethod by lazy {
            mutableClass.findMutableMethodOf(method)
        }

        implementation.instructions.forEachIndexed { index, instruction ->
            val string = ((instruction as? Instruction21c)?.reference as? StringReference)?.string
                ?: return@forEachIndexed

            val transformedString = transform(string) ?: return@forEachIndexed

            mutableMethod.replaceInstruction(
                index,
                BuilderInstruction21c(
                    Opcode.CONST_STRING,
                    instruction.registerA,
                    ImmutableStringReference(transformedString),
                ),
            )
        }
    }
}
```

```kotlin
earlyReturnFingerprints.forEach {
    it.method.apply {
        if (returnType == "Z") {
            returnEarly(false)
        } else {
            returnEarly()
        }
    }
}
```

Why this matters:

- demonstrates whole-app bytecode scanning, not just one matched method
- preserves the original destination register with `instruction.registerA`
- uses `returnType` to choose the correct patch strategy generically

## Advanced guidance

- Prefer extracting `FieldReference`, `MethodReference`, registers, parameter types, and return
  types from matched code over hardcoding them.
- If you must inject smali that refers to an app method or field, try to derive the owning type,
  member name, and descriptor from a matched instruction first.
- When patching obfuscated apps, stable structure usually means opcodes, access patterns, string
  anchors, constructor shapes, and reference types, not member names.
- If you need a helper method, consider synthesizing one with `ImmutableMethod` and
  `MutableMethodImplementation` rather than forcing complex logic inline.

## Final reminder

If `patch-tools run main.kts` does not print the expected diff for the patch you are working on,
the patch is not working yet. Do not stop at a clean run with no relevant diff. Keep iterating
until the expected diff appears.

If you are iterating on one patch or fingerprint, temporarily return only that target item from the
script. Extra returned items pollute the output and can hide whether the current work actually
succeeded.

If a diff appears on the wrong method or wrong class, the patch is not working. A diff alone is not
success; it must be the expected diff on the intended target.

If a patch depends on a fingerprint, verify the fingerprint target first before trusting the patch
result.

If the user asks to migrate a patch or fingerprint to a newer app version, load both the old and new
packages and use `patch-tools map <old_apk> <method_id> <new_apk>` to find the likely equivalent
methods before editing code. Once the mapped methods are confirmed, try
`patch-tools common-fingerprint ...` to derive a shared target; if no common fingerprint is found,
use fresh `patch-tools fingerprint ...` output for the new version instead of carrying over old
minified names by hand.

If there is no diff, or the diff is on the wrong target, do not treat the run as success.

If you changed which items are returned, or changed their order while iterating, restore the
intended stable return list before finishing.

If a patch only works because of a temporary debug hack, brittle assumption, or manual one-off
adjustment, it is not finished.

Do not write code that depends on minified or obfuscated names like single-letter method or field
references. Avoid injected smali that hardcodes unstable calls such as `->a(...)` or `->b(...)`.

Do not hardcode registers unless the register choice is structurally guaranteed. Prefer deriving the
correct register from the matched instructions or method shape. Small self-contained early-return
patches that use `v0` directly are acceptable when the injected code fully controls that register.

Do not try to generate fingerprints by yourself when you already have a target method. Use
`patch-tools fingerprint [apk] <method_id>` first, then adapt the generated fingerprint if needed.
You can omit `[apk]` when exactly one APK is loaded.
