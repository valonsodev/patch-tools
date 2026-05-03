package dev.valonso.tools.engine.patcher

import app.morphe.patcher.Patcher
import app.morphe.patcher.patch.ResourcePatchContext
import app.morphe.patcher.resource.coder.ResourceCoder

private val resourceCoderField =
    ResourcePatchContext::class.java.getDeclaredField("resourceCoder").apply {
        isAccessible = true
    }

fun Patcher.deletedResourcePaths(): Set<String> {
    val resourceCoder = resourceCoderField.get(context.resourceContext) as ResourceCoder
    return resourceCoder.getDeletedFiles()
}
