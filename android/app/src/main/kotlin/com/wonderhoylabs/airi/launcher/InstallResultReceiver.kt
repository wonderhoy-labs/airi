package com.wonderhoylabs.airi.launcher

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller

/** Hears how an install went, and passes on Android's request for you to confirm it. */
class InstallResultReceiver : BroadcastReceiver() {
	override fun onReceive(context: Context, intent: Intent) {
		val packageName = intent.getStringExtra(LauncherApps.EXTRA_PACKAGE).orEmpty()
		when (intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)) {
			PackageInstaller.STATUS_PENDING_USER_ACTION -> {
				val confirm = intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent::class.java) ?: return
				context.startActivity(confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
			}

			PackageInstaller.STATUS_SUCCESS -> LauncherApps.report(LauncherApps.Result.Installed(packageName))

			else -> LauncherApps.report(
				LauncherApps.Result.Failed(packageName, intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE)),
			)
		}
	}
}
