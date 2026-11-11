package com.wonderhoylabs.airi.agent

import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.provider.MediaStore
import com.wonderhoylabs.airi.BuildConfig
import com.wonderhoylabs.airi.linux.Linux
import com.wonderhoylabs.airi.linux.Minori
import java.io.File
import java.io.IOException
import java.net.InetSocketAddress
import java.security.SecureRandom
import java.util.Properties

/** Everything about reaching the agent, and getting it into Linux in the first place. */
object AgentSetup {
	const val TERMINAL_PACKAGE = "com.android.virtualization.terminal"
	private const val AGENT_ASSET = "airi-agent"
	private const val INSTALLER_ASSET = "install.sh"
	private const val PREFS = "airi"
	private const val TOKEN = "token"
	private const val CONNECTED_BEFORE = "connected_before"

	/**
	 * Debug builds can be pointed at another agent through this file in the app's external files.
	 * That's how an emulator, which can't run Linux, tests against a phone's over `adb forward`.
	 */
	private fun devOverrides(context: Context): Properties? {
		if (!BuildConfig.DEBUG) return null
		val file = File(context.getExternalFilesDir(null), "dev.properties")
		if (!file.exists()) return null
		return Properties().apply { file.inputStream().use(::load) }
	}

	fun address(context: Context): InetSocketAddress {
		val dev = devOverrides(context)
		return InetSocketAddress(
			dev?.getProperty("host") ?: "127.0.0.1",
			dev?.getProperty("port")?.toInt() ?: Protocol.PORT,
		)
	}

	private fun hexToken(context: Context): String {
		devOverrides(context)?.getProperty("token")?.let { return it }
		val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
		prefs.getString(TOKEN, null)?.let { return it }
		val bytes = ByteArray(32).also(SecureRandom()::nextBytes)
		val token = bytes.toHexString()
		prefs.edit().putString(TOKEN, token).apply()
		return token
	}

	fun token(context: Context): ByteArray = hexToken(context).hexToByteArray()

	/** Hands the token to Minori, which installs the agent in one of its machines with it. */
	fun minoriSetUpIntent(context: Context, machine: String) = Minori.setUpIntent(machine, hexToken(context))

	/** Connects to the agent in [linux]: on the terminal's forwarded port, or through Minori for one of its machines. */
	fun connect(context: Context, linux: Linux): AgentConnection = when (linux) {
		Linux.Terminal -> AgentConnection.connect(address(context), token(context)).also {
			context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putBoolean(CONNECTED_BEFORE, true).apply()
		}

		is Linux.Machine -> AgentConnection.connect(SocketLink(Minori.openAgent(context, linux.id)), token(context))
	}

	/** Tells someone who only needs to start Linux from someone who hasn't set Airi up yet. */
	fun connectedBefore(context: Context): Boolean =
		context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getBoolean(CONNECTED_BEFORE, false)

	fun terminalIntent(context: Context): Intent? = context.packageManager.getLaunchIntentForPackage(TERMINAL_PACKAGE)

	fun hasBundledAgent(context: Context): Boolean = runCatching { context.assets.open(AGENT_ASSET).close() }.isSuccess

	fun bundledAgent(context: Context): ByteArray = context.assets.open(AGENT_ASSET).use { it.readBytes() }

	/**
	 * Puts the agent where Linux can see it, and returns the command that installs it. Android has no
	 * way to run something in Linux for you, so this is the one step someone has to do by hand.
	 *
	 * The token travels in the command instead of a file, because anything in shared storage can be
	 * read by every app with storage access. The folder's name is new each time, because Android won't
	 * let this app replace files left by an earlier install of itself.
	 */
	fun prepareInstall(context: Context): String {
		val folder = "Airi-" + ByteArray(3).also(SecureRandom()::nextBytes).toHexString()
		for (name in listOf(AGENT_ASSET, INSTALLER_ASSET)) {
			val values = ContentValues().apply {
				put(MediaStore.Downloads.DISPLAY_NAME, name)
				put(MediaStore.Downloads.MIME_TYPE, "application/octet-stream")
				put(MediaStore.Downloads.RELATIVE_PATH, "Download/$folder")
			}
			val resolver = context.contentResolver
			val uri = resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI, values)
				?: throw IOException("Android wouldn't create $name")
			val output = resolver.openOutputStream(uri) ?: throw IOException("Android wouldn't open $name")
			output.use { out -> context.assets.open(name).use { it.copyTo(out) } }
		}
		// The space at the start keeps the token out of the shell's history.
		return " sh /mnt/shared/Download/$folder/install.sh ${hexToken(context)}"
	}
}
