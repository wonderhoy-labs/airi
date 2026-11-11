package com.wonderhoylabs.airi.apps

import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.database.MatrixCursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import com.wonderhoylabs.airi.agent.AgentConnection
import com.wonderhoylabs.airi.agent.AgentSetup
import com.wonderhoylabs.airi.agent.Payload
import com.wonderhoylabs.airi.agent.Protocol
import com.wonderhoylabs.airi.launcher.AppBuilder
import com.wonderhoylabs.airi.launcher.LauncherApps
import org.json.JSONArray
import java.io.FileNotFoundException
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import kotlin.concurrent.thread

/**
 * The Linux apps Airi can open, for the shell user and anything else holding `android.permission.DUMP`, such as a web
 * page driving the phone over adb:
 *
 * ```
 * adb shell content query --uri content://com.wonderhoylabs.airi.apps/apps
 * adb shell content read --uri content://com.wonderhoylabs.airi.apps/icon/org.gnome.Calculator.desktop > icon.png
 * ```
 *
 * Each row has `desktop_id`, `on_launcher` (1 if Airi has put it on the launcher) and `name`, in that order, so the
 * name, which can hold anything, comes last. Open one with `airi://app/<desktop_id>`. The list comes from Linux, so
 * there are no rows while Linux isn't running; Airi's own screen starts it.
 */
class AppsProvider : ContentProvider() {
	override fun onCreate(): Boolean = true

	override fun query(
		uri: Uri,
		projection: Array<out String>?,
		selection: String?,
		selectionArgs: Array<out String>?,
		sortOrder: String?,
	): Cursor {
		if (uri.pathSegments != listOf(APPS)) throw IllegalArgumentException("Airi only lists $APPS, not $uri")
		val cursor = MatrixCursor(COLUMNS)
		val apps = runCatching { withAgent { agent -> listApps(agent) } }.getOrDefault(emptyList())
		val onLauncher = LauncherApps.installed(context!!)
		apps.sortedBy { it.second.lowercase() }.forEach { (id, name) ->
			cursor.addRow(arrayOf<Any>(id, if (AppBuilder.packageFor(id) in onLauncher) 1 else 0, name))
		}
		return cursor
	}

	override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor {
		val segments = uri.pathSegments
		if (mode != "r" || segments.size != 2 || segments[0] != ICON) throw FileNotFoundException("No such file: $uri")
		val png =
			runCatching { withAgent { agent -> icon(agent, segments[1]) } }.getOrNull()
				?: throw FileNotFoundException("Linux has no icon for ${segments[1]}, or isn't running")
		val (read, write) = ParcelFileDescriptor.createPipe()
		thread(name = "airi-icon", isDaemon = true) {
			ParcelFileDescriptor.AutoCloseOutputStream(write).use { runCatching { it.write(png) } }
		}
		return read
	}

	override fun getType(uri: Uri): String? = when (uri.pathSegments.firstOrNull()) {
		APPS -> "vnd.android.cursor.dir/vnd.com.wonderhoylabs.airi.app"
		ICON -> "image/png"
		else -> null
	}

	override fun insert(uri: Uri, values: ContentValues?): Uri? = throw UnsupportedOperationException("Read only")

	override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?): Int =
		throw UnsupportedOperationException("Read only")

	override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<out String>?): Int =
		throw UnsupportedOperationException("Read only")

	/** Talks to the agent for one answer. Its reads block, so a stuck Linux is cut off by closing the connection. */
	private fun <T> withAgent(ask: (AgentConnection) -> T): T {
		val agent = AgentSetup.connect(context!!)
		val timer = Executors.newSingleThreadScheduledExecutor()
		timer.schedule({ agent.close() }, ANSWER_TIMEOUT_SECONDS, TimeUnit.SECONDS)
		try {
			return ask(agent)
		} finally {
			timer.shutdownNow()
			agent.finish()
		}
	}

	private fun listApps(agent: AgentConnection): List<Pair<String, String>> {
		agent.send(Protocol.LIST_APPS)
		while (true) {
			val message = agent.read()
			if (message.type != Protocol.APPS) continue
			val list = JSONArray(message.rest().decodeToString())
			return List(list.length()) { index ->
				val app = list.getJSONObject(index)
				app.getString("id") to app.getString("name")
			}
		}
	}

	private fun icon(agent: AgentConnection, desktopId: String): ByteArray? {
		agent.send(Protocol.GET_ICON, Payload().string(desktopId))
		while (true) {
			val message = agent.read()
			if (message.type != Protocol.ICON || message.string() != desktopId) continue
			return message.rest().takeIf(ByteArray::isNotEmpty)
		}
	}

	companion object {
		/** The provider's authority. */
		const val AUTHORITY = "com.wonderhoylabs.airi.apps"
		private const val APPS = "apps"
		private const val ICON = "icon"
		private val COLUMNS = arrayOf("desktop_id", "on_launcher", "name")
		private const val ANSWER_TIMEOUT_SECONDS = 5L
	}
}
