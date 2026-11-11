package com.wonderhoylabs.airi.ui

import android.content.Context
import android.graphics.BitmapFactory
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import com.wonderhoylabs.airi.BuildConfig
import com.wonderhoylabs.airi.agent.AgentConnection
import com.wonderhoylabs.airi.agent.AgentSetup
import com.wonderhoylabs.airi.agent.Payload
import com.wonderhoylabs.airi.agent.Protocol
import com.wonderhoylabs.airi.agent.WrongTokenException
import com.wonderhoylabs.airi.launcher.AppBuilder
import com.wonderhoylabs.airi.launcher.LauncherApps
import com.wonderhoylabs.airi.linux.Linux
import com.wonderhoylabs.airi.linux.Minori
import kotlinx.coroutines.Job
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.currentCoroutineContext
import kotlinx.coroutines.delay
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import org.json.JSONArray
import java.io.IOException
import java.util.concurrent.ConcurrentHashMap

data class LinuxApp(
	val linux: Linux,
	val id: String,
	val name: String,
	val comment: String,
	val icon: ImageBitmap? = null,
	val iconPng: ByteArray? = null,
	val onLauncher: Boolean = false,
	val adding: Boolean = false,
) {
	/** Tells the same app in two Linuxes apart. */
	val key: String get() = "${linux.key}/$id"
}

enum class Stage {
	CHECKING,
	UPDATING,
	READY,

	// Android's terminal
	NO_TERMINAL,
	SETUP,
	SETUP_AGAIN,
	LINUX_OFF,

	// A Minori machine
	NOT_SET_UP,
	STOPPED,
	STARTING,
	NOT_ANSWERING,
}

/** One Linux on Airi's screen, and what it's doing. [name] is the machine's, and empty for the terminal. */
data class Source(
	val linux: Linux,
	val name: String = "",
	val stage: Stage = Stage.CHECKING,
	val apps: List<LinuxApp> = emptyList(),
)

/** Android's terminal first, then Minori's machines in Minori's order. */
data class UiState(val sources: List<Source> = listOf(Source(Linux.Terminal))) {
	val apps: List<LinuxApp> get() = sources.flatMap { it.apps }
}

/**
 * Keeps a connection to the agent in every Linux Airi can reach while Airi is on screen, and holds what the screen
 * shows: Android's terminal, and each of Minori's machines that Airi's agent can run in.
 */
class AppModel(private val context: Context) {
	private val mutableState = MutableStateFlow(UiState())
	val state = mutableState.asStateFlow()

	private val connections = ConcurrentHashMap<Linux, AgentConnection>()

	@Volatile private var machines: Map<String, Minori.Machine> = emptyMap()

	private fun updateSource(linux: Linux, change: (Source) -> Source) {
		mutableState.update { state ->
			state.copy(sources = state.sources.map { if (it.linux == linux) change(it) else it })
		}
	}

	private fun setStage(linux: Linux, stage: Stage) = updateSource(linux) { it.copy(stage = stage) }

	/** Runs until cancelled. Closing the connections is what wakes them from a blocking read. */
	suspend fun run() = coroutineScope {
		launch { keepConnected(Linux.Terminal) }
		val running = mutableMapOf<String, Job>()
		while (true) {
			val found = Minori.machines(context)
			machines = found.associateBy { it.id }
			mutableState.update { state ->
				val terminal = state.sources.first { it.linux == Linux.Terminal }
				val known = state.sources.associateBy { it.linux }
				state.copy(
					sources = listOf(terminal) + found.map { machine ->
						(known[machine.linux] ?: Source(machine.linux)).copy(name = machine.name).let {
							if (machine.airi) it else it.copy(stage = Stage.NOT_SET_UP, apps = emptyList())
						}
					},
				)
			}
			for (machine in found.filter { it.airi }) {
				if (running[machine.id]?.isActive != true) running[machine.id] = launch { keepConnected(machine.linux) }
			}
			for (id in running.keys - found.filter { it.airi }.map { it.id }.toSet()) {
				running.remove(id)?.cancel()
				connections.remove(Linux.Machine(id))?.close()
			}
			delay(RETRY_MS)
		}
	}

	private suspend fun keepConnected(linux: Linux) {
		while (true) {
			currentCoroutineContext().ensureActive()
			if (linux is Linux.Machine) {
				val machine = machines[linux.id]
				if (machine == null || !machine.running) {
					setStage(linux, if (machine?.state == "starting") Stage.STARTING else Stage.STOPPED)
					updateSource(linux) { it.copy(apps = emptyList()) }
					delay(RETRY_MS)
					continue
				}
			}
			val agent = try {
				AgentSetup.connect(context, linux)
			} catch (_: WrongTokenException) {
				setStage(linux, Stage.SETUP_AGAIN)
				delay(RETRY_MS)
				continue
			} catch (_: IOException) {
				setStage(linux, unreachable(linux))
				delay(RETRY_MS)
				continue
			}
			connections[linux] = agent
			try {
				currentCoroutineContext().ensureActive()
				serve(linux, agent)
			} catch (_: IOException) {
				// Linux stopped or the agent restarted, so go back to looking for it.
			} finally {
				connections.remove(linux, agent)
				agent.close()
			}
		}
	}

	private fun unreachable(linux: Linux): Stage = when {
		linux is Linux.Machine -> Stage.NOT_ANSWERING
		AgentSetup.terminalIntent(context) == null -> Stage.NO_TERMINAL
		AgentSetup.connectedBefore(context) -> Stage.LINUX_OFF
		else -> Stage.SETUP
	}

	fun disconnect() {
		connections.values.forEach { it.close() }
	}

	private fun serve(linux: Linux, agent: AgentConnection) {
		if (agent.agentVersion != BuildConfig.VERSION_NAME && AgentSetup.hasBundledAgent(context)) {
			setStage(linux, Stage.UPDATING)
			agent.send(Protocol.UPGRADE, Payload().bytes(AgentSetup.bundledAgent(context)))
			while (agent.read().type != Protocol.UPGRADED) Unit
			return
		}
		agent.send(Protocol.LIST_APPS)
		while (true) {
			val message = agent.read()
			when (message.type) {
				Protocol.APPS -> {
					val apps = parseApps(linux, message.rest().decodeToString())
					updateSource(linux) { it.copy(stage = Stage.READY, apps = apps) }
					apps.forEach { agent.send(Protocol.GET_ICON, Payload().string(it.id)) }
				}

				Protocol.ICON -> {
					val id = message.string()
					val png = message.rest().takeIf(ByteArray::isNotEmpty) ?: continue
					val bitmap = BitmapFactory.decodeByteArray(png, 0, png.size) ?: continue
					updateApp("${linux.key}/$id") { it.copy(icon = bitmap.asImageBitmap(), iconPng = png) }
				}
			}
		}
	}

	private fun parseApps(linux: Linux, json: String): List<LinuxApp> {
		val onLauncher = LauncherApps.installed(context)
		val list = JSONArray(json)
		return List(list.length()) { index ->
			val app = list.getJSONObject(index)
			val id = app.getString("id")
			LinuxApp(
				linux = linux,
				id = id,
				name = app.getString("name"),
				comment = app.optString("comment"),
				onLauncher = AppBuilder.packageFor(linux, id) in onLauncher,
			)
		}
	}

	private fun updateApps(change: (LinuxApp) -> LinuxApp) {
		mutableState.update { state ->
			state.copy(sources = state.sources.map { source -> source.copy(apps = source.apps.map(change)) })
		}
	}

	private fun updateApp(key: String, change: (LinuxApp) -> LinuxApp) = updateApps {
		if (it.key ==
			key
		) {
			change(it)
		} else {
			it
		}
	}

	/** Checks the list against what's really installed, since you can remove apps from anywhere. */
	fun refreshLauncher() {
		val onLauncher = LauncherApps.installed(context)
		updateApps {
			val present = AppBuilder.packageFor(it.linux, it.id) in onLauncher
			it.copy(onLauncher = present, adding = it.adding && !present)
		}
	}

	fun setAdding(app: LinuxApp, adding: Boolean) = updateApp(app.key) { it.copy(adding = adding) }

	fun finishAdding(packageName: String) {
		updateApps { if (AppBuilder.packageFor(it.linux, it.id) == packageName) it.copy(adding = false) else it }
		refreshLauncher()
	}

	private companion object {
		const val RETRY_MS = 2000L
	}
}
