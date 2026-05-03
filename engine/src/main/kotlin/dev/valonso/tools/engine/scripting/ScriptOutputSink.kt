package dev.valonso.tools.engine.scripting

internal object ScriptOutputSink {
    private val collectorRef = ThreadLocal<ScriptOutputCollector?>()

    fun print(text: String) {
        collectorRef.get()?.print(text)
    }

    fun println(text: String) {
        collectorRef.get()?.println(text)
    }

    suspend fun <T> withCallback(
        callback: ((String) -> Unit)?,
        block: suspend () -> T,
    ): T {
        val collector = callback?.let(::ScriptOutputCollector)
        val previous = collectorRef.get()
        collectorRef.set(collector)
        try {
            return block()
        } finally {
            collector?.flushRemainder()
            collectorRef.set(previous)
        }
    }
}

private class ScriptOutputCollector(
    private val callback: (String) -> Unit,
) {
    private val lineBuffer = StringBuilder()

    fun print(text: String) {
        lineBuffer.append(text)
    }

    fun println(text: String) {
        lineBuffer.append(text)
        flushLine()
    }

    fun flushRemainder() {
        if (lineBuffer.isNotEmpty()) {
            flushLine()
        }
    }

    private fun flushLine() {
        callback(lineBuffer.toString())
        lineBuffer.setLength(0)
    }
}
