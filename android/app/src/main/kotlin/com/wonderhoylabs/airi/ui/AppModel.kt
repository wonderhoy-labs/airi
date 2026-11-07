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
import java.io.IOException
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import org.json.JSONArray

data class LinuxApp(
    val id: String,
    val name: String,
    val comment: String,
    val icon: ImageBitmap? = null,
    val iconPng: ByteArray? = null,
    val onLauncher: Boolean = false,
    val adding: Boolean = false,
)

enum class Stage { CHECKING, NO_TERMINAL, SETUP, SETUP_AGAIN, UPDATING, READY }

data class UiState(val stage: Stage = Stage.CHECKING, val apps: List<LinuxApp> = emptyList())

/** Keeps a connection to the agent while Airi is on screen, and holds what the screen shows. */
class AppModel(private val context: Context) {
    private val mutableState = MutableStateFlow(UiState())
    val state = mutableState.asStateFlow()

    @Volatile private var connection: AgentConnection? = null

    private fun setStage(stage: Stage) = mutableState.update { it.copy(stage = stage) }

    /** Runs until cancelled. Closing the connection is what wakes it from a blocking read. */
    suspend fun run() {
        while (true) {
            val agent = try {
                AgentSetup.connect(context, Protocol.ROLE_CONTROL)
            } catch (_: WrongTokenException) {
                setStage(Stage.SETUP_AGAIN)
                delay(RETRY_MS)
                continue
            } catch (_: IOException) {
                setStage(if (AgentSetup.terminalIntent(context) == null) Stage.NO_TERMINAL else Stage.SETUP)
                delay(RETRY_MS)
                continue
            }
            connection = agent
            try {
                serve(agent)
            } catch (_: IOException) {
                // Linux stopped or the agent restarted, so go back to looking for it.
            } finally {
                connection = null
                agent.close()
            }
        }
    }

    fun disconnect() {
        connection?.close()
    }

    private fun serve(agent: AgentConnection) {
        if (agent.agentVersion != BuildConfig.VERSION_NAME && AgentSetup.hasBundledAgent(context)) {
            setStage(Stage.UPDATING)
            agent.send(Protocol.UPGRADE, Payload().bytes(AgentSetup.bundledAgent(context)))
            while (agent.read().type != Protocol.UPGRADED) Unit
            return
        }
        agent.send(Protocol.LIST_APPS)
        while (true) {
            val message = agent.read()
            when (message.type) {
                Protocol.APPS -> {
                    val apps = parseApps(message.rest().decodeToString())
                    mutableState.update { UiState(Stage.READY, apps) }
                    apps.forEach { agent.send(Protocol.GET_ICON, Payload().string(it.id)) }
                }
                Protocol.ICON -> {
                    val id = message.string()
                    val png = message.rest().takeIf(ByteArray::isNotEmpty) ?: continue
                    val bitmap = BitmapFactory.decodeByteArray(png, 0, png.size) ?: continue
                    updateApp(id) { it.copy(icon = bitmap.asImageBitmap(), iconPng = png) }
                }
            }
        }
    }

    private fun parseApps(json: String): List<LinuxApp> {
        val onLauncher = LauncherApps.installed(context)
        val list = JSONArray(json)
        return List(list.length()) { index ->
            val app = list.getJSONObject(index)
            val id = app.getString("id")
            LinuxApp(
                id = id,
                name = app.getString("name"),
                comment = app.optString("comment"),
                onLauncher = AppBuilder.packageFor(id) in onLauncher,
            )
        }
    }

    private fun updateApp(id: String, change: (LinuxApp) -> LinuxApp) {
        mutableState.update { state ->
            state.copy(apps = state.apps.map { if (it.id == id) change(it) else it })
        }
    }

    /** Checks the list against what's really installed, since you can remove apps from anywhere. */
    fun refreshLauncher() {
        val onLauncher = LauncherApps.installed(context)
        mutableState.update { state ->
            state.copy(
                apps = state.apps.map {
                    val present = AppBuilder.packageFor(it.id) in onLauncher
                    it.copy(onLauncher = present, adding = it.adding && !present)
                },
            )
        }
    }

    fun setAdding(id: String, adding: Boolean) = updateApp(id) { it.copy(adding = adding) }

    fun finishAdding(packageName: String) {
        mutableState.update { state ->
            state.copy(apps = state.apps.map { if (AppBuilder.packageFor(it.id) == packageName) it.copy(adding = false) else it })
        }
        refreshLauncher()
    }

    private companion object {
        const val RETRY_MS = 2000L
    }
}
