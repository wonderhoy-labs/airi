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
import com.wonderhoylabs.airi.linux.Linux
import com.wonderhoylabs.airi.linux.Minori
import org.json.JSONArray
import java.io.FileNotFoundException
import java.util.concurrent.Callable
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
 * adb shell content read --uri 'content://com.wonderhoylabs.airi.apps/icon/foot.desktop?source=minori:<machine>' > icon.png
 * ```
 *
 * Each row has `desktop_id`, `on_launcher` (1 if Airi has put it on the launcher), `name` and `source`, in that order.
 * The source says which Linux it's in: `terminal` for Android's terminal, or `minori:<machine ID>` for one of Minori's
 * machines. It came last, after the name, in Airi 0.4.0, so a reader of 0.3.0's rows still finds its columns where they
 * were; take the name as everything between `name=` and the last `, source=`. Open an app with the link [Linux.appUri]
 * makes: `airi://app/<desktop_id>` for the terminal, or `airi://minori/<machine ID>/<desktop_id>`. The lists come from
 * Linux, so a Linux that isn't running has no rows; Airi's own screen starts the terminal, and Minori starts machines.
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
		val context = context!!
		val machines = Minori.machines(context).filter { it.airi && it.running }.map { it.linux }
		val sources = listOf(Linux.Terminal) + machines
		// Each Linux answers on its own, so one that's slow doesn't hold up the rest.
		val pool = Executors.newFixedThreadPool(sources.size)
		val lists = try {
			pool.invokeAll(
				sources.map { linux ->
					Callable { runCatching { withAgent(linux) { agent -> listApps(agent) } }.getOrDefault(emptyList()) }
				},
			).map { it.get() }
		} finally {
			pool.shutdownNow()
		}
		val onLauncher = LauncherApps.installed(context)
		val cursor = MatrixCursor(COLUMNS)
		sources.zip(lists).forEach { (linux, apps) ->
			apps.sortedBy { it.second.lowercase() }.forEach { (id, name) ->
				val added = if (AppBuilder.packageFor(linux, id) in onLauncher) 1 else 0
				cursor.addRow(arrayOf<Any>(id, added, name, linux.key))
			}
		}
		return cursor
	}

	override fun openFile(uri: Uri, mode: String): ParcelFileDescriptor {
		val segments = uri.pathSegments
		if (mode != "r" || segments.size != 2 || segments[0] != ICON) throw FileNotFoundException("No such file: $uri")
		val linux = uri.getQueryParameter(SOURCE)?.let { Linux.fromKey(it) ?: throw FileNotFoundException("No Linux $it") }
			?: Linux.Terminal
		val png =
			runCatching { withAgent(linux) { agent -> icon(agent, segments[1]) } }.getOrNull()
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

	/** Talks to an agent for one answer. Its reads block, so a stuck Linux is cut off by closing the connection. */
	private fun <T> withAgent(linux: Linux, ask: (AgentConnection) -> T): T {
		val agent = AgentSetup.connect(context!!, linux)
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
		private const val SOURCE = "source"
		val COLUMNS = arrayOf("desktop_id", "on_launcher", "name", "source")
		private const val ANSWER_TIMEOUT_SECONDS = 5L
	}
}
