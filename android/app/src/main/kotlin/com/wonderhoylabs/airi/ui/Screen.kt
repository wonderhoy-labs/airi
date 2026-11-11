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

@Composable
fun Screen(
	state: UiState,
	onSetUp: () -> Unit,
	onOpenTerminal: () -> Unit,
	onOpenApp: (LinuxApp) -> Unit,
	onAdd: (LinuxApp) -> Unit,
	onRemove: (LinuxApp) -> Unit,
) {
	val insets = WindowInsets.safeDrawing.asPaddingValues()
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
			when (state.stage) {
				Stage.CHECKING -> item { Waiting(stringResource(R.string.status_checking)) }

				Stage.NO_TERMINAL -> item { NoTerminal() }

				Stage.SETUP -> item { SetUp(again = false, onSetUp) }

				Stage.SETUP_AGAIN -> item { SetUp(again = true, onSetUp) }

				Stage.LINUX_OFF -> item { LinuxOff(onOpenTerminal, onSetUp) }

				Stage.UPDATING -> item { Waiting(stringResource(R.string.status_updating)) }

				Stage.READY -> {
					if (state.apps.isEmpty()) {
						item { NoApps() }
					} else {
						item {
							Text(
								stringResource(R.string.apps_title),
								style = MaterialTheme.typography.titleMedium,
								modifier = Modifier.padding(top = 8.dp),
							)
						}
						items(state.apps, key = LinuxApp::id) { app ->
							AppRow(app, onOpenApp, onAdd, onRemove)
						}
					}
				}
			}
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
