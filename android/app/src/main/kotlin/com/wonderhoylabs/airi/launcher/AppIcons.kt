package com.wonderhoylabs.airi.launcher

import android.content.Context
import com.wonderhoylabs.airi.linux.Linux
import java.io.File
import java.security.MessageDigest

/** Remembers each Linux app's name and icon, so that a window can show them before Linux is up. */
object AppIcons {
	private fun file(context: Context, linux: Linux, desktopId: String, extension: String): File {
		// The terminal's apps keep the files they had before Airi knew Minori.
		val key = if (linux is Linux.Machine) "${linux.key}/$desktopId" else desktopId
		val name = MessageDigest.getInstance("SHA-256").digest(key.toByteArray()).toHexString().take(32)
		return File(File(context.filesDir, "apps").apply { mkdirs() }, "$name.$extension")
	}

	fun save(context: Context, linux: Linux, desktopId: String, name: String, png: ByteArray?) {
		file(context, linux, desktopId, "name").writeText(name)
		if (png != null) file(context, linux, desktopId, "png").writeBytes(png)
	}

	fun name(context: Context, linux: Linux, desktopId: String): String? =
		file(context, linux, desktopId, "name").takeIf(File::exists)?.readText()

	fun load(context: Context, linux: Linux, desktopId: String): ByteArray? =
		file(context, linux, desktopId, "png").takeIf(File::exists)?.readBytes()
}
