package dev.valonso.tools.engine.session

import com.android.tools.smali.dexlib2.iface.Method
import dev.valonso.tools.engine.method.javaAccessFlags
import dev.valonso.tools.engine.method.javaClassName
import dev.valonso.tools.engine.method.javaParameterTypes
import dev.valonso.tools.engine.method.javaReturnType
import dev.valonso.tools.engine.method.javaSignature
import dev.valonso.tools.engine.method.shortId
import dev.valonso.tools.engine.method.uniqueId

data class MethodInfo(
    val uniqueId: String,
    val definingClass: String,
    val name: String,
    val returnType: String,
    val parameters: List<String>,
    val accessFlags: Int,
    val className: String,
    val javaReturnType: String,
    val javaParameterTypes: List<String>,
    val javaAccessFlags: List<String>,
    val shortId: String,
    val javaSignature: String,
)

fun Method.toMethodInfo(): MethodInfo =
    MethodInfo(
        uniqueId = uniqueId,
        definingClass = definingClass,
        name = name,
        returnType = returnType,
        parameters = parameterTypes.map { it.toString() },
        accessFlags = accessFlags,
        className = javaClassName,
        javaReturnType = javaReturnType,
        javaParameterTypes = javaParameterTypes,
        javaAccessFlags = javaAccessFlags,
        shortId = shortId,
        javaSignature = javaSignature,
    )
