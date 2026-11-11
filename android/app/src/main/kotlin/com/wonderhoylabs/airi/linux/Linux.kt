package com.wonderhoylabs.airi.linux

import android.net.Uri
import com.wonderhoylabs.airi.client.Airi

/** One Linux that Airi shows apps from: Android's own terminal, or a machine in Minori. */
sealed interface Linux {
	/** Names it in links, the apps provider's `source` column and saved files: `terminal` or `minori:<machine ID>`. */
	val key: String

	/** The link that opens [desktopId] from this Linux. */
	fun appUri(desktopId: String): Uri

	/** Android's Linux terminal, reached on the port it forwards. */
	data object Terminal : Linux {
		override val key = "terminal"

		override fun appUri(desktopId: String): Uri = Airi.appUri(desktopId)
	}

	/** A machine in Minori with Airi's agent, reached through Minori. */
	data class Machine(val id: String) : Linux {
		override val key = "$MINORI_PREFIX$id"

		override fun appUri(desktopId: String): Uri = Airi.minoriAppUri(id, desktopId)
	}

	companion object {
		private const val MINORI_PREFIX = "minori:"

		fun fromKey(key: String): Linux? = when {
			key == Terminal.key -> Terminal
			key.startsWith(MINORI_PREFIX) && key.length > MINORI_PREFIX.length -> Machine(key.removePrefix(MINORI_PREFIX))
			else -> null
		}

		/** The Linux a link from [Airi.appUri] or [Airi.minoriAppUri] opens an app from, or null if it isn't one. */
		fun fromUri(uri: Uri?): Linux? {
			Airi.desktopId(uri) ?: return null
			return Airi.minoriMachine(uri)?.let(::Machine) ?: Terminal
		}
	}
}
