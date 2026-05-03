package dev.valonso.tools.engine.fingerprint

import app.morphe.patcher.extensions.InstructionExtensions.instructionsOrNull
import com.android.tools.smali.dexlib2.Opcode
import com.android.tools.smali.dexlib2.iface.Method
import com.android.tools.smali.dexlib2.iface.instruction.Instruction
import com.android.tools.smali.dexlib2.iface.instruction.ReferenceInstruction
import com.android.tools.smali.dexlib2.iface.instruction.WideLiteralInstruction
import com.android.tools.smali.dexlib2.iface.reference.FieldReference
import com.android.tools.smali.dexlib2.iface.reference.MethodReference
import com.android.tools.smali.dexlib2.iface.reference.StringReference
import com.android.tools.smali.dexlib2.iface.reference.TypeReference
// ---------------------------------------------------------------------------
//  Data model for method features used in fingerprint generation.
//
//  Design rationale:
//  - MethodSignature groups the three "general" features (return type, access
//    flags, full parameter list). Parameters are all-or-nothing because the
//    Morphe Fingerprint constructor requires a complete ordered list or null.
//  - InstructionFeature models the interesting instruction-level
//    observations. Each carries its bytecode index so we can preserve ordering.
//  - MethodFeatures is the extracted method feature set exported to Rust for
//    fingerprint generation and ranking.
// ---------------------------------------------------------------------------

/**
 * General signature-level features of a method.
 *
 * @property returnType Smali return type descriptor, e.g. `"V"`, `"Ljava/lang/String;"`.
 * @property accessFlags Raw dexlib2 access-flag bitfield.
 * @property parameters Full ordered parameter type list, or null when unknown.
 *                      Empty list means the method takes zero parameters.
 */
data class MethodSignature(
    val returnType: String,
    val accessFlags: Int,
    val parameters: List<String>,
)

/**
 * An instruction-level feature extracted from a method's bytecode.
 *
 * @property index Position inside the method's instruction list (0-based).
 */
sealed class InstructionFeature : Comparable<InstructionFeature> {
    abstract val index: Int

    override fun compareTo(other: InstructionFeature): Int = index.compareTo(other.index)

    data class Literal(
        override val index: Int,
        val value: Long,
    ) : InstructionFeature()

    data class StringConst(
        override val index: Int,
        val string: String,
    ) : InstructionFeature()

    data class MethodCall(
        override val index: Int,
        val definingClass: String,
        val sameDefiningClass: Boolean,
        val useThisDefiningClass: Boolean = false,
        val name: String,
        val parameters: List<String>,
        val returnType: String,
    ) : InstructionFeature()

    data class FieldAccess(
        override val index: Int,
        val definingClass: String,
        val sameDefiningClass: Boolean,
        val useThisDefiningClass: Boolean = false,
        val name: String,
        val type: String,
    ) : InstructionFeature()

    data class NewInstance(
        override val index: Int,
        val type: String,
    ) : InstructionFeature()

    data class InstanceOf(
        override val index: Int,
        val type: String,
    ) : InstructionFeature()

    data class CheckCast(
        override val index: Int,
        val type: String,
    ) : InstructionFeature()
}

/**
 * All features extracted from a single method. This is the *input* to the
 * fingerprint generation algorithm on the Rust side.
 */
data class MethodFeatures(
    val signature: MethodSignature,
    val instructions: List<InstructionFeature>,
) {
    companion object {
        /**
         * Extract every observable feature from [method].
         *
         * String constants have special characters escaped so the rendered
         * Kotlin source code is valid.
         */
        fun extract(method: Method): MethodFeatures {
            val instructions =
                buildList {
                    method.instructionsOrNull?.forEachIndexed { index, instruction ->
                        when {
                            instruction.isStringInstruction() -> {
                                val ref = (instruction as ReferenceInstruction).reference as? StringReference
                                if (ref != null) {
                                    add(
                                        InstructionFeature.StringConst(
                                            index,
                                            ref.string,
                                        ),
                                    )
                                }
                            }

                            instruction is WideLiteralInstruction -> {
                                add(InstructionFeature.Literal(index, instruction.wideLiteral))
                            }

                            instruction is ReferenceInstruction && instruction.reference is MethodReference -> {
                                val ref = instruction.reference as MethodReference
                                add(
                                    InstructionFeature.MethodCall(
                                        index = index,
                                        definingClass = ref.definingClass,
                                        sameDefiningClass = ref.definingClass == method.definingClass,
                                        useThisDefiningClass = false,
                                        name = ref.name,
                                        parameters = ref.parameterTypes.map { it.toString() },
                                        returnType = ref.returnType,
                                    ),
                                )
                            }

                            instruction is ReferenceInstruction && instruction.reference is FieldReference -> {
                                val ref = instruction.reference as FieldReference
                                add(
                                    InstructionFeature.FieldAccess(
                                        index = index,
                                        definingClass = ref.definingClass,
                                        sameDefiningClass = ref.definingClass == method.definingClass,
                                        useThisDefiningClass = false,
                                        name = ref.name,
                                        type = ref.type,
                                    ),
                                )
                            }

                            instruction is ReferenceInstruction && instruction.reference is TypeReference -> {
                                val ref = instruction.reference as TypeReference
                                when (instruction.opcode) {
                                    Opcode.NEW_INSTANCE,
                                    Opcode.NEW_ARRAY,
                                    -> add(InstructionFeature.NewInstance(index, ref.type))

                                    Opcode.INSTANCE_OF -> add(InstructionFeature.InstanceOf(index, ref.type))

                                    Opcode.CHECK_CAST -> add(InstructionFeature.CheckCast(index, ref.type))

                                    else -> Unit
                                }
                            }

                            else -> {
                                // Intentionally skip raw opcode features — they are broad and brittle.
                            }
                        }
                    }
                }

            return MethodFeatures(
                signature =
                    MethodSignature(
                        returnType = method.returnType,
                        accessFlags = method.accessFlags,
                        parameters = method.parameters.map { it.type },
                    ),
                instructions = instructions.sorted(),
            )
        }
    }
}

// ---------------------------------------------------------------------------
//  Private helpers
// ---------------------------------------------------------------------------

private fun Instruction.isStringInstruction(): Boolean = opcode == Opcode.CONST_STRING || opcode == Opcode.CONST_STRING_JUMBO
