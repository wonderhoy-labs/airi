package com.wonderhoylabs.airi.ui

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.wonderhoylabs.airi.R
import com.wonderhoylabs.airi.linux.Linux

/** What the screen can ask for. A source's actions act on that one Linux. */
interface ScreenActions {
	fun setUp()

	fun openTerminal()

	fun setUpInMinori(source: Source)

	fun startMachine(source: Source)

	fun openApp(app: LinuxApp)

	fun add(app: LinuxApp)

	fun remove(app: LinuxApp)
}

@Composable
fun Screen(state: UiState, actions: ScreenActions) {
	val insets = WindowInsets.safeDrawing.asPaddingValues()
	// Without the Terminal app, there's nothing to say about it if Minori's machines are here instead.
	val sources = state.sources.filter {
		it.linux != Linux.Terminal || it.stage != Stage.NO_TERMINAL ||
			state.sources.size == 1
	}
	val titled = sources.size > 1
	Surface(modifier = Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
		LazyColumn(
			contentPadding = PaddingValues(
				start = 20.dp,
				end = 20.dp,
				top = insets.calculateTopPadding() + 16.dp,
				bottom = insets.calculateBottomPadding() + 16.dp,
			),
			verticalArrangement = Arrangement.spacedBy(12.dp),
		) {
			item {
				BrandHeader(
					subtitle = stringResource(R.string.tagline),
					logo = painterResource(R.drawable.logo),
					modifier = Modifier.padding(bottom = 8.dp),
				)
			}
			for (source in sources) {
				if (titled || source.stage == Stage.READY) {
					item(key = "title-${source.linux.key}") { SourceTitle(source) }
				}
				sourceItems(source, actions)
			}
		}
	}
}

private fun LazyListScope.sourceItems(source: Source, actions: ScreenActions) {
	val name = source.name
	when (source.stage) {
		Stage.CHECKING -> item(key = source.linux.key) { Waiting(stringResource(R.string.status_checking)) }

		Stage.UPDATING -> item(key = source.linux.key) { Waiting(stringResource(R.string.status_updating)) }

		Stage.NO_TERMINAL -> item(key = source.linux.key) { NoTerminal() }

		Stage.SETUP -> item(key = source.linux.key) { SetUp(again = false, actions::setUp) }

		Stage.SETUP_AGAIN -> item(key = source.linux.key) {
			if (source.linux == Linux.Terminal) {
				SetUp(again = true, actions::setUp)
			} else {
				MachineCard(
					stringResource(R.string.machine_setup_again_title, name),
					stringResource(R.string.machine_setup_again_body, name),
					stringResource(R.string.machine_set_up_again),
				) { actions.setUpInMinori(source) }
			}
		}

		Stage.LINUX_OFF -> item(key = source.linux.key) { LinuxOff(actions::openTerminal, actions::setUp) }

		Stage.NOT_SET_UP -> item(key = source.linux.key) {
			MachineCard(
				stringResource(R.string.machine_setup_title, name),
				stringResource(R.string.machine_setup_body, name),
				stringResource(R.string.machine_set_up),
			) { actions.setUpInMinori(source) }
		}

		Stage.STOPPED -> item(key = source.linux.key) {
			MachineCard(
				stringResource(R.string.machine_off_title, name),
				stringResource(R.string.machine_off_body),
				stringResource(R.string.machine_start),
			) { actions.startMachine(source) }
		}

		Stage.STARTING -> item(key = source.linux.key) { Waiting(stringResource(R.string.machine_starting, name)) }

		Stage.NOT_ANSWERING -> item(key = source.linux.key) {
			MachineCard(
				stringResource(R.string.machine_not_answering_title, name),
				stringResource(R.string.machine_not_answering_body),
				stringResource(R.string.machine_set_up_again),
				outlined = true,
			) { actions.setUpInMinori(source) }
		}

		Stage.READY -> {
			if (source.apps.isEmpty()) {
				item(key = source.linux.key) { NoApps() }
			} else {
				items(source.apps, key = LinuxApp::key) { app ->
					AppRow(app, actions::openApp, actions::add, actions::remove)
				}
			}
		}
	}
}

@Composable
private fun SourceTitle(source: Source) {
	Row(verticalAlignment = Alignment.CenterVertically, modifier = Modifier.padding(top = 8.dp)) {
		Column {
			Text(
				if (source.linux == Linux.Terminal) stringResource(R.string.source_terminal) else source.name,
				style = MaterialTheme.typography.titleMedium,
			)
			Text(
				stringResource(if (source.linux == Linux.Terminal) R.string.source_terminal_detail else R.string.source_minori),
				style = MaterialTheme.typography.bodySmall,
				color = MaterialTheme.colorScheme.onSurfaceVariant,
			)
		}
	}
}

@Composable
private fun MachineCard(title: String, body: String, button: String, outlined: Boolean = false, onClick: () -> Unit) {
	InfoCard {
		Text(title, style = MaterialTheme.typography.titleMedium)
		Text(body, style = MaterialTheme.typography.bodyMedium)
		if (outlined) {
			OutlinedButton(onClick = onClick, modifier = Modifier.fillMaxWidth()) { Text(button) }
		} else {
			Button(onClick = onClick, modifier = Modifier.fillMaxWidth()) { Text(button) }
		}
	}
}

@Composable
private fun InfoCard(content: @Composable () -> Unit) {
	Card(
		modifier = Modifier.fillMaxWidth(),
		colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
	) {
		Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) { content() }
	}
}

@Composable
private fun Waiting(text: String) {
	InfoCard {
		Row(verticalAlignment = Alignment.CenterVertically) {
			CircularProgressIndicator(Modifier.size(20.dp), strokeWidth = 2.dp)
			Spacer(Modifier.width(14.dp))
			Text(text, style = MaterialTheme.typography.bodyLarge)
		}
	}
}

@Composable
private fun NoTerminal() {
	InfoCard {
		Text(stringResource(R.string.no_terminal_title), style = MaterialTheme.typography.titleMedium)
		Text(stringResource(R.string.no_terminal_body), style = MaterialTheme.typography.bodyMedium)
	}
}

@Composable
private fun NoApps() {
	InfoCard {
		Text(stringResource(R.string.no_apps_title), style = MaterialTheme.typography.titleMedium)
		Text(stringResource(R.string.no_apps_body), style = MaterialTheme.typography.bodyMedium)
	}
}

@Composable
private fun Step(number: Int, text: String) {
	Row {
		Box(
			contentAlignment = Alignment.Center,
			modifier = Modifier.size(24.dp).background(MaterialTheme.colorScheme.primaryContainer, CircleShape),
		) {
			Text(
				number.toString(),
				style = MaterialTheme.typography.labelMedium,
				color = MaterialTheme.colorScheme.onPrimaryContainer,
			)
		}
		Spacer(Modifier.width(12.dp))
		Text(text, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
	}
}

@Composable
private fun SetUp(again: Boolean, onSetUp: () -> Unit) {
	InfoCard {
		Text(
			stringResource(if (again) R.string.setup_again_title else R.string.setup_title),
			style = MaterialTheme.typography.titleMedium,
		)
		Text(
			stringResource(if (again) R.string.setup_again_body else R.string.setup_body),
			style = MaterialTheme.typography.bodyMedium,
		)
		Step(1, stringResource(R.string.setup_step_copy))
		Step(2, stringResource(R.string.setup_step_paste))
		Step(3, stringResource(R.string.setup_step_allow))
		Button(onClick = onSetUp, modifier = Modifier.fillMaxWidth()) {
			Text(stringResource(R.string.setup_button))
		}
	}
}

@Composable
private fun LinuxOff(onOpenTerminal: () -> Unit, onSetUp: () -> Unit) {
	InfoCard {
		Text(stringResource(R.string.linux_off_title), style = MaterialTheme.typography.titleMedium)
		Text(stringResource(R.string.linux_off_body), style = MaterialTheme.typography.bodyMedium)
		Button(onClick = onOpenTerminal, modifier = Modifier.fillMaxWidth()) {
			Text(stringResource(R.string.setup_start_linux))
		}
		OutlinedButton(onClick = onSetUp, modifier = Modifier.fillMaxWidth()) {
			Text(stringResource(R.string.linux_off_set_up_again))
		}
	}
}

@Composable
private fun AppRow(
	app: LinuxApp,
	onOpen: (LinuxApp) -> Unit,
	onAdd: (LinuxApp) -> Unit,
	onRemove: (LinuxApp) -> Unit,
) {
	Card(
		modifier = Modifier.fillMaxWidth().clip(RoundedCornerShape(16.dp)).clickable { onOpen(app) },
		shape = RoundedCornerShape(16.dp),
		colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainerLow),
	) {
		Column(Modifier.padding(horizontal = 14.dp, vertical = 12.dp)) {
			Row(verticalAlignment = Alignment.CenterVertically) {
				AppIcon(app)
				Spacer(Modifier.width(12.dp))
				Column(Modifier.weight(1f)) {
					Text(
						app.name,
						style = MaterialTheme.typography.bodyLarge,
						maxLines = 1,
						overflow = TextOverflow.Ellipsis,
					)
					if (app.comment.isNotEmpty()) {
						Text(
							app.comment,
							style = MaterialTheme.typography.bodySmall,
							color = MaterialTheme.colorScheme.onSurfaceVariant,
							maxLines = 2,
							overflow = TextOverflow.Ellipsis,
						)
					}
				}
			}
			Spacer(Modifier.height(10.dp))
			Row(
				verticalAlignment = Alignment.CenterVertically,
				horizontalArrangement = Arrangement.spacedBy(8.dp, Alignment.End),
				modifier = Modifier.fillMaxWidth(),
			) {
				when {
					app.adding -> CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)

					app.onLauncher -> OutlinedButton(onClick = { onRemove(app) }) {
						Text(stringResource(R.string.remove))
					}

					else -> FilledTonalButton(onClick = { onAdd(app) }) {
						Text(stringResource(R.string.add))
					}
				}
				Button(onClick = { onOpen(app) }) {
					Text(stringResource(R.string.launch))
				}
			}
		}
	}
}

@Composable
private fun AppIcon(app: LinuxApp) {
	val icon = app.icon
	if (icon != null) {
		Image(icon, contentDescription = null, modifier = Modifier.size(44.dp))
	} else {
		Box(
			contentAlignment = Alignment.Center,
			modifier = Modifier.size(
				44.dp,
			).background(MaterialTheme.colorScheme.primaryContainer, RoundedCornerShape(12.dp)),
		) {
			Text(
				app.name.take(1).uppercase(),
				style = MaterialTheme.typography.titleMedium,
				color = MaterialTheme.colorScheme.onPrimaryContainer,
			)
		}
	}
}
