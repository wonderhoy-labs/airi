package com.wonderhoylabs.airi.launcher

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.net.Uri
import android.provider.Settings
import com.wonderhoylabs.airi.client.Airi
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import java.io.File

/** Installs and removes the small apps that put Linux apps on the launcher. */
object LauncherApps {
	const val EXTRA_PACKAGE = "package"

	sealed interface Result {
		data class Installed(val packageName: String) : Result
		data class Failed(val packageName: String, val message: String?) : Result
	}

	private val mutableResults = MutableSharedFlow<Result>(extraBufferCapacity = 8)
	val results = mutableResults.asSharedFlow()

	internal fun report(result: Result) {
		mutableResults.tryEmit(result)
	}

	/** The packages of every app Airi has put on the launcher. */
	fun installed(context: Context): Set<String> = context.packageManager
		.queryIntentActivities(Intent(Airi.ACTION_LINUX_APP), PackageManager.ResolveInfoFlags.of(0))
		.map { it.activityInfo.packageName }
		.toSet()

	fun canInstall(context: Context): Boolean = context.packageManager.canRequestPackageInstalls()

	/** The Settings page where you allow Airi to install apps. */
	fun installPermissionIntent(context: Context): Intent =
		Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:${context.packageName}"))

	fun add(context: Context, desktopId: String, name: String, iconPng: ByteArray?) {
		AppIcons.save(context, desktopId, name, iconPng)
		val packageName = AppBuilder.packageFor(desktopId)
		val apk = File.createTempFile("launcher", ".apk", context.cacheDir)
		try {
			AppBuilder(context).build(desktopId, name, iconPng, apk)
			val installer = context.packageManager.packageInstaller
			val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL).apply {
				setAppPackageName(packageName)
				setAppLabel(name)
				setInstallReason(PackageManager.INSTALL_REASON_USER)
			}
			val sessionId = installer.createSession(params)
			installer.openSession(sessionId).use { session ->
				session.openWrite("app.apk", 0, apk.length()).use { out ->
					apk.inputStream().use { it.copyTo(out) }
					session.fsync(out)
				}
				val result = Intent(context, InstallResultReceiver::class.java).putExtra(EXTRA_PACKAGE, packageName)
				val callback = PendingIntent.getBroadcast(
					context,
					sessionId,
					result,
					PendingIntent.FLAG_MUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
				)
				session.commit(callback.intentSender)
			}
		} finally {
			apk.delete()
		}
	}

	/** Android asks you to confirm this itself. */
	fun removeIntent(desktopId: String): Intent =
		Intent(Intent.ACTION_DELETE, Uri.parse("package:${AppBuilder.packageFor(desktopId)}"))
}
