package dev.valonso.tools.engine.smali

import com.android.tools.smali.baksmali.Adaptors.ClassDefinition
import com.android.tools.smali.baksmali.Adaptors.Format.InstructionMethodItemFactory
import com.android.tools.smali.baksmali.Adaptors.MethodDefinition
import com.android.tools.smali.baksmali.Adaptors.MethodItem
import com.android.tools.smali.baksmali.Adaptors.RegisterFormatter
import com.android.tools.smali.baksmali.BaksmaliOptions
import com.android.tools.smali.baksmali.formatter.BaksmaliWriter
import com.android.tools.smali.dexlib2.AccessFlags
import com.android.tools.smali.dexlib2.iface.ClassDef
import com.android.tools.smali.dexlib2.iface.Method
import com.android.tools.smali.dexlib2.iface.instruction.Instruction
import com.android.tools.smali.dexlib2.util.TypeUtils
import java.io.StringWriter

fun MethodDefinition.getParameterRegisterCount(): Int {
    var parameterRegisterCount = 0
    if (!AccessFlags.STATIC.isSet(method.accessFlags)) {
        parameterRegisterCount++
    }
    for (parameter in methodParameters) {
        val type = parameter.type
        parameterRegisterCount++
        if (TypeUtils.isWideType(type)) {
            parameterRegisterCount++
        }
    }
    return parameterRegisterCount
}

fun ClassDef.toClassDefinition(): ClassDefinition = ClassDefinition(BaksmaliOptions(), this)

fun ClassDef.toSmaliString(): String {
    val stringWriter = StringWriter()
    val baksmaliWriter = BaksmaliWriter(stringWriter, null)
    this.toClassDefinition().writeTo(baksmaliWriter)
    return stringWriter.toString()
}

/**
 * Renders just the class header (directives, interfaces, annotations, fields)
 * without methods. Useful for detecting class-level structural changes.
 */
fun ClassDef.toHeaderSmaliString(): String {
    val fullSmali = toSmaliString()
    // baksmali always writes fields before methods. Trim at the first .method directive.
    val firstMethodIdx = fullSmali.indexOf("\n.method ")
    return if (firstMethodIdx >= 0) fullSmali.substring(0, firstMethodIdx).trimEnd() else fullSmali.trimEnd()
}

fun Method.toMethodDefinition(classDef: ClassDef): MethodDefinition? {
    val classDefinition = classDef.toClassDefinition()
    // Return null if the method has no implementation (abstract, interface, or native methods)
    val impl = implementation ?: return null
    val methodDefinition = MethodDefinition(classDefinition, this, impl)
    methodDefinition.registerFormatter =
        RegisterFormatter(
            methodDefinition.classDef.options,
            methodDefinition.methodImpl.registerCount,
            methodDefinition.getParameterRegisterCount(),
        )
    return methodDefinition
}

fun Method.toSmaliString(classDef: ClassDef): String? {
    val stringWriter = StringWriter()
    val baksmaliWriter = BaksmaliWriter(stringWriter, null)
    this.toMethodDefinition(classDef)?.writeTo(baksmaliWriter) ?: return null
    return stringWriter.toString()
}

fun Instruction.toSmaliString(
    classDef: ClassDef,
    method: Method,
): String? {
    val stringWriter = StringWriter()
    val baksmaliWriter = BaksmaliWriter(stringWriter, null)
    val methodDefinition = method.toMethodDefinition(classDef) ?: return null
    val methodItem: MethodItem =
        InstructionMethodItemFactory.makeInstructionFormatMethodItem(methodDefinition, 0, this)
    methodItem.writeTo(baksmaliWriter)
    return stringWriter.toString()
}
