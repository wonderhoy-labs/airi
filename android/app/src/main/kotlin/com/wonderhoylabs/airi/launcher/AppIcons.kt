package com.wonderhoylabs.airi.launcher

import android.content.Context
import java.io.File
import java.security.MessageDigest

/** Remembers each Linux app's name and icon, so that a window can show them before Linux is up. */
object AppIcons {
	private fun file(context: Context, desktopId: String, extension: String): File {
		val name = MessageDigest.getInstance("SHA-256").digest(desktopId.toByteArray()).toHexString().take(32)
		return File(File(context.filesDir, "apps").apply { mkdirs() }, "$name.$extension")
	}

	fun save(context: Context, desktopId: String, name: String, png: ByteArray?) {
		file(context, desktopId, "name").writeText(name)
		if (png != null) file(context, desktopId, "png").writeBytes(png)
	}

	fun name(context: Context, desktopId: String): String? =
		file(context, desktopId, "name").takeIf(File::exists)?.readText()

	fun load(context: Context, desktopId: String): ByteArray? =
		file(context, desktopId, "png").takeIf(File::exists)?.readBytes()
}
