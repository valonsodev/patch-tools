package dev.valonso.tools.engine.method

import com.android.tools.smali.dexlib2.AccessFlags
import com.android.tools.smali.dexlib2.analysis.reflection.util.ReflectionUtils
import com.android.tools.smali.dexlib2.iface.Method

fun dexToJavaNameWithNestedClasses(dexName: String): String = ReflectionUtils.dexToJavaName(dexName).replace('$', '.')

// shortId:
// onFrameProcessed(Lcom/google/android/libraries/vision/common/FrameProcessor$Frame;Lcom/google/android/libraries/vision/common/FrameProcessor$Frame;)V
val Method.shortId: String
    get() {
        return "${this.name}(${this.parameterTypes.joinToString(separator = "") { it.toString() }})${this.returnType}"
    }

// uniqueId:
// Lcom/google/android/libraries/vision/common/FrameProcessor$FrameProcessorListener;->onFrameProcessed(Lcom/google/android/libraries/vision/common/FrameProcessor$Frame;Lcom/google/android/libraries/vision/common/FrameProcessor$Frame;)V
val Method.uniqueId: String
    get() {
        return "${this.definingClass}->${this.shortId}"
    }
val Method.javaClassName: String
    get() {
        return dexToJavaNameWithNestedClasses(this.definingClass)
    }
val Method.javaMethodName: String
    get() {
        return this.name
    }
val Method.javaParameterTypes: List<String>
    get() {
        return this.parameterTypes.map { dexToJavaNameWithNestedClasses(it.toString()) }
    }
val Method.javaReturnType: String
    get() {
        return ReflectionUtils.dexToJavaName(this.returnType)
    }
val Method.javaSignature: String
    get() {
        return "${this.javaClassName}.${this.javaMethodName}(${
            this.javaParameterTypes.joinToString(
                separator = ", ",
            )
        })${this.javaReturnType}"
    }
val Method.javaAccessFlags: List<String>
    get() {
        return AccessFlags.getAccessFlagsForMethod(this.accessFlags).map { it.toString() }
    }
