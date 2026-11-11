package com.wonderhoylabs.airi.linux

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.ParcelFileDescriptor
import java.io.FileNotFoundException
import java.io.IOException

/**
 * Minori's side of reaching its machines, which Minori documents in its docs/airi.md. Minori lists the machines Airi's
 * agent can run in, connects Airi to the agent in a running one, and starts and sets up machines when asked.
 */
object Minori {
	const val PACKAGE = "com.wonderhoylabs.minori"
	private const val AUTHORITY = "com.wonderhoylabs.minori.airi"
	private val MACHINES: Uri = Uri.parse("content://$AUTHORITY/machines")
	private const val ACTION_START = "com.wonderhoylabs.minori.action.START_FOR_AIRI"
	private const val ACTION_SET_UP = "com.wonderhoylabs.minori.action.SET_UP_AIRI"
	private const val EXTRA_MACHINE = "machine"
	private const val EXTRA_TOKEN = "token"

	/** A machine as Minori describes it. [airi] says whether Airi's agent is installed in it. */
	data class Machine(val id: String, val name: String, val state: String, val airi: Boolean) {
		val running: Boolean get() = state == "running"
		val linux: Linux.Machine get() = Linux.Machine(id)
	}

	/** Minori's machines that Airi's agent can run in. Empty without a Minori that knows Airi, which is 0.4.0 on. */
	fun machines(context: Context): List<Machine> = try {
		context.contentResolver.query(MACHINES, null, null, null, null)?.use { cursor ->
			val id = cursor.getColumnIndexOrThrow("id")
			val state = cursor.getColumnIndexOrThrow("state")
			val airi = cursor.getColumnIndexOrThrow("airi")
			val name = cursor.getColumnIndexOrThrow("name")
			buildList {
				while (cursor.moveToNext()) {
					add(Machine(cursor.getString(id), cursor.getString(name), cursor.getString(state), cursor.getInt(airi) == 1))
				}
			}
		}.orEmpty()
	} catch (_: RuntimeException) {
		// No Minori, an older one, or one that won't answer: no machines either way.
		emptyList()
	}

	fun machine(context: Context, id: String): Machine? = machines(context).firstOrNull { it.id == id }

	/** A socket connected to Airi's agent in a running machine. */
	fun openAgent(context: Context, id: String): ParcelFileDescriptor = try {
		context.contentResolver.openFileDescriptor(MACHINES.buildUpon().appendPath(id).appendPath("agent").build(), "rw")
			?: throw IOException("Minori didn't connect to its machine")
	} catch (error: FileNotFoundException) {
		throw error
	} catch (error: RuntimeException) {
		throw IOException(error.message ?: "Minori didn't connect to its machine", error)
	}

	/** Asks Minori to start a machine. It's an activity, because only an app on screen may start Minori's service. */
	fun startIntent(id: String): Intent = Intent(ACTION_START).setPackage(PACKAGE).putExtra(EXTRA_MACHINE, id)

	/** Opens the machine in Minori with Airi's token, so Minori installs Airi's agent there. */
	fun setUpIntent(id: String, token: String): Intent =
		Intent(ACTION_SET_UP).setPackage(PACKAGE).putExtra(EXTRA_MACHINE, id).putExtra(EXTRA_TOKEN, token)

	fun installed(context: Context): Boolean = context.packageManager.getLaunchIntentForPackage(PACKAGE) != null
}
