package dev.valonso.tools.engine.scripting

fun print(value: Any?) {
    ScriptOutputSink.print(value.toString())
}

fun println() {
    ScriptOutputSink.println("")
}

fun println(value: Any?) {
    ScriptOutputSink.println(value.toString())
}
