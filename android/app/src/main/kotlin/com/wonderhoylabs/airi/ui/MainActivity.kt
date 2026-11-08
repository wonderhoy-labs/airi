package com.wonderhoylabs.airi.ui

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.os.PersistableBundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import com.wonderhoylabs.airi.R
import com.wonderhoylabs.airi.agent.AgentSetup
import com.wonderhoylabs.airi.launcher.AppBuilder
import com.wonderhoylabs.airi.launcher.LauncherApps
import com.wonderhoylabs.airi.viewer.ViewerActivity
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

class MainActivity : ComponentActivity() {
	private lateinit var model: AppModel

	override fun onCreate(savedInstanceState: Bundle?) {
		super.onCreate(savedInstanceState)
		enableEdgeToEdge()
		model = AppModel(applicationContext)
		setContent {
			AiriTheme {
				val state = collectState()
				Screen(
					state = state,
					onSetUp = ::setUp,
					onOpenTerminal = ::openTerminal,
					onOpenApp = ::openApp,
					onAdd = ::addToLauncher,
					onRemove = { startActivity(LauncherApps.removeIntent(it.id)) },
				)
			}
		}
		lifecycleScope.launch {
			repeatOnLifecycle(Lifecycle.State.STARTED) {
				launch(Dispatchers.IO) { model.run() }
				launch { LauncherApps.results.collect(::onInstallResult) }
				try {
					kotlinx.coroutines.awaitCancellation()
				} finally {
					model.disconnect()
				}
			}
		}
	}

	@androidx.compose.runtime.Composable
	private fun collectState() = model.state.collectAsStateWithLifecycle().value

	override fun onResume() {
		super.onResume()
		model.refreshLauncher()
	}

	private fun onInstallResult(result: LauncherApps.Result) {
		when (result) {
			is LauncherApps.Result.Installed -> model.finishAdding(result.packageName)

			is LauncherApps.Result.Failed -> {
				model.finishAdding(result.packageName)
				Toast.makeText(this, result.message ?: getString(R.string.add_failed), Toast.LENGTH_LONG).show()
			}
		}
	}

	private fun openTerminal() {
		AgentSetup.terminalIntent(this)?.let(::startActivity)
	}

	/** Copies the install command and opens the Terminal app, so that all that's left is pasting it. */
	private fun setUp() {
		lifecycleScope.launch {
			val command = try {
				withContext(Dispatchers.IO) {
					check(AgentSetup.hasBundledAgent(this@MainActivity))
					AgentSetup.prepareInstall(this@MainActivity)
				}
			} catch (e: Exception) {
				Toast.makeText(this@MainActivity, getString(R.string.setup_failed, e.message), Toast.LENGTH_LONG).show()
				return@launch
			}
			val clip = ClipData.newPlainText(getString(R.string.app_name), command)
			// It holds the token, so keep it out of the keyboard's clipboard preview.
			clip.description.extras = PersistableBundle().apply {
				putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true)
			}
			getSystemService(ClipboardManager::class.java).setPrimaryClip(clip)
			openTerminal()
		}
	}

	private fun openApp(app: LinuxApp) {
		// Go through its launcher entry if it has one, so that there's only ever one task for it.
		val entry = packageManager.getLaunchIntentForPackage(AppBuilder.packageFor(app.id))
		if (entry != null) {
			startActivity(entry)
			return
		}
		val uri = Uri.Builder().scheme("airi").authority("app").appendPath(app.id).build()
		startActivity(
			Intent(Intent.ACTION_VIEW, uri, this, ViewerActivity::class.java)
				.addFlags(Intent.FLAG_ACTIVITY_NEW_DOCUMENT),
		)
	}

	private fun addToLauncher(app: LinuxApp) {
		if (!LauncherApps.canInstall(this)) {
			Toast.makeText(this, R.string.add_needs_permission, Toast.LENGTH_LONG).show()
			startActivity(LauncherApps.installPermissionIntent(this))
			return
		}
		model.setAdding(app.id, true)
		lifecycleScope.launch {
			try {
				withContext(Dispatchers.IO) { LauncherApps.add(this@MainActivity, app.id, app.name, app.iconPng) }
			} catch (e: Exception) {
				model.setAdding(app.id, false)
				Toast.makeText(this@MainActivity, e.message ?: getString(R.string.add_failed), Toast.LENGTH_LONG).show()
			}
		}
	}
}
